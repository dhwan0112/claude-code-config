/* Draft 03 (docs/upstream/03-render-state-unmarked-rows.md) on the pinned Ghostty (e035beb):
 * one render state kept across updates (the incremental path) against a fresh render state read from the same
 * terminal. Prints, per case, the row as the kept state has it and as a fresh state has it.
 *
 * Build, after tools/ghostty-ref/build.sh (PREFIX = its install folder, the one with lib/ and include/):
 *   zig cc -std=c11 -O2 -DGHOSTTY_STATIC -I $PREFIX/include repro03.c $PREFIX/lib/libghostty-vt.a -o repro03
 */
#include <ghostty/vt.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(x)                                                                         \
  do {                                                                                   \
    if ((x) != GHOSTTY_SUCCESS) {                                                        \
      fprintf(stderr, "%s failed at line %d\n", #x, __LINE__);                           \
      exit(2);                                                                           \
    }                                                                                    \
  } while (0)

typedef struct {
  GhosttyTerminal term;
  GhosttyRenderState kept; /* updated again and again: the incremental path */
  GhosttyRenderStateRowIterator rows;
  GhosttyRenderStateRowCells cells;
} Screen;

static void screen_new(Screen *s, uint16_t cols, uint16_t rows) {
  CHECK(ghostty_terminal_new(NULL, &s->term, cols, rows));
  CHECK(ghostty_render_state_new(NULL, &s->kept));
  CHECK(ghostty_render_state_row_iterator_new(NULL, &s->rows));
  CHECK(ghostty_render_state_row_cells_new(NULL, &s->cells));
}

static void feed(Screen *s, const char *bytes, size_t n) {
  ghostty_terminal_vt_write(s->term, (const uint8_t *)bytes, n);
}

static void update(GhosttyRenderState rs, GhosttyTerminal t) {
  CHECK(ghostty_render_state_update(rs, t));
}

/* Row y of render state rs: its text (blank cells as spaces) and its dirty flag. */
static void row_text(Screen *s, GhosttyRenderState rs, int y, char *out, size_t cap, bool *dirty) {
  int i = 0;
  size_t n = 0;
  CHECK(ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &s->rows));
  while (ghostty_render_state_row_iterator_next(s->rows)) {
    if (i++ == y) break;
  }
  CHECK(ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY, dirty));
  CHECK(ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &s->cells));
  while (ghostty_render_state_row_cells_next(s->cells)) {
    char gbuf[64];
    GhosttyBuffer gb;
    gb.ptr = (uint8_t *)gbuf;
    gb.cap = sizeof gbuf;
    gb.len = 0;
    if (ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                                           &gb) == GHOSTTY_SUCCESS &&
        gb.len > 0) {
      if (n + gb.len < cap) {
        memcpy(out + n, gb.ptr, gb.len);
        n += gb.len;
      }
    } else if (n + 1 < cap) {
      out[n++] = ' ';
    }
  }
  out[n] = 0;
}

/* The width attribute of cell x of row y. */
static GhosttyCellWide wide_at(Screen *s, GhosttyRenderState rs, int y, int x) {
  int i = 0;
  int col = 0;
  GhosttyCellWide w = GHOSTTY_CELL_WIDE_NARROW;
  CHECK(ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &s->rows));
  while (ghostty_render_state_row_iterator_next(s->rows)) {
    if (i++ == y) break;
  }
  CHECK(ghostty_render_state_row_get(s->rows, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &s->cells));
  while (ghostty_render_state_row_cells_next(s->cells)) {
    if (col++ == x) {
      GhosttyCell raw;
      CHECK(ghostty_render_state_row_cells_get(s->cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &raw));
      CHECK(ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &w));
      break;
    }
  }
  return w;
}

static const char *wide_name(GhosttyCellWide w) {
  switch (w) {
    case GHOSTTY_CELL_WIDE_NARROW: return "narrow";
    case GHOSTTY_CELL_WIDE_WIDE: return "wide";
    case GHOSTTY_CELL_WIDE_SPACER_TAIL: return "spacer_tail";
    case GHOSTTY_CELL_WIDE_SPACER_HEAD: return "spacer_head";
    default: return "?";
  }
}

/* Reads the row as the kept state and as a fresh state have it, after the terminal's current contents. */
static int compare_row(Screen *s, int y, const char *title) {
  char inc[256], fresh_text[256];
  bool dirty = false, fresh_dirty = false;
  /* The kept state is read before anything else touches the terminal. */
  row_text(s, s->kept, y, inc, sizeof inc, &dirty);
  GhosttyRenderState fresh;
  CHECK(ghostty_render_state_new(NULL, &fresh));
  update(fresh, s->term);
  row_text(s, fresh, y, fresh_text, sizeof fresh_text, &fresh_dirty);
  int stale = strcmp(inc, fresh_text) != 0;
  printf("%-46s kept: dirty=%d \"%s\"   fresh: \"%s\"   %s\n", title, dirty, inc, fresh_text,
         stale ? "STALE" : "same");
  ghostty_render_state_free(fresh);
  return stale;
}

/* A consumer renders the kept state and then marks it consumed (ghostty_render_state_clean), as a frame
 * renderer does, so each update's row dirty flags are its own. */
static void render_and_clean(Screen *s) {
  char buf[256];
  bool d = false;
  row_text(s, s->kept, 0, buf, sizeof buf, &d);
  CHECK(ghostty_render_state_clean(s->kept));
}

/* Base in one read, the mark in a later read (with an update in between, as an application that
 * updates once per frame does). */
static int join_case(const char *title, const char *base, size_t nb, const char *mark, size_t nm) {
  Screen s;
  screen_new(&s, 20, 4);
  feed(&s, base, nb);
  update(s.kept, s.term);
  render_and_clean(&s);
  feed(&s, mark, nm);
  update(s.kept, s.term);
  return compare_row(&s, 0, title);
}

static int same_write_case(void) {
  Screen s;
  screen_new(&s, 20, 4);
  feed(&s, "cafe\xcc\x81", 6);
  update(s.kept, s.term);
  return compare_row(&s, 0, "control: cafe + U+0301 in one write");
}

static int split_fe0f_case(void) {
  Screen s;
  screen_new(&s, 20, 4);
  feed(&s, "\xe2\x9d\xa4", 3); /* U+2764 */
  update(s.kept, s.term);
  render_and_clean(&s);
  feed(&s, "\xef\xb8", 2); /* the first two bytes of U+FE0F */
  update(s.kept, s.term);
  render_and_clean(&s);
  feed(&s, "\x8f", 1);
  update(s.kept, s.term);
  return compare_row(&s, 0, "U+2764, then U+FE0F split over two writes");
}

/* Case 2 of the draft: row 1 (0-based) holds a spacer_head at column 79 after an emoji wraps; the next
 * write overwrites the emoji's first cell and the spacer goes back to narrow in the terminal. */
static int spacer_case(void) {
  Screen s;
  screen_new(&s, 80, 4);
  feed(&s, "\x1b[2;1H", 6);
  for (int i = 0; i < 79; i++) feed(&s, "w", 1);
  feed(&s, "\xf0\x9f\x98\x80", 4); /* U+1F600: does not fit in column 80 */
  update(s.kept, s.term);
  render_and_clean(&s);
  feed(&s, "\x1b[3;1Hx", 7);
  update(s.kept, s.term);
  GhosttyCellWide a = wide_at(&s, s.kept, 1, 79); /* the kept state, read before anything else */
  GhosttyRenderState fresh;
  CHECK(ghostty_render_state_new(NULL, &fresh));
  update(fresh, s.term);
  GhosttyCellWide b = wide_at(&s, fresh, 1, 79);
  int stale = a != b;
  printf("%-46s kept: %s   fresh: %s   %s\n", "spacer head cleared by a write (row 1, col 80)",
         wide_name(a), wide_name(b), stale ? "STALE" : "same");
  ghostty_render_state_free(fresh);
  return stale;
}

int main(void) {
  int stale = 0;
  stale += same_write_case();
  stale += join_case("cafe, then U+0301 in a later read", "cafe", 4, "\xcc\x81", 2);
  stale += join_case("U+2764, then U+FE0F in a later read", "\xe2\x9d\xa4", 3, "\xef\xb8\x8f", 3);
  stale += join_case("U+263A, then U+FE0F in a later read", "\xe2\x98\xba", 3, "\xef\xb8\x8f", 3);
  stale += split_fe0f_case();
  stale += spacer_case();
  printf("%s\n", stale ? "result: stale rows in the kept render state" : "result: no stale rows");
  return stale ? 1 : 0;
}
