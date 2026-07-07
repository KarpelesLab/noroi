/*
 * noroi.h — C bindings for the noroi terminal UI library.
 *
 * Build the C libraries (or just run `make capi`):
 *     cargo rustc --release --features capi --lib --crate-type cdylib
 *     cargo rustc --release --features capi --lib --crate-type staticlib
 * then link against target/release/libnoroi.a (static) or libnoroi.so (shared).
 *
 * Usage sketch:
 *     noroi_terminal *t = noroi_open();
 *     for (;;) {
 *         noroi_begin(t);
 *         noroi_box(t, 0, 0, 20, 3, NOROI_BORDER_ROUNDED, "Hello",
 *                   noroi_color_indexed(NOROI_CYAN), noroi_color_default(), 0);
 *         noroi_text(t, 2, 1, "Press q", noroi_color_default(),
 *                    noroi_color_none(), 0, 16);
 *         noroi_end(t);
 *         noroi_event ev;
 *         if (noroi_poll_event(t, 100, &ev) == 1 &&
 *             ev.kind == NOROI_EVENT_KEY && ev.ch == 'q') break;
 *     }
 *     noroi_close(t);
 *
 * All strings are NUL-terminated UTF-8. Functions taking a terminal accept NULL
 * (becoming no-ops / returning an error). This library is thread-compatible but
 * a single terminal handle must be used from one thread at a time.
 */
#ifndef NOROI_H
#define NOROI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque terminal handle. */
typedef struct NoroiTerminal noroi_terminal;

/* ---- colors ---------------------------------------------------------- */
/* Colors are opaque uint32_t values; build them with these constructors. */
uint32_t noroi_color_indexed(uint8_t index);       /* 0..255 palette/named  */
uint32_t noroi_color_rgb(uint8_t r, uint8_t g, uint8_t b); /* 24-bit truecolor */
uint32_t noroi_color_default(void);                 /* terminal default      */
uint32_t noroi_color_none(void);                    /* keep existing color   */

/* Named ANSI colors, for use with noroi_color_indexed(). */
enum {
    NOROI_BLACK = 0,  NOROI_RED = 1,     NOROI_GREEN = 2,   NOROI_YELLOW = 3,
    NOROI_BLUE = 4,   NOROI_MAGENTA = 5, NOROI_CYAN = 6,    NOROI_GRAY = 7,
    NOROI_DARKGRAY = 8, NOROI_LIGHTRED = 9, NOROI_LIGHTGREEN = 10,
    NOROI_LIGHTYELLOW = 11, NOROI_LIGHTBLUE = 12, NOROI_LIGHTMAGENTA = 13,
    NOROI_LIGHTCYAN = 14, NOROI_WHITE = 15
};

/* Text attribute bits (OR together). */
enum {
    NOROI_ATTR_BOLD          = 1 << 0,
    NOROI_ATTR_DIM           = 1 << 1,
    NOROI_ATTR_ITALIC        = 1 << 2,
    NOROI_ATTR_UNDERLINE     = 1 << 3,
    NOROI_ATTR_BLINK         = 1 << 4,
    NOROI_ATTR_REVERSE       = 1 << 5,
    NOROI_ATTR_HIDDEN        = 1 << 6,
    NOROI_ATTR_STRIKETHROUGH = 1 << 7
};

/* Border line styles for noroi_box(). */
enum {
    NOROI_BORDER_PLAIN   = 0,
    NOROI_BORDER_ROUNDED = 1,
    NOROI_BORDER_DOUBLE  = 2,
    NOROI_BORDER_THICK   = 3
};

/* Event kinds. */
enum {
    NOROI_EVENT_NONE   = 0,
    NOROI_EVENT_KEY    = 1,
    NOROI_EVENT_MOUSE  = 2,
    NOROI_EVENT_RESIZE = 3,
    NOROI_EVENT_PASTE  = 4,
    NOROI_EVENT_FOCUS  = 5
};

/* Special (non-character) key codes, in noroi_event.key. */
enum {
    NOROI_KEY_CHAR      = 0,   /* see noroi_event.ch */
    NOROI_KEY_ENTER     = 1,
    NOROI_KEY_TAB       = 2,
    NOROI_KEY_BACKSPACE = 3,
    NOROI_KEY_ESC       = 4,
    NOROI_KEY_LEFT      = 5,
    NOROI_KEY_RIGHT     = 6,
    NOROI_KEY_UP        = 7,
    NOROI_KEY_DOWN      = 8,
    NOROI_KEY_HOME      = 9,
    NOROI_KEY_END       = 10,
    NOROI_KEY_PAGEUP    = 11,
    NOROI_KEY_PAGEDOWN  = 12,
    NOROI_KEY_INSERT    = 13,
    NOROI_KEY_DELETE    = 14,
    NOROI_KEY_BACKTAB   = 15
    /* Function keys Fn are 100 + n (e.g. F1 == 101). */
};

/* Modifier bits, in noroi_event.modifiers. */
enum {
    NOROI_MOD_SHIFT = 1 << 0,
    NOROI_MOD_ALT   = 1 << 1,
    NOROI_MOD_CTRL  = 1 << 2
};

/* Mouse kinds, in noroi_event.mouse_kind. */
enum {
    NOROI_MOUSE_DOWN         = 0,
    NOROI_MOUSE_UP           = 1,
    NOROI_MOUSE_DRAG         = 2,
    NOROI_MOUSE_MOVED        = 3,
    NOROI_MOUSE_SCROLL_UP    = 4,
    NOROI_MOUSE_SCROLL_DOWN  = 5,
    NOROI_MOUSE_SCROLL_LEFT  = 6,
    NOROI_MOUSE_SCROLL_RIGHT = 7
};

typedef struct {
    int32_t  kind;         /* NOROI_EVENT_*                                   */
    uint32_t key;          /* NOROI_KEY_* (0 => character in `ch`)            */
    uint32_t ch;           /* Unicode scalar for a character key, else 0      */
    uint8_t  modifiers;    /* NOROI_MOD_* bitset                              */
    int32_t  mouse_kind;   /* NOROI_MOUSE_* for a mouse event, else -1        */
    int32_t  mouse_button; /* 0 left, 1 middle, 2 right, -1 none              */
    uint16_t x;            /* mouse col / resize cols / focus 1=gained,0=lost */
    uint16_t y;            /* mouse row / resize rows                         */
} noroi_event;

/* ---- lifecycle ------------------------------------------------------- */
noroi_terminal *noroi_open(void);
void            noroi_close(noroi_terminal *t);
int             noroi_size(noroi_terminal *t, uint16_t *cols, uint16_t *rows);

/* ---- per-frame drawing ----------------------------------------------- */
int  noroi_begin(noroi_terminal *t);   /* clear working buffer            */
int  noroi_end(noroi_terminal *t);     /* diff + flush to the screen      */
void noroi_set_cursor(noroi_terminal *t, uint16_t x, uint16_t y);

void noroi_text(noroi_terminal *t, uint16_t x, uint16_t y, const char *text,
                uint32_t fg, uint32_t bg, uint16_t attrs, uint16_t max_width);
void noroi_fill(noroi_terminal *t, uint16_t x, uint16_t y, uint16_t w, uint16_t h,
                const char *ch, uint32_t fg, uint32_t bg, uint16_t attrs);
void noroi_box(noroi_terminal *t, uint16_t x, uint16_t y, uint16_t w, uint16_t h,
               int border, const char *title, uint32_t fg, uint32_t bg, uint16_t attrs);
void noroi_gauge(noroi_terminal *t, uint16_t x, uint16_t y, uint16_t w, uint16_t h,
                 float ratio, uint32_t filled_fg, uint32_t filled_bg,
                 uint32_t unfilled_fg, uint32_t unfilled_bg);

/* ---- input ----------------------------------------------------------- */
/* Returns 1 if an event was written to *out, 0 on timeout, -1 on error.   */
int         noroi_poll_event(noroi_terminal *t, int timeout_ms, noroi_event *out);
/* Text of the most recent paste; valid until the next noroi_poll_event.   */
const char *noroi_paste_text(noroi_terminal *t);

#ifdef __cplusplus
}
#endif

#endif /* NOROI_H */
