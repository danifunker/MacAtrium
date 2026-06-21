/*
 * json.h — tiny JSON parser for the MacAtrium catalog (schema v2, docs/06).
 *
 * Scope is deliberately small: one *flat* JSON object per call, with values that
 * are strings, integers, booleans, or arrays-of-strings (for `categories`).
 * Nested objects are not in the schema and are skipped. The parser is liberal —
 * unknown fields are kept, malformed input fails the single object (so the
 * caller can drop one bad catalog line and keep the rest), and it is pure C with
 * no Toolbox dependency so it can be unit-tested off-target with host gcc.
 */
#ifndef MACATRIUM_JSON_H
#define MACATRIUM_JSON_H

#define JSON_MAX_FIELDS 24
#define JSON_MAX_STR    256
#define JSON_MAX_ARR    16
#define JSON_ARR_STR    48
#define JSON_KEY_LEN    40

typedef enum { JT_NONE = 0, JT_STR, JT_NUM, JT_BOOL, JT_ARR } JsonType;

typedef struct {
    char     key[JSON_KEY_LEN];
    JsonType type;
    char     str[JSON_MAX_STR];                 /* JT_STR */
    long     num;                               /* JT_NUM (integer part)   */
    int      boolean;                           /* JT_BOOL                 */
    char     arr[JSON_MAX_ARR][JSON_ARR_STR];   /* JT_ARR (strings)        */
    int      narr;
} JsonField;

typedef struct {
    JsonField fields[JSON_MAX_FIELDS];
    int       nfields;
} JsonObject;

/* Parse a single flat JSON object from s[0..len).
 * Returns  1 = one object parsed,
 *          0 = no object found (blank / whitespace only),
 *         -1 = malformed. */
int json_parse_object(const char *s, long len, JsonObject *out);

/* Field lookup by key (NULL if absent). */
const JsonField *json_get(const JsonObject *o, const char *key);

#endif /* MACATRIUM_JSON_H */
