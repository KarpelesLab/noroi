/*
 * A tiny C program using noroi's C bindings.
 *
 * Build the library, then this program (or just `make cdemo`):
 *     cargo rustc --release --features capi --lib --crate-type staticlib
 *     cc examples/demo.c -Iinclude -Ltarget/release -lnoroi \
 *        -lpthread -ldl -lm -o noroidemo_c
 *     ./noroidemo_c
 *
 * Controls: arrow keys move the marker, q quits.
 */
#include "noroi.h"
#include <stdio.h>

int main(void) {
    noroi_terminal *t = noroi_open();
    if (!t) {
        fprintf(stderr, "failed to open terminal\n");
        return 1;
    }

    uint16_t cols = 80, rows = 24;
    noroi_size(t, &cols, &rows);

    int x = (int)cols / 2, y = (int)rows / 2;
    float ratio = 0.0f;
    int running = 1;

    while (running) {
        noroi_begin(t);

        noroi_box(t, 0, 0, cols, rows, NOROI_BORDER_DOUBLE, " noroi (C) ",
                  noroi_color_indexed(NOROI_LIGHTCYAN), noroi_color_default(), 0);
        noroi_text(t, 2, 2, "Arrow keys move the marker, q to quit.",
                   noroi_color_default(), noroi_color_none(), 0, cols - 4);
        noroi_text(t, x, y, "@", noroi_color_indexed(NOROI_LIGHTYELLOW),
                   noroi_color_none(), NOROI_ATTR_BOLD, 1);

        noroi_gauge(t, 2, rows - 3, cols - 4, 1, ratio,
                    noroi_color_indexed(NOROI_BLACK), noroi_color_indexed(NOROI_GREEN),
                    noroi_color_indexed(NOROI_GRAY), noroi_color_indexed(NOROI_DARKGRAY));

        noroi_end(t);

        noroi_event ev;
        int r = noroi_poll_event(t, 100, &ev);
        if (r < 0) break;
        if (r == 0) { /* timeout: animate */
            ratio += 0.02f;
            if (ratio > 1.0f) ratio = 0.0f;
            continue;
        }
        if (ev.kind == NOROI_EVENT_KEY) {
            switch (ev.key) {
                case NOROI_KEY_CHAR: if (ev.ch == 'q') running = 0; break;
                case NOROI_KEY_LEFT:  if (x > 1) x--; break;
                case NOROI_KEY_RIGHT: if (x < cols - 2) x++; break;
                case NOROI_KEY_UP:    if (y > 1) y--; break;
                case NOROI_KEY_DOWN:  if (y < rows - 2) y++; break;
                case NOROI_KEY_ESC:   running = 0; break;
            }
        }
    }

    noroi_close(t);
    return 0;
}
