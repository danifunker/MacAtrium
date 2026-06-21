/*
 * json.c — see json.h. Hand-written, allocation-free, MacRoman-safe (bytes > 127
 * pass through untouched). Tolerates CR / LF / CRLF inside whitespace.
 */
#include "json.h"

#include <string.h>

/* ---- low-level cursor ----------------------------------------------------- */

typedef struct {
    const char *p;
    const char *end;
} Cur;

static int at_end(Cur *c) { return c->p >= c->end; }

static void skip_ws(Cur *c)
{
    while (!at_end(c)) {
        char ch = *c->p;
        if (ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n')
            c->p++;
        else
            break;
    }
}

/* Append one char to a bounded buffer (keeps room for the NUL). */
static void push(char *buf, int cap, int *n, char ch)
{
    if (*n < cap - 1) buf[(*n)++] = ch;
}

/* Parse a JSON string token (assumes *c->p == '"'); writes into out/cap.
 * Returns 1 on success. */
static int parse_string(Cur *c, char *out, int cap)
{
    int n = 0;
    if (at_end(c) || *c->p != '"') return 0;
    c->p++;
    while (!at_end(c)) {
        char ch = *c->p++;
        if (ch == '"') { out[n] = '\0'; return 1; }
        if (ch == '\\') {
            if (at_end(c)) return 0;
            char e = *c->p++;
            switch (e) {
                case 'n': push(out, cap, &n, '\n'); break;
                case 't': push(out, cap, &n, '\t'); break;
                case 'r': push(out, cap, &n, '\r'); break;
                case 'b': push(out, cap, &n, '\b'); break;
                case 'f': push(out, cap, &n, '\f'); break;
                case '/': push(out, cap, &n, '/');  break;
                case '\\': push(out, cap, &n, '\\'); break;
                case '"': push(out, cap, &n, '"');  break;
                case 'u': {
                    /* \uXXXX — keep it simple: ASCII range copied, anything
                     * else becomes '?'. The host transcodes to MacRoman before
                     * emit, so \u should be rare in practice. */
                    int i, v = 0, ok = 1;
                    for (i = 0; i < 4 && !at_end(c); i++) {
                        char h = *c->p++;
                        v <<= 4;
                        if (h >= '0' && h <= '9') v |= h - '0';
                        else if (h >= 'a' && h <= 'f') v |= h - 'a' + 10;
                        else if (h >= 'A' && h <= 'F') v |= h - 'A' + 10;
                        else { ok = 0; break; }
                    }
                    push(out, cap, &n, (ok && v >= 0x20 && v < 0x7f) ? (char)v : '?');
                    break;
                }
                default: push(out, cap, &n, e); break;
            }
        } else {
            push(out, cap, &n, ch);
        }
    }
    return 0; /* unterminated */
}

/* Parse a number token into *out (integer part). Returns 1 on success. */
static int parse_number(Cur *c, long *out)
{
    long sign = 1, v = 0;
    int any = 0;
    if (!at_end(c) && *c->p == '-') { sign = -1; c->p++; }
    while (!at_end(c) && *c->p >= '0' && *c->p <= '9') {
        v = v * 10 + (*c->p - '0');
        c->p++;
        any = 1;
    }
    /* swallow any fractional / exponent part; we only keep the integer value */
    if (!at_end(c) && *c->p == '.') {
        c->p++;
        while (!at_end(c) && *c->p >= '0' && *c->p <= '9') c->p++;
    }
    if (!at_end(c) && (*c->p == 'e' || *c->p == 'E')) {
        c->p++;
        if (!at_end(c) && (*c->p == '+' || *c->p == '-')) c->p++;
        while (!at_end(c) && *c->p >= '0' && *c->p <= '9') c->p++;
    }
    if (!any) return 0;
    *out = sign * v;
    return 1;
}

/* Skip a value we don't model (object/null/etc.) without storing it. */
static int skip_value(Cur *c);

static int skip_object_or_array(Cur *c, char open, char close)
{
    int depth = 0;
    if (at_end(c) || *c->p != open) return 0;
    while (!at_end(c)) {
        char ch = *c->p;
        if (ch == '"') {           /* skip strings so braces inside don't count */
            char tmp[JSON_MAX_STR];
            if (!parse_string(c, tmp, sizeof tmp)) return 0;
            continue;
        }
        if (ch == open)  depth++;
        if (ch == close) { depth--; c->p++; if (depth == 0) return 1; continue; }
        c->p++;
    }
    return 0;
}

static int skip_value(Cur *c)
{
    skip_ws(c);
    if (at_end(c)) return 0;
    switch (*c->p) {
        case '"': { char t[JSON_MAX_STR]; return parse_string(c, t, sizeof t); }
        case '{': return skip_object_or_array(c, '{', '}');
        case '[': return skip_object_or_array(c, '[', ']');
        case 't': c->p += (c->end - c->p >= 4) ? 4 : (c->end - c->p); return 1; /* true  */
        case 'f': c->p += (c->end - c->p >= 5) ? 5 : (c->end - c->p); return 1; /* false */
        case 'n': c->p += (c->end - c->p >= 4) ? 4 : (c->end - c->p); return 1; /* null  */
        default: { long n; return parse_number(c, &n); }
    }
}

/* ---- field parsing -------------------------------------------------------- */

static int parse_value(Cur *c, JsonField *f)
{
    skip_ws(c);
    if (at_end(c)) return 0;
    char ch = *c->p;
    if (ch == '"') {
        f->type = JT_STR;
        return parse_string(c, f->str, sizeof f->str);
    }
    if (ch == '[') {
        f->type = JT_ARR;
        f->narr = 0;
        c->p++; /* '[' */
        skip_ws(c);
        if (!at_end(c) && *c->p == ']') { c->p++; return 1; } /* empty array */
        for (;;) {
            skip_ws(c);
            if (at_end(c)) return 0;
            if (*c->p == '"') {
                char tmp[JSON_MAX_STR];
                if (!parse_string(c, tmp, sizeof tmp)) return 0;
                if (f->narr < JSON_MAX_ARR) {
                    strncpy(f->arr[f->narr], tmp, JSON_ARR_STR - 1);
                    f->arr[f->narr][JSON_ARR_STR - 1] = '\0';
                    f->narr++;
                }
            } else {
                /* non-string array element: skip it (we only model string arrays) */
                if (!skip_value(c)) return 0;
            }
            skip_ws(c);
            if (at_end(c)) return 0;
            if (*c->p == ',') { c->p++; continue; }
            if (*c->p == ']') { c->p++; return 1; }
            return 0;
        }
    }
    if (ch == 't' || ch == 'f') {
        f->type = JT_BOOL;
        f->boolean = (ch == 't');
        return skip_value(c);
    }
    if (ch == 'n') { /* null — store as NONE but consume */
        f->type = JT_NONE;
        return skip_value(c);
    }
    if (ch == '{') { /* nested object: not modelled, skip, mark NONE */
        f->type = JT_NONE;
        return skip_value(c);
    }
    f->type = JT_NUM;
    return parse_number(c, &f->num);
}

int json_parse_object(const char *s, long len, JsonObject *out)
{
    Cur cur; cur.p = s; cur.end = s + len;
    Cur *c = &cur;

    out->nfields = 0;

    skip_ws(c);
    if (at_end(c)) return 0;            /* blank line */
    if (*c->p != '{') return -1;
    c->p++;

    skip_ws(c);
    if (!at_end(c) && *c->p == '}') { c->p++; return 1; } /* empty object */

    for (;;) {
        JsonField tmp;
        memset(&tmp, 0, sizeof tmp);

        skip_ws(c);
        if (at_end(c) || *c->p != '"') return -1;
        if (!parse_string(c, tmp.key, sizeof tmp.key)) return -1;

        skip_ws(c);
        if (at_end(c) || *c->p != ':') return -1;
        c->p++;

        if (!parse_value(c, &tmp)) return -1;

        if (out->nfields < JSON_MAX_FIELDS && tmp.type != JT_NONE)
            out->fields[out->nfields++] = tmp;

        skip_ws(c);
        if (at_end(c)) return -1;
        if (*c->p == ',') { c->p++; continue; }
        if (*c->p == '}') { c->p++; return 1; }
        return -1;
    }
}

const JsonField *json_get(const JsonObject *o, const char *key)
{
    int i;
    for (i = 0; i < o->nfields; i++)
        if (strcmp(o->fields[i].key, key) == 0)
            return &o->fields[i];
    return 0;
}
