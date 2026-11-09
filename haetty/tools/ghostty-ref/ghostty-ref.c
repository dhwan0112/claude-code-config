/*
 * ghostty-ref: text-protocol reference engine on libghostty-vt, built from the
 * Ghostty checkout pinned in build.sh (commit e035beb).
 *
 * It mirrors haetty-core's ghostty.rs (libghostty-vt 0.2.2, Ghostty a887df4)
 * so that haetty-vtdiff can use it as its reference engine. The protocol, the
 * mapping and every deviation are documented in README.md.
 */
#define _POSIX_C_SOURCE 200809L

#include <ghostty/vt.h>

#include <limits.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>

/* haetty-core's CellFlags bits (crates/haetty-core/src/types.rs). */
enum {
  CF_BOLD = 1u << 0,
  CF_ITALIC = 1u << 1,
  CF_FAINT = 1u << 2,
  CF_UNDERLINE = 1u << 3,
  CF_STRIKETHROUGH = 1u << 4,
  CF_INVERSE = 1u << 5,
  CF_INVISIBLE = 1u << 6,
  CF_BLINK = 1u << 7,
  CF_OVERLINE = 1u << 8,
  CF_SELECTED = 1u << 9,
};

/* haetty's terminal setup (crates/haetty-vtdiff/src/main.rs, init()). */
static const GhosttyColorRgb kFg = {0xcc, 0xcc, 0xcc};
static const GhosttyColorRgb kBg = {0x14, 0x14, 0x14};
static const char kXtversion[] = "haetty(0.3.0)";

static void die(const char *what) {
  fprintf(stderr, "ghostty-ref: %s\n", what);
  exit(1);
}

/* ---- growable byte buffers ---- */

struct buf {
  unsigned char *p;
  size_t len;
  size_t cap;
};

/* Makes room for at least `cap` bytes. */
static void buf_reserve(struct buf *b, size_t cap) {
  size_t n;
  unsigned char *p;
  if (cap <= b->cap) return;
  n = b->cap ? b->cap : 256;
  while (n < cap) n *= 2;
  p = realloc(b->p, n);
  if (!p) die("out of memory");
  b->p = p;
  b->cap = n;
}

static void buf_put(struct buf *b, const void *data, size_t n) {
  buf_reserve(b, b->len + n);
  if (n) memcpy(b->p + b->len, data, n);
  b->len += n;
}

static void buf_c(struct buf *b, char c) {
  buf_reserve(b, b->len + 1);
  b->p[b->len++] = (unsigned char)c;
}

static void buf_s(struct buf *b, const char *s) { buf_put(b, s, strlen(s)); }

static void buf_f(struct buf *b, const char *fmt, ...) {
  char tmp[128];
  va_list ap;
  int n;
  va_start(ap, fmt);
  n = vsnprintf(tmp, sizeof tmp, fmt, ap);
  va_end(ap);
  if (n < 0) die("format error");
  if ((size_t)n < sizeof tmp) {
    buf_put(b, tmp, (size_t)n);
    return;
  }
  buf_reserve(b, b->len + (size_t)n + 1);
  va_start(ap, fmt);
  vsnprintf((char *)b->p + b->len, (size_t)n + 1, fmt, ap);
  va_end(ap);
  b->len += (size_t)n;
}

/* Lowercase hex of n bytes. */
static void buf_hex(struct buf *b, const void *data, size_t n) {
  static const char digits[] = "0123456789abcdef";
  const unsigned char *p = data;
  size_t i;
  buf_reserve(b, b->len + 2 * n);
  for (i = 0; i < n; i++) {
    b->p[b->len++] = (unsigned char)digits[p[i] >> 4];
    b->p[b->len++] = (unsigned char)digits[p[i] & 0xf];
  }
}

/* RRGGBB, lowercase, or "-" when the colour is absent. */
static void buf_color(struct buf *b, bool have, GhosttyColorRgb c) {
  unsigned char t[3];
  if (!have) {
    buf_c(b, '-');
    return;
  }
  t[0] = c.r;
  t[1] = c.g;
  t[2] = c.b;
  buf_hex(b, t, 3);
}

/* ---- the one terminal this process serves ---- */

struct srv {
  GhosttyTerminal term; /* NULL until a successful init */
  GhosttyRenderStateRowIterator rows;
  GhosttyRenderStateRowCells cells;
  uint16_t cols;   /* stored size, clamped as haetty's GridSize::clamped */
  uint16_t nrows;
  uint16_t cell_w; /* pixel size of a cell, at least 1 */
  uint16_t cell_h;
  struct buf replies; /* bytes the terminal wrote to the pty, not yet reported */
  struct buf text;    /* scratch: one cell's grapheme cluster */
  struct buf link;    /* scratch: one hyperlink URI */
  struct buf out;     /* one response, written with one fwrite */
};

static struct srv S;

/* ---- terminal callbacks (installed in srv_init, see README) ---- */

static void on_write_pty(GhosttyTerminal t, void *ud, const uint8_t *data, size_t len) {
  (void)t;
  buf_put(&((struct srv *)ud)->replies, data, len);
}

static void on_noop(GhosttyTerminal t, void *ud) {
  (void)t;
  (void)ud;
}

static GhosttyString on_xtversion(GhosttyTerminal t, void *ud) {
  GhosttyString s;
  (void)t;
  (void)ud;
  s.ptr = (const uint8_t *)kXtversion;
  s.len = sizeof kXtversion - 1;
  return s;
}

static bool on_size(GhosttyTerminal t, void *ud, GhosttySizeReportSize *out) {
  const struct srv *s = ud;
  (void)t;
  out->rows = s->nrows;
  out->columns = s->cols;
  out->cell_width = s->cell_w;
  out->cell_height = s->cell_h;
  return true;
}

static bool on_color_scheme(GhosttyTerminal t, void *ud, GhosttyColorScheme *out) {
  (void)t;
  (void)ud;
  *out = GHOSTTY_COLOR_SCHEME_DARK;
  return true;
}

/* 0.2.2's reply: VT220 with 132 columns, selective erase, ANSI colour and
 * clipboard; secondary type VT220, firmware 1, ROM 0; unit ID 0. */
static bool on_device_attributes(GhosttyTerminal t, void *ud, GhosttyDeviceAttributes *out) {
  (void)t;
  (void)ud;
  out->primary.conformance_level = GHOSTTY_DA_CONFORMANCE_VT220;
  out->primary.features[0] = GHOSTTY_DA_FEATURE_COLUMNS_132;
  out->primary.features[1] = GHOSTTY_DA_FEATURE_SELECTIVE_ERASE;
  out->primary.features[2] = GHOSTTY_DA_FEATURE_ANSI_COLOR;
  out->primary.features[3] = GHOSTTY_DA_FEATURE_CLIPBOARD;
  out->primary.num_features = 4;
  out->secondary.device_type = GHOSTTY_DA_DEVICE_TYPE_VT220;
  out->secondary.firmware_version = 1;
  out->secondary.rom_cartridge = 0;
  out->tertiary.unit_id = 0;
  return true;
}

/* Writes are not answered: the 0.2.2 engine never reported OSC 5522 writes. */
static void on_clipboard_write(GhosttyTerminal t, void *ud, const GhosttyClipboardWrite *w) {
  (void)t;
  (void)ud;
  (void)w;
}

/* ---- lifecycle ---- */

static uint16_t clamp16(unsigned long long v, unsigned lo, unsigned hi) {
  if (v < lo) return (uint16_t)lo;
  if (v > hi) return (uint16_t)hi;
  return (uint16_t)v;
}

static void srv_free(struct srv *s) {
  if (s->rows) ghostty_render_state_row_iterator_free(s->rows);
  if (s->cells) ghostty_render_state_row_cells_free(s->cells);
  if (s->term) ghostty_terminal_free(s->term);
  s->rows = NULL;
  s->cells = NULL;
  s->term = NULL;
  s->replies.len = 0;
}

/* NULL on success, else a message. Order as GhosttyEngine::new: callbacks,
 * colours, resize; the caller then drops any replies made so far. */
static const char *srv_init(struct srv *s, unsigned long long cols, unsigned long long rows,
                            unsigned long long cw, unsigned long long ch,
                            unsigned long long scrollback) {
  GhosttyTerminal t;
  GhosttyColorRgb palette[256];
  size_t max_bytes;

  srv_free(s);
  s->cols = clamp16(cols, 1, 4096);
  s->nrows = clamp16(rows, 1, 2048);
  s->cell_w = clamp16(cw, 1, 65535);
  s->cell_h = clamp16(ch, 1, 65535);
  if (ghostty_terminal_new(NULL, &s->term, s->cols, s->nrows) != GHOSTTY_SUCCESS || !s->term)
    return "terminal init failed";
  t = s->term;

  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_USERDATA, s);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_WRITE_PTY, (const void *)on_write_pty);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_BELL, (const void *)on_noop);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_TITLE_CHANGED, (const void *)on_noop);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_PWD_CHANGED, (const void *)on_noop);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_XTVERSION, (const void *)on_xtversion);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_SIZE, (const void *)on_size);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_COLOR_SCHEME, (const void *)on_color_scheme);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES,
                       (const void *)on_device_attributes);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE,
                       (const void *)on_clipboard_write);

  /* New default is 10,000 bytes; 0.2.2 took the byte count the caller gave. */
  max_bytes = (size_t)scrollback;
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, &max_bytes);
  /* Title report (CSI 21 t) and the glyph protocol stay at main's defaults, which are off: haetty answers
   * neither, so the reference must not either (tools/ghostty-ref/README.md). */

  /* TermColors: fg, bg, the built-in 256 palette (palette: None), no cursor colour. */
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, &kFg);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, &kBg);
  ghostty_color_palette_default(palette);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, palette);
  ghostty_terminal_set(t, GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, NULL);

  ghostty_terminal_resize(t, s->cols, s->nrows, s->cell_w, s->cell_h);

  if (ghostty_render_state_row_iterator_new(NULL, &s->rows) != GHOSTTY_SUCCESS)
    return "row iterator";
  if (ghostty_render_state_row_cells_new(NULL, &s->cells) != GHOSTTY_SUCCESS)
    return "row cells";
  s->replies.len = 0;
  return NULL;
}

/* ---- snapshot ---- */

static int shape_of(GhosttyCursorStyle st) {
  switch (st) {
    case GHOSTTY_CURSOR_STYLE_BAR:
      return 1;
    case GHOSTTY_CURSOR_STYLE_UNDERLINE:
      return 2;
    case GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW:
      return 3;
    default:
      return 0; /* BLOCK */
  }
}

static bool term_mode(GhosttyTerminal t, GhosttyMode m, bool dflt) {
  GhosttyTerminalModeConfig c;
  c.mode = m;
  c.value = false;
  if (ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_MODE, &c) != GHOSTTY_SUCCESS) return dflt;
  return c.value;
}

/* Resize. Main's core ends synchronized output (mode 2026) on every valid resize, and so did 0.2.2's C API
 * (c/terminal.zig: "Disable synchronized output mode so that we show changes immediately"). Nothing is put back. */
static void srv_resize(struct srv *s, uint16_t cols, uint16_t rows, uint16_t cw, uint16_t ch) {
  s->cols = cols;
  s->nrows = rows;
  s->cell_w = cw;
  s->cell_h = ch;
  ghostty_terminal_resize(s->term, cols, rows, cw, ch);
}

/* Rows of the viewport that are not in the scrollback: total - rows. */
static uint64_t sbrows_of(struct srv *s) {
  GhosttyTerminalScrollbar sb;
  memset(&sb, 0, sizeof sb);
  ghostty_terminal_get(s->term, GHOSTTY_TERMINAL_DATA_SCROLLBAR, &sb);
  return sb.total > s->nrows ? sb.total - s->nrows : 0;
}

/* The URI of the hyperlink at viewport cell (x, y); 0 when there is none. */
static size_t link_uri(struct srv *s, uint16_t x, uint32_t y) {
  GhosttyGridRef ref;
  GhosttyPoint pt;
  GhosttyResult r;
  size_t need = 0, got = 0;

  memset(&ref, 0, sizeof ref);
  memset(&pt, 0, sizeof pt);
  pt.tag = GHOSTTY_POINT_TAG_VIEWPORT;
  pt.value.coordinate.x = x;
  pt.value.coordinate.y = y;
  if (ghostty_terminal_grid_ref(s->term, pt, &ref) != GHOSTTY_SUCCESS) return 0;
  r = ghostty_grid_ref_hyperlink_uri(&ref, NULL, 0, &need);
  if (r != GHOSTTY_OUT_OF_SPACE || need == 0) return 0;
  buf_reserve(&s->link, need);
  if (ghostty_grid_ref_hyperlink_uri(&ref, s->link.p, need, &got) != GHOSTTY_SUCCESS) return 0;
  return got;
}

/* One cell token: flags:width:fg:bg:ul:ulk:ulc:text:link (see README). */
static void snap_cell(struct srv *s, const GhosttyRenderStateColors *col, bool row_links,
                      uint16_t x, uint32_t y) {
  GhosttyRenderStateRowCells c = s->cells;
  struct buf *o = &s->out;
  unsigned flags = 0, width = 0, ul = 0, ulk = 1;
  GhosttyColorRgb fg, bg, ulc;
  GhosttyCell raw = 0;
  GhosttyStyle st;
  uint32_t glen = 0;
  const unsigned char *text = NULL;
  size_t tlen = 0, llen = 0;
  bool have_fg = false, have_bg = false, have_ulc = false, have_raw = false;
  bool have_link = false, has_styling = false, selected = false;

  memset(&fg, 0, sizeof fg);
  memset(&bg, 0, sizeof bg);
  memset(&ulc, 0, sizeof ulc);
  memset(&st, 0, sizeof st);

  /* Text: the whole grapheme cluster as UTF-8; empty for a blank cell. */
  if (ghostty_render_state_row_cells_get(c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN,
                                         &glen) == GHOSTTY_SUCCESS &&
      glen > 0) {
    GhosttyBuffer gb;
    GhosttyResult r;
    gb.ptr = s->text.p;
    gb.cap = s->text.cap;
    gb.len = 0;
    r = ghostty_render_state_row_cells_get(
        c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &gb);
    if (r == GHOSTTY_OUT_OF_SPACE) {
      buf_reserve(&s->text, gb.len);
      gb.ptr = s->text.p;
      gb.cap = s->text.cap;
      gb.len = 0;
      r = ghostty_render_state_row_cells_get(
          c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &gb);
    }
    if (r == GHOSTTY_SUCCESS) {
      text = gb.ptr;
      tlen = gb.len;
    }
  }

  /* Resolved colours; NO_VALUE means the cell uses the default. */
  if (ghostty_render_state_row_cells_get(c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, &fg) ==
      GHOSTTY_SUCCESS)
    have_fg = true;
  if (ghostty_render_state_row_cells_get(c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, &bg) ==
      GHOSTTY_SUCCESS)
    have_bg = true;

  /* Attributes and underline: only for a styled cell, as in ghostty.rs. */
  if (ghostty_render_state_row_cells_get(c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING,
                                         &has_styling) == GHOSTTY_SUCCESS &&
      has_styling &&
      ghostty_render_state_row_cells_get(c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &st) ==
          GHOSTTY_SUCCESS) {
    uint16_t f = st.flags;
    int u = GHOSTTY_STYLE_UNDERLINE(st);

    if (f & GHOSTTY_STYLE_FLAG_BOLD) flags |= CF_BOLD;
    if (f & GHOSTTY_STYLE_FLAG_ITALIC) flags |= CF_ITALIC;
    if (f & GHOSTTY_STYLE_FLAG_FAINT) flags |= CF_FAINT;
    if (f & GHOSTTY_STYLE_FLAG_BLINK) flags |= CF_BLINK;
    if (f & GHOSTTY_STYLE_FLAG_INVERSE) flags |= CF_INVERSE;
    if (f & GHOSTTY_STYLE_FLAG_INVISIBLE) flags |= CF_INVISIBLE;
    if (f & GHOSTTY_STYLE_FLAG_STRIKETHROUGH) flags |= CF_STRIKETHROUGH;
    if (f & GHOSTTY_STYLE_FLAG_OVERLINE) flags |= CF_OVERLINE;
    if (u != GHOSTTY_SGR_UNDERLINE_NONE) {
      flags |= CF_UNDERLINE;
      ul = 1;
      switch (u) {
        case GHOSTTY_SGR_UNDERLINE_DOUBLE:
          ulk = 2;
          break;
        case GHOSTTY_SGR_UNDERLINE_CURLY:
          ulk = 3;
          break;
        case GHOSTTY_SGR_UNDERLINE_DOTTED:
          ulk = 4;
          break;
        case GHOSTTY_SGR_UNDERLINE_DASHED:
          ulk = 5;
          break;
        default:
          ulk = 1;
          break;
      }
    }
    switch (st.underline_color.tag) {
      case GHOSTTY_STYLE_COLOR_RGB:
        ulc = st.underline_color.value.rgb;
        have_ulc = true;
        break;
      case GHOSTTY_STYLE_COLOR_PALETTE:
        ulc = col->palette[st.underline_color.value.palette];
        have_ulc = true;
        break;
      default:
        break;
    }
  }

  /* Raw cell: wide-character state and the hyperlink flag. */
  if (ghostty_render_state_row_cells_get(c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &raw) ==
      GHOSTTY_SUCCESS)
    have_raw = true;
  if (have_raw) {
    GhosttyCellWide w = GHOSTTY_CELL_WIDE_NARROW;
    if (ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &w) == GHOSTTY_SUCCESS) {
      if (w == GHOSTTY_CELL_WIDE_WIDE)
        width = 1;
      else if (w == GHOSTTY_CELL_WIDE_SPACER_TAIL || w == GHOSTTY_CELL_WIDE_SPACER_HEAD)
        width = 2;
    }
  }

  if (ghostty_render_state_row_cells_get(c, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_SELECTED,
                                         &selected) == GHOSTTY_SUCCESS &&
      selected)
    flags |= CF_SELECTED;

  /* Link: only when the row and the cell both carry the hyperlink flag. */
  if (row_links && have_raw) {
    bool hl = false;
    if (ghostty_cell_get(raw, GHOSTTY_CELL_DATA_HAS_HYPERLINK, &hl) == GHOSTTY_SUCCESS && hl) {
      llen = link_uri(s, x, y);
      have_link = llen > 0;
    }
  }

  /* The token. */
  buf_c(o, ' ');
  buf_f(o, "%u:%u:", flags, width);
  buf_color(o, have_fg, fg);
  buf_c(o, ':');
  buf_color(o, have_bg, bg);
  buf_c(o, ':');
  buf_f(o, "%u:%u:", ul, ulk);
  buf_color(o, have_ulc, ulc);
  buf_c(o, ':');
  if (tlen)
    buf_hex(o, text, tlen);
  else
    buf_c(o, '-');
  buf_c(o, ':');
  if (have_link)
    buf_hex(o, s->link.p, llen);
  else
    buf_c(o, '-');
}

/* One full snapshot into s->out. NULL on success, else a message (the caller
 * then discards s->out). A fresh render state each time, as snapshot_fresh. */
static const char *snap(struct srv *s) {
  GhosttyRenderState rs = NULL;
  GhosttyRenderStateColors colors = GHOSTTY_INIT_SIZED(GhosttyRenderStateColors);
  GhosttyRenderStateCursor cur = GHOSTTY_INIT_SIZED(GhosttyRenderStateCursor);
  GhosttyString title, pwd;
  GhosttyTerminalScreen screen = GHOSTTY_TERMINAL_SCREEN_PRIMARY;
  GhosttyTerminalScrollbar sb;
  GhosttyTerminal t = s->term;
  struct buf *o = &s->out;
  const char *err = NULL;
  uint16_t cols = 0, rows = 0, y = 0;
  uint8_t kitty = 0;
  bool mouse = false, row_links = false;
  bool cursor_visible_mode;
  unsigned n;

  memset(&title, 0, sizeof title);
  memset(&pwd, 0, sizeof pwd);
  memset(&sb, 0, sizeof sb);

  if (ghostty_render_state_new(NULL, &rs) != GHOSTTY_SUCCESS) return "render state";
  if (ghostty_render_state_update(rs, t) != GHOSTTY_SUCCESS) {
    ghostty_render_state_free(rs);
    return "render state update";
  }
  ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_COLS, &cols);
  ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_ROWS, &rows);
  ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_COLORS, &colors);
  ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_CURSOR, &cur);

  /* Terminal-side state. The title and pwd are copied out right away. */
  ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &screen);
  ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING, &mouse);
  ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS, &kitty);
  ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_SCROLLBAR, &sb);
  cursor_visible_mode = term_mode(t, GHOSTTY_MODE_CURSOR_VISIBLE, true);

  buf_f(o, "size %u %u\n", (unsigned)cols, (unsigned)rows);
  buf_s(o, "colors ");
  buf_color(o, true, colors.foreground);
  buf_c(o, ' ');
  buf_color(o, true, colors.background);
  buf_c(o, ' ');
  buf_color(o, colors.cursor_has_value, colors.cursor);
  buf_c(o, '\n');

  ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_TITLE, &title);
  buf_s(o, "title ");
  buf_hex(o, title.ptr, title.len);
  buf_c(o, '\n');
  ghostty_terminal_get(t, GHOSTTY_TERMINAL_DATA_PWD, &pwd);
  buf_s(o, "pwd ");
  buf_hex(o, pwd.ptr, pwd.len);
  buf_c(o, '\n');

  buf_f(o, "modes %d %d %d %d %d %d %d %d %u\n",
        screen == GHOSTTY_TERMINAL_SCREEN_ALTERNATE ? 1 : 0, mouse ? 1 : 0,
        term_mode(t, GHOSTTY_MODE_MOUSE_FORMAT_SGR, false) ? 1 : 0,
        term_mode(t, GHOSTTY_MODE_BRACKETED_PASTE, false) ? 1 : 0,
        term_mode(t, GHOSTTY_MODE_FOCUS_EVENT, false) ? 1 : 0,
        term_mode(t, GHOSTTY_MODE_SYNCHRONIZED_OUTPUT, false) ? 1 : 0,
        term_mode(t, GHOSTTY_MODE_CURSOR_KEYS, false) ? 1 : 0,
        cursor_visible_mode ? 1 : 0, (unsigned)kitty);

  buf_f(o, "scroll %llu %llu %llu\n", (unsigned long long)sb.total,
        (unsigned long long)sb.offset, (unsigned long long)sb.len);

  /* The cursor only when the mode shows it and it is inside the viewport. */
  if (cur.visible && cur.viewport_has_value)
    buf_f(o, "cursor %u %u %d %d\n", (unsigned)cur.viewport_x, (unsigned)cur.viewport_y,
          shape_of(cur.visual_style), cur.blinking ? 1 : 0);
  else
    buf_s(o, "cursor none\n");

  /* Rows: exactly `rows` lines, each padded or cut to `cols` cells. */
  if (ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &s->rows) !=
      GHOSTTY_SUCCESS) {
    err = "row iterator";
  }
  while (!err && y < rows && ghostty_render_state_row_iterator_next(s->rows)) {
    GhosttyRenderStateRowCells cells = s->cells;
    GhosttyRow row = 0;
    row_links = false;
    if (ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_RAW, &row) ==
        GHOSTTY_SUCCESS)
      (void)ghostty_row_get(row, GHOSTTY_ROW_DATA_HYPERLINK, &row_links);
    if (ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &cells) !=
        GHOSTTY_SUCCESS) {
      err = "row cells";
      break;
    }
    s->cells = cells;
    buf_f(o, "r %u", (unsigned)y);
    n = 0;
    while (ghostty_render_state_row_cells_next(s->cells)) {
      if (n < cols) snap_cell(s, &colors, row_links, (uint16_t)n, y);
      n++;
    }
    for (; n < cols; n++) buf_s(o, " 0:0:-:-:0:1:-:-:-");
    buf_c(o, '\n');
    y++;
  }
  for (; !err && y < rows; y++) {
    buf_f(o, "r %u", (unsigned)y);
    for (n = 0; n < cols; n++) buf_s(o, " 0:0:-:-:0:1:-:-:-");
    buf_c(o, '\n');
  }
  buf_s(o, "end\n");

  ghostty_render_state_free(rs);
  return err;
}

/* ---- dump ---- */

static const char *dump_of(struct srv *s, GhosttyFormatterFormat fmt) {
  GhosttyFormatter f = NULL;
  GhosttyFormatterExtras extras = GHOSTTY_FORMATTER_EXTRAS_NONE;
  bool trim = true, unwrap = false;
  size_t need = 0, written = 0;
  GhosttyResult r;
  const char *err = NULL;

  if (ghostty_formatter_terminal_new(NULL, &f, s->term) != GHOSTTY_SUCCESS || !f)
    return "formatter";
  ghostty_formatter_set(f, GHOSTTY_FORMATTER_OPT_EMIT, &fmt);
  ghostty_formatter_set(f, GHOSTTY_FORMATTER_OPT_UNWRAP, &unwrap);
  ghostty_formatter_set(f, GHOSTTY_FORMATTER_OPT_TRIM, &trim);
  /* 0.2.2 zeroed every extra; the new default (palette, style, hyperlink) is off. */
  ghostty_formatter_set(f, GHOSTTY_FORMATTER_OPT_EXTRAS, &extras);

  r = ghostty_formatter_format_buf(f, NULL, 0, &need);
  if (r == GHOSTTY_OUT_OF_SPACE) {
    buf_reserve(&s->text, need);
    r = ghostty_formatter_format_buf(f, s->text.p, s->text.cap, &written);
    if (r == GHOSTTY_SUCCESS) buf_hex(&s->out, s->text.p, written);
    else err = "format";
  } else if (r == GHOSTTY_SUCCESS) {
    /* Empty output. */
  } else {
    err = "format";
  }
  ghostty_formatter_free(f);
  return err;
}

/* ---- protocol ---- */

static void flush_out(struct srv *s) {
  if (s->out.len) fwrite(s->out.p, 1, s->out.len, stdout);
  fflush(stdout);
  s->out.len = 0;
}

static void reply_err(struct srv *s, const char *text) {
  buf_s(&s->out, "err ");
  buf_s(&s->out, text);
  buf_c(&s->out, '\n');
}

static int parse_u(const char *s, unsigned long long *out) {
  unsigned long long v = 0;
  const char *q;
  if (!*s) return -1;
  for (q = s; *q; q++) {
    unsigned d;
    if (*q < '0' || *q > '9') return -1;
    d = (unsigned)(*q - '0');
    if (v > (ULLONG_MAX - d) / 10) return -1;
    v = v * 10 + d;
  }
  *out = v;
  return 0;
}

static int parse_i(const char *s, long long *out) {
  unsigned long long u;
  if (*s == '-') {
    if (parse_u(s + 1, &u) || u > (unsigned long long)LLONG_MAX) return -1;
    *out = -(long long)u;
    return 0;
  }
  if (parse_u(s, &u) || u > (unsigned long long)LLONG_MAX) return -1;
  *out = (long long)u;
  return 0;
}

static int hex_val(int c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  return -1;
}

/* Decodes hex into s->text (scratch). Returns the byte count, or -1. */
static long hex_decode(struct srv *s, const char *h) {
  size_t n = strlen(h), i;
  if (n % 2) return -1;
  buf_reserve(&s->text, n / 2 + 1);
  for (i = 0; i < n; i += 2) {
    int a = hex_val((unsigned char)h[i]), b = hex_val((unsigned char)h[i + 1]);
    if (a < 0 || b < 0) return -1;
    s->text.p[i / 2] = (unsigned char)(a * 16 + b);
  }
  return (long)(n / 2);
}

/* The next space-delimited word; *pp moves past it. "" when none is left. */
static char *next_word(char **pp) {
  char *p = *pp, *start;
  while (*p == ' ' || *p == '\t') p++;
  start = p;
  while (*p && *p != ' ' && *p != '\t') p++;
  if (*p) *p++ = '\0';
  *pp = p;
  return start;
}

static bool only_spaces(const char *p) {
  while (*p == ' ' || *p == '\t') p++;
  return *p == '\0';
}

/* Handles one request line. Returns true for quit. */
static bool handle(char *line) {
  struct srv *s = &S;
  char *p = line;
  char *cmd = next_word(&p);
  bool quit = false;

  if (!strcmp(cmd, "quit")) {
    quit = true;
  } else if (!strcmp(cmd, "init")) {
    unsigned long long v[5];
    int i, bad = 0;
    for (i = 0; i < 5; i++) {
      if (parse_u(next_word(&p), &v[i])) bad = 1;
    }
    if (bad || !only_spaces(p)) {
      reply_err(s, "init takes five unsigned numbers");
    } else {
      const char *e = srv_init(s, v[0], v[1], v[2], v[3], v[4]);
      if (e) {
        srv_free(s); /* a failed init leaves no terminal behind */
        reply_err(s, e);
      } else {
        buf_s(&s->out, "ok\n");
      }
    }
  } else if (!s->term) {
    reply_err(s, "not initialised: send init first");
  } else if (!strcmp(cmd, "feed")) {
    char *h = next_word(&p);
    long n;
    if (!only_spaces(p)) {
      reply_err(s, "feed takes one hex argument");
    } else if ((n = hex_decode(s, h)) < 0) {
      reply_err(s, "bad hex");
    } else {
      if (n > 0) ghostty_terminal_vt_write(s->term, s->text.p, (size_t)n);
      buf_s(&s->out, "ok\n");
    }
  } else if (!strcmp(cmd, "resize")) {
    unsigned long long v[4];
    int i, bad = 0;
    for (i = 0; i < 4; i++) {
      if (parse_u(next_word(&p), &v[i])) bad = 1;
    }
    if (bad || !only_spaces(p)) {
      reply_err(s, "resize takes four unsigned numbers");
    } else {
      srv_resize(s, clamp16(v[0], 1, 4096), clamp16(v[1], 1, 2048), clamp16(v[2], 1, 65535),
                 clamp16(v[3], 1, 65535));
      buf_s(&s->out, "ok\n");
    }
  } else if (!strcmp(cmd, "replies")) {
    buf_s(&s->out, "replies ");
    buf_hex(&s->out, s->replies.p, s->replies.len);
    buf_c(&s->out, '\n');
    s->replies.len = 0;
  } else if (!strcmp(cmd, "snap")) {
    const char *e;
    s->out.len = 0;
    e = snap(s);
    if (e) {
      s->out.len = 0;
      reply_err(s, e);
    }
  } else if (!strcmp(cmd, "scroll")) {
    char *what = next_word(&p);
    GhosttyTerminalScrollViewport v;
    memset(&v, 0, sizeof v);
    if (!strcmp(what, "top")) {
      v.tag = GHOSTTY_TERMINAL_SCROLL_VIEWPORT_TOP;
    } else if (!strcmp(what, "bottom")) {
      v.tag = GHOSTTY_TERMINAL_SCROLL_VIEWPORT_BOTTOM;
    } else if (!strcmp(what, "delta")) {
      long long d;
      if (parse_i(next_word(&p), &d)) {
        reply_err(s, "scroll delta needs a signed number");
        flush_out(s);
        return quit;
      }
      v.tag = GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA;
      v.value.delta = (intptr_t)d;
    } else {
      reply_err(s, "scroll takes top, bottom or delta N");
      flush_out(s);
      return quit;
    }
    ghostty_terminal_scroll_viewport(s->term, v);
    buf_s(&s->out, "ok\n");
  } else if (!strcmp(cmd, "sbrows")) {
    buf_f(&s->out, "sbrows %llu\n", (unsigned long long)sbrows_of(s));
  } else if (!strcmp(cmd, "dump")) {
    char *which = next_word(&p);
    GhosttyFormatterFormat fmt;
    const char *e;
    if (!strcmp(which, "plain"))
      fmt = GHOSTTY_FORMATTER_FORMAT_PLAIN;
    else if (!strcmp(which, "vt"))
      fmt = GHOSTTY_FORMATTER_FORMAT_VT;
    else if (!strcmp(which, "html"))
      fmt = GHOSTTY_FORMATTER_FORMAT_HTML;
    else {
      reply_err(s, "dump takes plain, vt or html");
      flush_out(s);
      return quit;
    }
    s->out.len = 0;
    buf_s(&s->out, "dump ");
    e = dump_of(s, fmt);
    if (e) {
      s->out.len = 0;
      reply_err(s, e);
    } else {
      buf_c(&s->out, '\n');
    }
  } else {
    reply_err(s, "unknown command");
  }
  flush_out(s);
  return quit;
}

int main(void) {
  char *line = NULL;
  size_t cap = 0;
  ssize_t n;
  while ((n = getline(&line, &cap, stdin)) >= 0) {
    while (n > 0 && (line[n - 1] == '\n' || line[n - 1] == '\r')) line[--n] = '\0';
    if (handle(line)) break;
  }
  free(line);
  srv_free(&S);
  free(S.replies.p);
  free(S.text.p);
  free(S.link.p);
  free(S.out.p);
  return 0;
}
