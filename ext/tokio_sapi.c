/*
 * tokio_sapi - PHP extension for tokio_php server
 *
 * This extension provides direct access to PHP internals,
 * bypassing the need for zend_eval_string() to set superglobals.
 *
 * ZTS Note: Uses C11 __thread for request context instead of PHP module globals
 * to ensure proper thread-local storage in worker threads.
 */

#include "tokio_sapi.h"
#include "bridge/bridge.h"  /* Shared bridge for Rust <-> PHP communication */
#include <stdlib.h>
#include <unistd.h>  /* STDOUT_FILENO, lseek */

/* ============================================================================
 * Thread-local request context (NOT using PHP module globals)
 *
 * PHP module globals require complex ZTS initialization that doesn't work
 * well when called from external (Rust) threads. Instead, we use simple
 * C11 thread-local storage which works correctly in any thread.
 * ============================================================================ */

static __thread tokio_request_context *tls_request_ctx = NULL;
static __thread uint64_t tls_request_id = 0;

/* Heartbeat context for request timeout extension */
static __thread void *tls_heartbeat_ctx = NULL;
static __thread uint64_t tls_heartbeat_max_secs = 0;
/* Forward declaration for heartbeat callback type */
typedef int64_t (*tokio_heartbeat_fn_t)(void *ctx, uint64_t secs);
static __thread tokio_heartbeat_fn_t tls_heartbeat_callback = NULL;

/* Finish request state now uses tokio_bridge shared library
 * to solve TLS isolation between static lib and dynamic extension.
 * See ext/bridge/bridge.h for details. */

/* Get or create thread-local request context */
static tokio_request_context* get_request_context(void)
{
    if (tls_request_ctx == NULL) {
        /* Use regular malloc - we manage lifecycle ourselves */
        tls_request_ctx = (tokio_request_context*)calloc(1, sizeof(tokio_request_context));
        if (tls_request_ctx) {
            tls_request_ctx->http_response_code = 200;
        }
    }
    return tls_request_ctx;
}

/* Free thread-local request context */
static void free_request_context(void)
{
    tokio_request_context *ctx = tls_request_ctx;
    if (ctx == NULL) return;

    /* Free POST data (allocated with malloc) */
    if (ctx->post_data) {
        free(ctx->post_data);
    }

    /* Free headers (allocated with malloc) */
    for (int i = 0; i < ctx->header_count; i++) {
        if (ctx->headers[i].name) free(ctx->headers[i].name);
        if (ctx->headers[i].value) free(ctx->headers[i].value);
    }

    free(ctx);
    tls_request_ctx = NULL;
}

/* ============================================================================
 * Module globals (only for callbacks, not request state)
 * ============================================================================ */

ZEND_DECLARE_MODULE_GLOBALS(tokio_sapi)

static void php_tokio_sapi_init_globals(zend_tokio_sapi_globals *globals)
{
    memset(globals, 0, sizeof(zend_tokio_sapi_globals));
}

/* ============================================================================
 * Superglobals manipulation (the main performance win!)
 * ============================================================================ */

/* Superglobal names for symbol table lookup */
static const char *superglobal_names[] = {
    "_POST",    /* TRACK_VARS_POST = 0 */
    "_GET",     /* TRACK_VARS_GET = 1 */
    "_COOKIE",  /* TRACK_VARS_COOKIE = 2 */
    "_SERVER",  /* TRACK_VARS_SERVER = 3 */
    "_ENV",     /* TRACK_VARS_ENV = 4 */
    "_FILES",   /* TRACK_VARS_FILES = 5 */
    "_REQUEST"  /* TRACK_VARS_REQUEST = 6 (not really used here) */
};
static const size_t superglobal_name_lens[] = {5, 4, 7, 7, 4, 6, 8};

/* Cached interned strings for superglobal names (avoids alloc/free per call)
 * Using __thread for ZTS thread-local storage */
static __thread zend_string *superglobal_zstrings[7] = {NULL, NULL, NULL, NULL, NULL, NULL, NULL};
static __thread zend_string *request_zstring = NULL;
static __thread int superglobal_strings_initialized = 0;

/* Initialize cached strings (called once per thread) */
static void init_superglobal_strings(void)
{
    if (superglobal_strings_initialized) return; /* Already initialized */

    for (int i = 0; i < 6; i++) {
        /* Use persistent=1 to avoid per-request memory management */
        superglobal_zstrings[i] = zend_string_init(superglobal_names[i], superglobal_name_lens[i], 1);
    }
    request_zstring = zend_string_init("_REQUEST", sizeof("_REQUEST")-1, 1);
    superglobal_strings_initialized = 1;
}

/* Helper: get superglobal from symbol table (where PHP code reads from)
 * Fast path: try direct access first, only call zend_is_auto_global if needed */
static zval* get_superglobal_from_symtable(int track_var)
{
    if (track_var < 0 || track_var > 5) return NULL;

    const char *name = superglobal_names[track_var];
    size_t name_len = superglobal_name_lens[track_var];

    /* Fast path: try direct symbol table access first */
    zval *arr = zend_hash_str_find(&EG(symbol_table), name, name_len);
    if (arr && Z_TYPE_P(arr) == IS_ARRAY) {
        return arr;  /* Already exists and is array - return directly */
    }

    /* Slow path: initialize cached strings and trigger auto-global init */
    init_superglobal_strings();
    zend_is_auto_global(superglobal_zstrings[track_var]);

    /* Try again after auto-global init */
    arr = zend_hash_str_find(&EG(symbol_table), name, name_len);
    if (arr == NULL || Z_TYPE_P(arr) != IS_ARRAY) {
        /* Create new array in symbol table */
        zval new_arr;
        array_init(&new_arr);
        arr = zend_hash_str_update(&EG(symbol_table), name, name_len, &new_arr);
    }
    return arr;
}

/* Helper: set a value in a superglobal using PHP's proper API */
static void set_superglobal_value(int track_var, const char *key, size_t key_len,
                                   const char *value, size_t value_len)
{
    /* Use PG(http_globals) which is what PHP's SAPI layer uses */
    zval *arr = &PG(http_globals)[track_var];

    /* Initialize if needed */
    if (Z_TYPE_P(arr) != IS_ARRAY) {
        array_init(arr);
    }

    /* Use php_register_variable_safe - the proper PHP API for SAPIs */
    php_register_variable_safe((char*)key, (char*)value, value_len, arr);
}

/* Clear a superglobal array (fast path - no auto-global init) */
static void clear_superglobal(int track_var)
{
    if (track_var < 0 || track_var > 5) return;

    const char *name = superglobal_names[track_var];
    size_t name_len = superglobal_name_lens[track_var];

    /* Direct symbol table access - if not there, nothing to clear */
    zval *arr = zend_hash_str_find(&EG(symbol_table), name, name_len);
    if (arr && Z_TYPE_P(arr) == IS_ARRAY) {
        zend_hash_clean(Z_ARRVAL_P(arr));
    }
}

/* Public API: set $_SERVER variable */
void tokio_sapi_set_server_var(const char *key, size_t key_len,
                                const char *value, size_t value_len)
{
    set_superglobal_value(TRACK_VARS_SERVER, key, key_len, value, value_len);
}

/* Public API: set $_GET variable */
void tokio_sapi_set_get_var(const char *key, size_t key_len,
                             const char *value, size_t value_len)
{
    set_superglobal_value(TRACK_VARS_GET, key, key_len, value, value_len);
}

/* Public API: set $_POST variable */
void tokio_sapi_set_post_var(const char *key, size_t key_len,
                              const char *value, size_t value_len)
{
    set_superglobal_value(TRACK_VARS_POST, key, key_len, value, value_len);
}

/* Public API: set $_COOKIE variable */
void tokio_sapi_set_cookie_var(const char *key, size_t key_len,
                                const char *value, size_t value_len)
{
    set_superglobal_value(TRACK_VARS_COOKIE, key, key_len, value, value_len);
}

/* ============================================================================
 * Cached superglobal arrays (avoids repeated zend_is_auto_global calls)
 * ============================================================================ */

/* Thread-local cached superglobal array pointers */
static __thread zval *cached_superglobal_arrs[6] = {NULL};
static __thread int superglobals_initialized = 0;

/* Reset cached pointers (call at request end) */
static void reset_superglobal_cache(void)
{
    superglobals_initialized = 0;
    for (int i = 0; i < 6; i++) {
        cached_superglobal_arrs[i] = NULL;
    }
}

/* Initialize all superglobals once per request (avoids repeated zend_is_auto_global calls) */
void tokio_sapi_init_superglobals(void)
{
    if (superglobals_initialized) return;

    for (int i = 0; i < 6; i++) {
        cached_superglobal_arrs[i] = get_superglobal_from_symtable(i);
    }
    superglobals_initialized = 1;
}

/* Get cached superglobal array (fast path after init) */
static zval* get_cached_superglobal(int track_var)
{
    if (track_var < 0 || track_var > 5) return NULL;
    if (!superglobals_initialized) {
        tokio_sapi_init_superglobals();
    }
    return cached_superglobal_arrs[track_var];
}

/* ============================================================================
 * Batch API - set multiple variables in one FFI call
 * ============================================================================ */

/* Set a nested array value using bracket notation (e.g., form[field][subfield])
 * Handles PHP-style array key parsing:
 * - form[field] -> $arr['form']['field'] = value
 * - form[] -> $arr['form'][] = value (auto-indexed)
 * - form[0][name] -> $arr['form'][0]['name'] = value
 */
static void set_nested_array_value(zval *arr, const char *key, size_t key_len, const char *val, size_t val_len)
{
    /* Validate input */
    if (arr == NULL || Z_TYPE_P(arr) != IS_ARRAY || key == NULL || key_len == 0) {
        return;
    }

    /* Find first bracket */
    const char *bracket = memchr(key, '[', key_len);

    if (bracket == NULL) {
        /* Simple key, no brackets - direct set */
        zval zval_val;
        ZVAL_STRINGL(&zval_val, val, val_len);
        zend_hash_str_update(Z_ARRVAL_P(arr), key, key_len, &zval_val);
        return;
    }

    /* Extract base name (before first bracket) */
    size_t base_len = bracket - key;
    if (base_len == 0) {
        /* Key starts with [ - invalid, treat as literal */
        zval zval_val;
        ZVAL_STRINGL(&zval_val, val, val_len);
        zend_hash_str_update(Z_ARRVAL_P(arr), key, key_len, &zval_val);
        return;
    }

    /* Get or create base array */
    zval *current = zend_hash_str_find(Z_ARRVAL_P(arr), key, base_len);
    if (current == NULL || Z_TYPE_P(current) != IS_ARRAY) {
        /* Create new array for base key */
        zval new_arr;
        array_init(&new_arr);
        current = zend_hash_str_update(Z_ARRVAL_P(arr), key, base_len, &new_arr);
        if (current == NULL) {
            /* Hash update failed - bail out to prevent SIGSEGV */
            return;
        }
    }

    /* Parse remaining brackets */
    const char *ptr = bracket;
    const char *end = key + key_len;

    while (ptr < end && *ptr == '[' && current != NULL) {
        ptr++; /* Skip '[' */

        /* Find closing bracket */
        const char *close = memchr(ptr, ']', end - ptr);
        if (close == NULL) {
            /* Malformed - no closing bracket, bail out */
            break;
        }

        size_t index_len = close - ptr;

        /* Check if there are more brackets after this one */
        const char *next = close + 1;
        int has_more = (next < end && *next == '[');

        if (index_len == 0) {
            /* Empty brackets [] - auto-indexed append */
            if (has_more) {
                /* More brackets after - create array element */
                zval new_arr;
                array_init(&new_arr);
                current = zend_hash_next_index_insert(Z_ARRVAL_P(current), &new_arr);
                if (current == NULL) break;
            } else {
                /* Final [] - append value */
                zval zval_val;
                ZVAL_STRINGL(&zval_val, val, val_len);
                zend_hash_next_index_insert(Z_ARRVAL_P(current), &zval_val);
                return;
            }
        } else {
            /* Named or numeric index */
            /* Check if it's a numeric index */
            int is_numeric = 1;
            for (size_t i = 0; i < index_len; i++) {
                if (ptr[i] < '0' || ptr[i] > '9') {
                    is_numeric = 0;
                    break;
                }
            }

            if (has_more) {
                /* More brackets - get or create sub-array */
                zval *next_arr;
                if (is_numeric) {
                    zend_long idx = ZEND_STRTOL(ptr, NULL, 10);
                    next_arr = zend_hash_index_find(Z_ARRVAL_P(current), idx);
                    if (next_arr == NULL || Z_TYPE_P(next_arr) != IS_ARRAY) {
                        zval new_arr;
                        array_init(&new_arr);
                        next_arr = zend_hash_index_update(Z_ARRVAL_P(current), idx, &new_arr);
                        if (next_arr == NULL) break;  /* Hash update failed */
                    }
                } else {
                    next_arr = zend_hash_str_find(Z_ARRVAL_P(current), ptr, index_len);
                    if (next_arr == NULL || Z_TYPE_P(next_arr) != IS_ARRAY) {
                        zval new_arr;
                        array_init(&new_arr);
                        next_arr = zend_hash_str_update(Z_ARRVAL_P(current), ptr, index_len, &new_arr);
                        if (next_arr == NULL) break;  /* Hash update failed */
                    }
                }
                current = next_arr;
            } else {
                /* Final index - set value */
                zval zval_val;
                ZVAL_STRINGL(&zval_val, val, val_len);
                if (is_numeric) {
                    zend_long idx = ZEND_STRTOL(ptr, NULL, 10);
                    zend_hash_index_update(Z_ARRVAL_P(current), idx, &zval_val);
                } else {
                    zend_hash_str_update(Z_ARRVAL_P(current), ptr, index_len, &zval_val);
                }
                return;
            }
        }

        ptr = next;
    }

    /* If we get here with remaining value (malformed brackets or NULL current), set as last element */
    if (current != NULL && Z_TYPE_P(current) == IS_ARRAY) {
        zval zval_val;
        ZVAL_STRINGL(&zval_val, val, val_len);
        zend_hash_next_index_insert(Z_ARRVAL_P(current), &zval_val);
    }
}

/* Batch set superglobal from packed buffer:
 * Buffer format: [key_len:u32][key\0][val_len:u32][val]...
 * key_len includes null terminator, val_len does not
 * parse_brackets: if true, parse PHP-style bracket notation (e.g., form[field])
 * Returns number of variables set */
static int set_superglobal_batch(int track_var, const char *buffer, size_t buffer_len, size_t count, int parse_brackets)
{
    /* Get cached array (fast path, no repeated zend_is_auto_global) */
    zval *arr = get_cached_superglobal(track_var);
    if (arr == NULL || Z_TYPE_P(arr) != IS_ARRAY) {
        return 0;
    }

    const unsigned char *ptr = (const unsigned char *)buffer;
    const unsigned char *end = ptr + buffer_len;
    int set_count = 0;

    for (size_t i = 0; i < count && ptr + 4 <= end; i++) {
        /* Read key length (includes null terminator) */
        uint32_t key_len;
        memcpy(&key_len, ptr, 4);
        ptr += 4;
        if (key_len == 0 || ptr + key_len > end) break;

        const char *key = (const char *)ptr;  /* Already null-terminated */
        size_t key_str_len = key_len - 1;  /* Exclude null for hash key */
        ptr += key_len;

        /* Read value length */
        if (ptr + 4 > end) break;
        uint32_t val_len;
        memcpy(&val_len, ptr, 4);
        ptr += 4;
        if (ptr + val_len > end) break;

        const char *val = (const char *)ptr;
        ptr += val_len;

        if (parse_brackets) {
            /* Parse bracket notation (e.g., form[field] -> nested array) */
            set_nested_array_value(arr, key, key_str_len, val, val_len);
        } else {
            /* Direct hash update (faster for simple keys like $_SERVER) */
            zval zval_val;
            ZVAL_STRINGL(&zval_val, val, val_len);
            zend_hash_str_update(Z_ARRVAL_P(arr), key, key_str_len, &zval_val);
        }
        set_count++;
    }

    return set_count;
}

/* Public API: batch set $_SERVER variables (no bracket parsing) */
int tokio_sapi_set_server_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_SERVER, buffer, buffer_len, count, 0);
}

/* Public API: batch set $_GET variables (with bracket parsing) */
int tokio_sapi_set_get_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_GET, buffer, buffer_len, count, 1);
}

/* Public API: batch set $_POST variables (with bracket parsing) */
int tokio_sapi_set_post_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_POST, buffer, buffer_len, count, 1);
}

/* Public API: batch set $_COOKIE variables (with bracket parsing) */
int tokio_sapi_set_cookie_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_COOKIE, buffer, buffer_len, count, 1);
}

/* Public API: ultra-batch - set ALL superglobals in one call
 * This combines: clear, init caches, set all vars, build $_REQUEST, init request state */
void tokio_sapi_set_all_superglobals(
    const char *server_buf, size_t server_len, size_t server_count,
    const char *get_buf, size_t get_len, size_t get_count,
    const char *post_buf, size_t post_len, size_t post_count,
    const char *cookie_buf, size_t cookie_len, size_t cookie_count)
{
    /* 1. Clear all superglobals */
    clear_superglobal(TRACK_VARS_GET);
    clear_superglobal(TRACK_VARS_POST);
    clear_superglobal(TRACK_VARS_SERVER);
    clear_superglobal(TRACK_VARS_COOKIE);
    clear_superglobal(TRACK_VARS_FILES);

    /* Clear $_REQUEST */
    zval *request = zend_hash_str_find(&EG(symbol_table), "_REQUEST", sizeof("_REQUEST")-1);
    if (request && Z_TYPE_P(request) == IS_ARRAY) {
        zend_hash_clean(Z_ARRVAL_P(request));
    }

    /* Reset and reinitialize cache */
    reset_superglobal_cache();
    tokio_sapi_init_superglobals();

    /* 2. Set all superglobals using cached arrays */
    if (server_count > 0) {
        set_superglobal_batch(TRACK_VARS_SERVER, server_buf, server_len, server_count, 0);
    }
    if (get_count > 0) {
        set_superglobal_batch(TRACK_VARS_GET, get_buf, get_len, get_count, 1);
    }
    if (post_count > 0) {
        set_superglobal_batch(TRACK_VARS_POST, post_buf, post_len, post_count, 1);
    }
    if (cookie_count > 0) {
        set_superglobal_batch(TRACK_VARS_COOKIE, cookie_buf, cookie_len, cookie_count, 1);
    }

    /* 3. Build $_REQUEST from $_GET + $_POST */
    tokio_sapi_build_request();

    /* 4. Initialize request state (headers, output buffering) */
    tokio_sapi_init_request_state();
}

/* Public API: set $_FILES variable (single file) */
void tokio_sapi_set_files_var(const char *field, size_t field_len,
                               const char *name, const char *type,
                               const char *tmp_name, int error, size_t size)
{
    zval *files_arr = get_superglobal_from_symtable(TRACK_VARS_FILES);
    if (files_arr == NULL) return;

    zval file_entry;
    zval tmp;

    array_init(&file_entry);

    ZVAL_STRING(&tmp, name);
    zend_hash_str_update(Z_ARRVAL(file_entry), "name", sizeof("name")-1, &tmp);

    ZVAL_STRING(&tmp, type);
    zend_hash_str_update(Z_ARRVAL(file_entry), "type", sizeof("type")-1, &tmp);

    ZVAL_STRING(&tmp, tmp_name);
    zend_hash_str_update(Z_ARRVAL(file_entry), "tmp_name", sizeof("tmp_name")-1, &tmp);

    ZVAL_LONG(&tmp, error);
    zend_hash_str_update(Z_ARRVAL(file_entry), "error", sizeof("error")-1, &tmp);

    ZVAL_LONG(&tmp, size);
    zend_hash_str_update(Z_ARRVAL(file_entry), "size", sizeof("size")-1, &tmp);

    zend_hash_str_update(Z_ARRVAL_P(files_arr), field, field_len, &file_entry);
}

/* Public API: clear all superglobals */
void tokio_sapi_clear_superglobals(void)
{
    clear_superglobal(TRACK_VARS_GET);
    clear_superglobal(TRACK_VARS_POST);
    clear_superglobal(TRACK_VARS_SERVER);
    clear_superglobal(TRACK_VARS_COOKIE);
    clear_superglobal(TRACK_VARS_FILES);

    /* Also clear $_REQUEST */
    zval *request = zend_hash_str_find(&EG(symbol_table), "_REQUEST", sizeof("_REQUEST")-1);
    if (request && Z_TYPE_P(request) == IS_ARRAY) {
        zend_hash_clean(Z_ARRVAL_P(request));
    }

    /* Reset cache for next request */
    reset_superglobal_cache();
}

/* Public API: initialize request state (replaces header_remove();http_response_code(200);ob_start()) */
void tokio_sapi_init_request_state(void)
{
    /* Clear any existing headers from previous request */
    zend_llist_clean(&SG(sapi_headers).headers);

    /* Set default response code */
    SG(sapi_headers).http_response_code = 200;

    /* Start output buffering if not already started */
    if (!OG(active)) {
        php_output_start_default();
    }
}

/* Public API: build $_REQUEST from $_GET + $_POST */
void tokio_sapi_build_request(void)
{
    zval *get_arr = get_superglobal_from_symtable(TRACK_VARS_GET);
    zval *post_arr = get_superglobal_from_symtable(TRACK_VARS_POST);
    zval request_arr;
    zend_string *key;
    zval *val;

    if (get_arr == NULL || post_arr == NULL) return;

    array_init(&request_arr);

    /* Copy $_GET */
    ZEND_HASH_FOREACH_STR_KEY_VAL(Z_ARRVAL_P(get_arr), key, val) {
        if (key) {
            Z_TRY_ADDREF_P(val);
            zend_hash_update(Z_ARRVAL(request_arr), key, val);
        }
    } ZEND_HASH_FOREACH_END();

    /* Merge $_POST (overwrites GET) */
    ZEND_HASH_FOREACH_STR_KEY_VAL(Z_ARRVAL_P(post_arr), key, val) {
        if (key) {
            Z_TRY_ADDREF_P(val);
            zend_hash_update(Z_ARRVAL(request_arr), key, val);
        }
    } ZEND_HASH_FOREACH_END();

    /* Update $_REQUEST in symbol table */
    zend_hash_str_update(&EG(symbol_table), "_REQUEST", sizeof("_REQUEST")-1, &request_arr);
}

/* ============================================================================
 * POST body handling for php://input
 * ============================================================================ */

void tokio_sapi_set_post_data(const char *data, size_t len)
{
    tokio_request_context *ctx = get_request_context();
    if (ctx == NULL) return;

    /* Free previous POST data */
    if (ctx->post_data) {
        free(ctx->post_data);
        ctx->post_data = NULL;
        ctx->post_data_len = 0;
    }

    /* Close previous request body stream if any */
    if (SG(request_info).request_body) {
        php_stream_close(SG(request_info).request_body);
        SG(request_info).request_body = NULL;
    }

    if (data && len > 0) {
        /* Store copy for our context */
        ctx->post_data = (char*)malloc(len + 1);
        if (ctx->post_data) {
            memcpy(ctx->post_data, data, len);
            ctx->post_data[len] = '\0';
            ctx->post_data_len = len;
        }

        /* Create temp stream for php://input
         * TEMP_STREAM_DEFAULT allows both read and write */
        php_stream *stream = php_stream_temp_create(TEMP_STREAM_DEFAULT, len);
        if (stream) {
            php_stream_write(stream, data, len);
            php_stream_rewind(stream);
            SG(request_info).request_body = stream;
        }

        /* Set content length */
        SG(request_info).content_length = len;
    } else {
        ctx->post_data = NULL;
        ctx->post_data_len = 0;
        SG(request_info).content_length = 0;
    }
    ctx->post_data_read = 0;
}

/* ============================================================================
 * Header capture (using thread-local context)
 * ============================================================================ */

/* Public API: get header count */
int tokio_sapi_get_header_count(void)
{
    tokio_request_context *ctx = tls_request_ctx;
    return ctx ? ctx->header_count : 0;
}

/* Public API: get header name by index */
const char* tokio_sapi_get_header_name(int index)
{
    tokio_request_context *ctx = tls_request_ctx;
    if (ctx && index >= 0 && index < ctx->header_count) {
        return ctx->headers[index].name;
    }
    return NULL;
}

/* Public API: get header value by index */
const char* tokio_sapi_get_header_value(int index)
{
    tokio_request_context *ctx = tls_request_ctx;
    if (ctx && index >= 0 && index < ctx->header_count) {
        return ctx->headers[index].value;
    }
    return NULL;
}

/* Public API: get response code */
int tokio_sapi_get_response_code(void)
{
    tokio_request_context *ctx = tls_request_ctx;
    return ctx ? ctx->http_response_code : 200;
}

/* Public API: add a header (called from Rust or internal) */
void tokio_sapi_add_header(const char *name, size_t name_len,
                           const char *value, size_t value_len, int replace)
{
    tokio_request_context *ctx = get_request_context();
    if (ctx == NULL || ctx->header_count >= TOKIO_MAX_HEADERS) return;

    /* For replace mode, check if header exists */
    if (replace) {
        for (int i = 0; i < ctx->header_count; i++) {
            if (ctx->headers[i].name &&
                strncasecmp(ctx->headers[i].name, name, name_len) == 0 &&
                strlen(ctx->headers[i].name) == name_len) {
                /* Replace existing */
                free(ctx->headers[i].value);
                ctx->headers[i].value = (char*)malloc(value_len + 1);
                if (ctx->headers[i].value) {
                    memcpy(ctx->headers[i].value, value, value_len);
                    ctx->headers[i].value[value_len] = '\0';
                }
                return;
            }
        }
    }

    /* Add new header */
    int idx = ctx->header_count;
    ctx->headers[idx].name = (char*)malloc(name_len + 1);
    ctx->headers[idx].value = (char*)malloc(value_len + 1);

    if (ctx->headers[idx].name && ctx->headers[idx].value) {
        memcpy(ctx->headers[idx].name, name, name_len);
        ctx->headers[idx].name[name_len] = '\0';
        memcpy(ctx->headers[idx].value, value, value_len);
        ctx->headers[idx].value[value_len] = '\0';
        ctx->header_count++;
    }
}

/* Public API: set response code */
void tokio_sapi_set_response_code(int code)
{
    tokio_request_context *ctx = get_request_context();
    if (ctx) {
        ctx->http_response_code = code;
    }
}

/* ============================================================================
 * Script execution
 * ============================================================================ */

int tokio_sapi_execute_script(const char *path)
{
    zend_file_handle file_handle;
    int ret = FAILURE;

    zend_stream_init_filename(&file_handle, path);

    if (php_execute_script(&file_handle)) {
        ret = SUCCESS;
    }

    zend_destroy_file_handle(&file_handle);
    return ret;
}

/* ============================================================================
 * Request lifecycle (using thread-local storage)
 * ============================================================================ */

int tokio_sapi_request_init(uint64_t request_id)
{
    /* Free any existing context from previous request */
    free_request_context();

    /* Create fresh context */
    tokio_request_context *ctx = get_request_context();
    if (ctx == NULL) {
        return FAILURE;
    }

    ctx->request_id = request_id;
    ctx->http_response_code = 200;
    ctx->header_count = 0;
    ctx->post_data = NULL;
    ctx->post_data_len = 0;
    ctx->post_data_read = 0;

    /* Store in thread-local for PHP functions */
    tls_request_id = request_id;

    return SUCCESS;
}

void tokio_sapi_request_shutdown(void)
{
    /* Close request body stream if any */
    if (SG(request_info).request_body) {
        php_stream_close(SG(request_info).request_body);
        SG(request_info).request_body = NULL;
    }
    SG(request_info).content_length = 0;

    free_request_context();
    tls_request_id = 0;
    tls_heartbeat_ctx = NULL;
    tls_heartbeat_max_secs = 0;
    tls_heartbeat_callback = NULL;

    /* CRITICAL: Reset superglobal cache to prevent use-after-free
     * After php_request_shutdown(), PHP's symbol table is destroyed
     * and cached pointers become dangling. */
    reset_superglobal_cache();

    /* Bridge context is destroyed by Rust after reading finish state */
}

/* ============================================================================
 * Heartbeat API for request timeout extension
 * ============================================================================ */

/* Set heartbeat context and callback (called from Rust before PHP execution)
 * NOTE: This function is no longer used. Heartbeat info is now passed via $_SERVER
 * because the static library and dynamic extension have separate TLS storage. */
void tokio_sapi_set_heartbeat_ctx(void *ctx, uint64_t max_secs, tokio_heartbeat_fn_t callback)
{
    tls_heartbeat_ctx = ctx;
    tls_heartbeat_max_secs = max_secs;
    tls_heartbeat_callback = callback;
}

/* Get heartbeat context (for internal use) */
void* tokio_sapi_get_heartbeat_ctx(void)
{
    return tls_heartbeat_ctx;
}

/* Get max heartbeat extension (for internal use) */
uint64_t tokio_sapi_get_heartbeat_max_secs(void)
{
    return tls_heartbeat_max_secs;
}

/* ============================================================================
 * PHP Functions (available from PHP scripts)
 * ============================================================================ */

/* tokio_request_id(): int - get current request ID
 * Reads from $_SERVER['TOKIO_REQUEST_ID'] which is set by Rust in server_vars.
 * This allows sharing between Rust and PHP.
 */
PHP_FUNCTION(tokio_request_id)
{
    zval *server_arr, *request_id_val;

    ZEND_PARSE_PARAMETERS_NONE();

    /* Get $_SERVER from the symbol table (not PG(http_globals)) */
    server_arr = zend_hash_str_find(&EG(symbol_table), "_SERVER", sizeof("_SERVER")-1);
    if (server_arr && Z_TYPE_P(server_arr) == IS_ARRAY) {
        /* Get TOKIO_REQUEST_ID from $_SERVER */
        request_id_val = zend_hash_str_find(Z_ARRVAL_P(server_arr), "TOKIO_REQUEST_ID", sizeof("TOKIO_REQUEST_ID")-1);
        if (request_id_val && Z_TYPE_P(request_id_val) == IS_STRING) {
            RETURN_LONG(atoll(Z_STRVAL_P(request_id_val)));
        }
    }

    /* Fallback to TLS (same compilation unit) */
    RETURN_LONG((zend_long)tls_request_id);
}

/* tokio_worker_id(): int - get worker thread ID
 * Reads from $_SERVER['TOKIO_WORKER_ID'] which is set by Rust in server_vars.
 */
PHP_FUNCTION(tokio_worker_id)
{
    zval *server_arr, *worker_id_val;

    ZEND_PARSE_PARAMETERS_NONE();

    /* Get $_SERVER from the symbol table */
    server_arr = zend_hash_str_find(&EG(symbol_table), "_SERVER", sizeof("_SERVER")-1);
    if (server_arr && Z_TYPE_P(server_arr) == IS_ARRAY) {
        /* Get TOKIO_WORKER_ID from $_SERVER */
        worker_id_val = zend_hash_str_find(Z_ARRVAL_P(server_arr), "TOKIO_WORKER_ID", sizeof("TOKIO_WORKER_ID")-1);
        if (worker_id_val && Z_TYPE_P(worker_id_val) == IS_STRING) {
            RETURN_LONG(atoll(Z_STRVAL_P(worker_id_val)));
        }
    }

    /* Fallback to 0 if not set */
    RETURN_LONG(0);
}

/* tokio_server_info(): array - get server information */
PHP_FUNCTION(tokio_server_info)
{
    zval *server_arr, *build_val;

    ZEND_PARSE_PARAMETERS_NONE();

    array_init(return_value);
    add_assoc_string(return_value, "server", "tokio_php");
    add_assoc_string(return_value, "version", TOKIO_SAPI_VERSION);
    add_assoc_string(return_value, "sapi", "tokio_sapi");
    add_assoc_bool(return_value, "zts", 1);

    /* Get build version from $_SERVER['TOKIO_SERVER_BUILD_VERSION'] */
    server_arr = zend_hash_str_find(&EG(symbol_table), "_SERVER", sizeof("_SERVER")-1);
    if (server_arr && Z_TYPE_P(server_arr) == IS_ARRAY) {
        build_val = zend_hash_str_find(Z_ARRVAL_P(server_arr), "TOKIO_SERVER_BUILD_VERSION", sizeof("TOKIO_SERVER_BUILD_VERSION")-1);
        if (build_val && Z_TYPE_P(build_val) == IS_STRING) {
            add_assoc_str(return_value, "build", zend_string_copy(Z_STR_P(build_val)));
        }
    }
}

/* tokio_async_call(string $name, string $data): string|false - call Rust async */
PHP_FUNCTION(tokio_async_call)
{
    char *name, *data;
    size_t name_len, data_len;

    ZEND_PARSE_PARAMETERS_START(2, 2)
        Z_PARAM_STRING(name, name_len)
        Z_PARAM_STRING(data, data_len)
    ZEND_PARSE_PARAMETERS_END();

    if (TOKIO_G(async_call_callback)) {
        char *result = NULL;
        size_t result_len = 0;

        int ret = TOKIO_G(async_call_callback)(name, data, data_len, &result, &result_len);

        if (ret == 0 && result) {
            RETVAL_STRINGL(result, result_len);
            free(result);
            return;
        }
    }

    RETURN_FALSE;
}

/* tokio_request_heartbeat(int $time = 10): bool - extend request timeout
 *
 * Extends the request timeout deadline by $time seconds.
 * Returns false if:
 * - No timeout is configured (REQUEST_TIMEOUT=off)
 * - $time <= 0
 * - $time > REQUEST_TIMEOUT limit (e.g., if REQUEST_TIMEOUT=5m, max is 300)
 *
 * Can be called multiple times to keep extending the deadline.
 * Other PHP limits (set_time_limit, max_execution_time) still apply.
 *
 * Uses tokio_bridge shared library for direct Rust <-> PHP communication.
 */
PHP_FUNCTION(tokio_request_heartbeat)
{
    zend_long time = 10;

    ZEND_PARSE_PARAMETERS_START(0, 1)
        Z_PARAM_OPTIONAL
        Z_PARAM_LONG(time)
    ZEND_PARSE_PARAMETERS_END();

    /* Validate time parameter */
    if (time <= 0) {
        RETURN_FALSE;
    }

    /* Use bridge for direct communication with Rust */
    int result = tokio_bridge_send_heartbeat((uint64_t)time);
    RETURN_BOOL(result != 0);
}

/* ============================================================================
 * Helper functions for streaming early response
 * ============================================================================ */

/**
 * Read current output from memfd (stdout).
 * Returns allocated buffer that caller must free().
 *
 * @param len  Output: length of returned buffer
 * @return     Allocated buffer with output, or NULL if empty/error
 */
static char* get_current_output(size_t *len)
{
    *len = 0;

    /* 1. Flush libc buffers to ensure all data is in the fd */
    fflush(stdout);

    /* 2. Get current position (= amount written so far) */
    off_t pos = lseek(STDOUT_FILENO, 0, SEEK_CUR);
    if (pos <= 0) {
        return NULL;
    }

    /* 3. Allocate buffer */
    char *buf = (char*)malloc((size_t)pos);
    if (buf == NULL) {
        return NULL;
    }

    /* 4. Seek to beginning and read all data */
    lseek(STDOUT_FILENO, 0, SEEK_SET);
    ssize_t n = read(STDOUT_FILENO, buf, (size_t)pos);

    /* 5. Seek back to end for further writes */
    lseek(STDOUT_FILENO, 0, SEEK_END);

    if (n <= 0) {
        free(buf);
        return NULL;
    }

    *len = (size_t)n;
    return buf;
}

/**
 * Serialize headers into a buffer for callback.
 * Format: name\0value\0name\0value\0...
 *
 * Headers are captured via Rust's SAPI header_handler callback into
 * the bridge TLS (tokio_bridge_add_header). This function reads from
 * the bridge to serialize headers for the finish_request callback.
 *
 * @param len    Output: total length of buffer
 * @param count  Output: number of header pairs
 * @return       Allocated buffer, or NULL if no headers
 */
static char* serialize_sapi_headers(size_t *len, int *count)
{
    *len = 0;
    *count = 0;

    int num_headers = tokio_bridge_get_header_count();

    if (num_headers == 0) {
        return NULL;
    }

    /* First pass: calculate buffer size */
    size_t total_len = 0;
    for (int i = 0; i < num_headers; i++) {
        const char *name = NULL;
        const char *value = NULL;
        if (tokio_bridge_get_header(i, &name, &value) && name && value) {
            /* name + \0 + value + \0 */
            total_len += strlen(name) + 1 + strlen(value) + 1;
        }
    }

    if (total_len == 0) {
        return NULL;
    }

    /* Allocate buffer */
    char *buf = (char*)malloc(total_len);
    if (buf == NULL) {
        return NULL;
    }

    /* Second pass: serialize headers */
    char *ptr = buf;
    int header_count = 0;

    for (int i = 0; i < num_headers; i++) {
        const char *name = NULL;
        const char *value = NULL;
        if (!tokio_bridge_get_header(i, &name, &value) || !name || !value) {
            continue;
        }

        /* Copy name */
        size_t name_len = strlen(name);
        memcpy(ptr, name, name_len);
        ptr += name_len;
        *ptr++ = '\0';

        /* Copy value */
        size_t value_len = strlen(value);
        memcpy(ptr, value, value_len);
        ptr += value_len;
        *ptr++ = '\0';

        header_count++;
    }

    *len = ptr - buf;
    *count = header_count;
    return buf;
}

/* ============================================================================
 * Streaming API (SSE support)
 * ============================================================================ */

/**
 * Read new output from memfd since last stream offset.
 * Returns allocated buffer that caller must free().
 *
 * @param offset  Input: last read position; Output: new position
 * @param len     Output: length of returned buffer
 * @return        Allocated buffer with new output, or NULL if none/error
 */
static char* get_output_since_offset(size_t *offset, size_t *len)
{
    *len = 0;

    /* 1. Flush libc buffers */
    fflush(stdout);

    /* 2. Get current position (= amount written so far) */
    off_t end_pos = lseek(STDOUT_FILENO, 0, SEEK_CUR);
    if (end_pos < 0 || (size_t)end_pos <= *offset) {
        return NULL;  /* No new data */
    }

    size_t new_data_len = (size_t)end_pos - *offset;

    /* 3. Allocate buffer */
    char *buf = (char*)malloc(new_data_len);
    if (buf == NULL) {
        return NULL;
    }

    /* 4. Seek to offset and read new data */
    lseek(STDOUT_FILENO, (off_t)*offset, SEEK_SET);
    ssize_t n = read(STDOUT_FILENO, buf, new_data_len);

    /* 5. Seek back to end for further writes */
    lseek(STDOUT_FILENO, 0, SEEK_END);

    if (n <= 0) {
        free(buf);
        return NULL;
    }

    *offset = (size_t)end_pos;  /* Update offset for next call */
    *len = (size_t)n;
    return buf;
}

/**
 * SAPI flush handler - called by PHP's flush() function.
 *
 * When streaming mode is enabled, this sends new output to the client
 * via tokio_bridge_send_chunk(). This allows standard flush() to work
 * for SSE streaming without requiring a custom function.
 *
 * @param server_context  SAPI server context (unused)
 */
/* Thread-local flag to prevent recursive flush handling */
static __thread int flush_in_progress = 0;

void tokio_sapi_flush(void *server_context)
{
    (void)server_context;  /* Unused */

    /* Prevent recursive calls (php_output_flush can trigger this handler again) */
    if (flush_in_progress) {
        return;
    }

    /* Only process if streaming mode is enabled */
    if (!tokio_bridge_is_streaming()) {
        /* Not streaming - just flush normally */
        fflush(stdout);
        return;
    }

    flush_in_progress = 1;

    /* 1. Flush PHP output buffers to stdout (memfd) */
    int flush_count = 0;
    while (php_output_get_level() > 0) {
        php_output_flush();
        flush_count++;
        if (flush_count > 10) {
            /* Safety limit to prevent infinite loop */
            break;
        }
    }
    fflush(stdout);

    /* 2. Get new output since last stream offset */
    size_t offset = tokio_bridge_get_stream_offset();
    size_t len = 0;
    char *data = get_output_since_offset(&offset, &len);

    if (data == NULL || len == 0) {
        /* No new data to send */
        if (data) free(data);
        flush_in_progress = 0;
        return;
    }

    /* 3. Send chunk via bridge callback */
    tokio_bridge_send_chunk(data, len);

    /* 4. Update stream offset */
    tokio_bridge_set_stream_offset(offset);

    /* 5. Free buffer */
    free(data);

    flush_in_progress = 0;
}

/* tokio_stream_flush(): bool - flush output buffer and send to client
 *
 * For SSE/streaming mode. Sends any new output since last flush to the client.
 * Returns false if streaming mode is not enabled.
 *
 * Note: With the SAPI flush handler installed, standard flush() also works
 * for SSE streaming. This function is kept for explicit streaming control
 * and backward compatibility.
 *
 * Usage:
 *   header('Content-Type: text/event-stream');
 *   header('Cache-Control: no-cache');
 *
 *   while ($has_data) {
 *       echo "data: " . json_encode($data) . "\n\n";
 *       flush();  // Works via SAPI flush handler
 *       sleep(1);
 *   }
 */
PHP_FUNCTION(tokio_stream_flush)
{
    ZEND_PARSE_PARAMETERS_NONE();

    /* Check if streaming mode is enabled */
    if (!tokio_bridge_is_streaming()) {
        RETURN_FALSE;
    }

    /* 1. Flush PHP output buffers to stdout (memfd) */
    while (php_output_get_level() > 0) {
        php_output_flush();
    }
    fflush(stdout);

    /* 2. Get new output since last stream offset */
    size_t offset = tokio_bridge_get_stream_offset();
    size_t len = 0;
    char *data = get_output_since_offset(&offset, &len);

    if (data == NULL || len == 0) {
        /* No new data to send */
        if (data) free(data);
        RETURN_TRUE;  /* Not an error, just no new data */
    }

    /* 3. Send chunk via bridge callback */
    int result = tokio_bridge_send_chunk(data, len);

    /* 4. Update stream offset */
    tokio_bridge_set_stream_offset(offset);

    /* 5. Free buffer */
    free(data);

    RETURN_BOOL(result != 0);
}

/* tokio_is_streaming(): bool - check if streaming mode is enabled */
PHP_FUNCTION(tokio_is_streaming)
{
    ZEND_PARSE_PARAMETERS_NONE();
    RETURN_BOOL(tokio_bridge_is_streaming());
}

/* tokio_finish_request(): bool - send response to client, continue script execution
 *
 * Analog of fastcgi_finish_request(). After calling:
 * - Response body (so far) is marked for sending to client
 * - HTTP headers (so far) are captured for response
 * - Script continues executing (for cleanup, logging, etc.)
 * - Any further output is NOT sent to client
 *
 * Use case:
 *   echo "Response to user";
 *   tokio_finish_request();  // User gets response NOW
 *   // Do slow cleanup without keeping user waiting:
 *   send_email($user);
 *   log_to_database($analytics);
 *   sleep(5);  // User doesn't wait for this!
 *
 * Uses tokio_bridge shared library for direct Rust <-> PHP communication.
 * With streaming mode, triggers callback to send response immediately.
 */
PHP_FUNCTION(tokio_finish_request)
{
    ZEND_PARSE_PARAMETERS_NONE();

    /* Already finished? Return true (idempotent) */
    if (tokio_bridge_is_finished()) {
        RETURN_TRUE;
    }

    /* 1. Flush all PHP output buffers
     * In streaming mode, this triggers ub_write callback to send any remaining output */
    while (php_output_get_level() > 0) {
        php_output_end();
    }

    /* 2. Trigger stream finish (new streaming architecture)
     * This marks request as finished and invokes the stream finish callback
     * which sends ResponseChunk::End to close the response */
    int result = tokio_bridge_trigger_stream_finish();

    /* 3. Start a new output buffer for any post-finish output
     * This output will be discarded (ub_write checks is_finished flag) */
    php_output_start_default();

    RETURN_BOOL(result != 0);
}

/* ============================================================================
 * Finish Request C API (called from Rust)
 * Now delegates to tokio_bridge shared library.
 * ============================================================================ */

/* Check if tokio_finish_request() was called */
int tokio_sapi_is_request_finished(void)
{
    return tokio_bridge_is_finished();
}

/* Get the byte offset where output should be truncated */
size_t tokio_sapi_get_finished_offset(void)
{
    return tokio_bridge_get_finished_offset();
}

/* Get header count at finish time */
int tokio_sapi_get_finished_header_count(void)
{
    return tokio_bridge_get_finished_header_count();
}

/* Get response code at finish time */
int tokio_sapi_get_finished_response_code(void)
{
    return tokio_bridge_get_finished_response_code();
}

/* ============================================================================
 * Arginfo for PHP 8+ (fixes "Missing arginfo" warnings)
 * ============================================================================ */

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_tokio_request_id, 0, 0, IS_LONG, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_tokio_worker_id, 0, 0, IS_LONG, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_tokio_server_info, 0, 0, IS_ARRAY, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_MASK_EX(arginfo_tokio_async_call, 0, 2, MAY_BE_STRING|MAY_BE_FALSE)
    ZEND_ARG_TYPE_INFO(0, name, IS_STRING, 0)
    ZEND_ARG_TYPE_INFO(0, data, IS_STRING, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_tokio_request_heartbeat, 0, 0, _IS_BOOL, 0)
    ZEND_ARG_TYPE_INFO_WITH_DEFAULT_VALUE(0, time, IS_LONG, 0, "10")
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_tokio_finish_request, 0, 0, _IS_BOOL, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_tokio_stream_flush, 0, 0, _IS_BOOL, 0)
ZEND_END_ARG_INFO()

ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX(arginfo_tokio_is_streaming, 0, 0, _IS_BOOL, 0)
ZEND_END_ARG_INFO()

/* ============================================================================
 * PHP Extension registration
 * ============================================================================ */

static const zend_function_entry tokio_sapi_functions[] = {
    PHP_FE(tokio_request_id, arginfo_tokio_request_id)
    PHP_FE(tokio_worker_id, arginfo_tokio_worker_id)
    PHP_FE(tokio_server_info, arginfo_tokio_server_info)
    PHP_FE(tokio_async_call, arginfo_tokio_async_call)
    PHP_FE(tokio_request_heartbeat, arginfo_tokio_request_heartbeat)
    PHP_FE(tokio_finish_request, arginfo_tokio_finish_request)
    PHP_FE(tokio_stream_flush, arginfo_tokio_stream_flush)
    PHP_FE(tokio_is_streaming, arginfo_tokio_is_streaming)
    PHP_FE_END
};

/* Module init */
PHP_MINIT_FUNCTION(tokio_sapi)
{
    ZEND_INIT_MODULE_GLOBALS(tokio_sapi, php_tokio_sapi_init_globals, NULL);

    /* Register constants */
    REGISTER_STRING_CONSTANT("TOKIO_SAPI_VERSION", TOKIO_SAPI_VERSION,
                              CONST_CS | CONST_PERSISTENT);

    return SUCCESS;
}

/* Module shutdown */
PHP_MSHUTDOWN_FUNCTION(tokio_sapi)
{
    return SUCCESS;
}

/* Request init - called by PHP for each request */
PHP_RINIT_FUNCTION(tokio_sapi)
{
    return SUCCESS;
}

/* Request shutdown - called by PHP for each request */
PHP_RSHUTDOWN_FUNCTION(tokio_sapi)
{
    /* Don't free context here - Rust manages lifecycle via tokio_sapi_request_shutdown() */
    return SUCCESS;
}

/* Module info */
PHP_MINFO_FUNCTION(tokio_sapi)
{
    php_info_print_table_start();
    php_info_print_table_header(2, "tokio_sapi support", "enabled");
    php_info_print_table_row(2, "Version", TOKIO_SAPI_VERSION);
    php_info_print_table_row(2, "Thread Safety", "ZTS with __thread TLS");
    php_info_print_table_end();
}

/* Module entry */
zend_module_entry tokio_sapi_module_entry = {
    STANDARD_MODULE_HEADER,
    TOKIO_SAPI_EXTNAME,
    tokio_sapi_functions,
    PHP_MINIT(tokio_sapi),
    PHP_MSHUTDOWN(tokio_sapi),
    PHP_RINIT(tokio_sapi),
    PHP_RSHUTDOWN(tokio_sapi),
    PHP_MINFO(tokio_sapi),
    TOKIO_SAPI_VERSION,
    STANDARD_MODULE_PROPERTIES
};

#ifdef COMPILE_DL_TOKIO_SAPI
ZEND_GET_MODULE(tokio_sapi)
#endif

/* ============================================================================
 * Standalone initialization (when linked statically)
 * ============================================================================ */

int tokio_sapi_init(void)
{
    return SUCCESS;
}

void tokio_sapi_shutdown(void)
{
    /* Cleanup thread-local context if any */
    free_request_context();
}
