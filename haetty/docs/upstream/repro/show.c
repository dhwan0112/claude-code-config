/* show COLS ROWS BYTES: writes BYTES to a terminal of COLS x ROWS and prints every non-blank row (0-based index),
 * as a fresh render state has it, with each character's column (1-based). Used for drafts 01 and 02. */
#include <ghostty/vt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do { if ((x) != GHOSTTY_SUCCESS) { fprintf(stderr, "%s failed\n", #x); exit(2); } } while (0)
int main(int argc, char **argv) {
  if (argc != 4) { fprintf(stderr, "usage: show COLS ROWS BYTES\n"); return 2; }
  uint16_t cols = (uint16_t)atoi(argv[1]), rows = (uint16_t)atoi(argv[2]);
  GhosttyTerminal t; GhosttyRenderState rs; GhosttyRenderStateRowIterator it; GhosttyRenderStateRowCells cells;
  CHECK(ghostty_terminal_new(NULL, &t, cols, rows));
  CHECK(ghostty_render_state_new(NULL, &rs));
  CHECK(ghostty_render_state_row_iterator_new(NULL, &it));
  CHECK(ghostty_render_state_row_cells_new(NULL, &cells));
  ghostty_terminal_vt_write(t, (const uint8_t *)argv[3], strlen(argv[3]));
  CHECK(ghostty_render_state_update(rs, t));
  CHECK(ghostty_render_state_get(rs, GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &it));
  int y = 0;
  while (ghostty_render_state_row_iterator_next(it)) {
    CHECK(ghostty_render_state_row_get(it, GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &cells));
    int x = 0, any = 0;
    while (ghostty_render_state_row_cells_next(cells)) {
      char gbuf[64]; GhosttyBuffer gb; gb.ptr = (uint8_t *)gbuf; gb.cap = sizeof gbuf; gb.len = 0;
      if (ghostty_render_state_row_cells_get(cells, GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &gb) == GHOSTTY_SUCCESS && gb.len > 0) {
        printf("row %d col %d: %.*s\n", y, x + 1, (int)gb.len, gbuf); any = 1;
      }
      x++;
    }
    if (!any) { /* blank row: nothing printed */ }
    y++;
  }
  return 0;
}
