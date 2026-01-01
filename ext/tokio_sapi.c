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
#include <stdlib.h>

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

/* Batch set superglobal from packed buffer:
 * Buffer format: [key_len:u32][key\0][val_len:u32][val]...
 * key_len includes null terminator, val_len does not
 * Returns number of variables set */
static int set_superglobal_batch(int track_var, const char *buffer, size_t buffer_len, size_t count)
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

        /* Direct hash update (faster than php_register_variable_safe) */
        zval zval_val;
        ZVAL_STRINGL(&zval_val, val, val_len);
        zend_hash_str_update(Z_ARRVAL_P(arr), key, key_str_len, &zval_val);
        set_count++;
    }

    return set_count;
}

/* Public API: batch set $_SERVER variables */
int tokio_sapi_set_server_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_SERVER, buffer, buffer_len, count);
}

/* Public API: batch set $_GET variables */
int tokio_sapi_set_get_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_GET, buffer, buffer_len, count);
}

/* Public API: batch set $_POST variables */
int tokio_sapi_set_post_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_POST, buffer, buffer_len, count);
}

/* Public API: batch set $_COOKIE variables */
int tokio_sapi_set_cookie_vars_batch(const char *buffer, size_t buffer_len, size_t count)
{
    return set_superglobal_batch(TRACK_VARS_COOKIE, buffer, buffer_len, count);
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
        set_superglobal_batch(TRACK_VARS_SERVER, server_buf, server_len, server_count);
    }
    if (get_count > 0) {
        set_superglobal_batch(TRACK_VARS_GET, get_buf, get_len, get_count);
    }
    if (post_count > 0) {
        set_superglobal_batch(TRACK_VARS_POST, post_buf, post_len, post_count);
    }
    if (cookie_count > 0) {
        set_superglobal_batch(TRACK_VARS_COOKIE, cookie_buf, cookie_len, cookie_count);
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
    ZEND_PARSE_PARAMETERS_NONE();

    array_init(return_value);
    add_assoc_string(return_value, "server", "tokio_php");
    add_assoc_string(return_value, "version", TOKIO_SAPI_VERSION);
    add_assoc_string(return_value, "sapi", "tokio_sapi");
    add_assoc_bool(return_value, "zts", 1);
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
 */
PHP_FUNCTION(tokio_request_heartbeat)
{
    zend_long time = 10;

    ZEND_PARSE_PARAMETERS_START(0, 1)
        Z_PARAM_OPTIONAL
        Z_PARAM_LONG(time)
    ZEND_PARSE_PARAMETERS_END();

    /* Get heartbeat info from $_SERVER (set by Rust via superglobals)
     * This is necessary because the extension (.so) and static library (.a)
     * have separate TLS storage when loaded dynamically. */
    zval *server_arr = zend_hash_str_find(&EG(symbol_table), "_SERVER", sizeof("_SERVER")-1);
    if (!server_arr || Z_TYPE_P(server_arr) != IS_ARRAY) {
        RETURN_FALSE;
    }

    /* Get context pointer from $_SERVER['TOKIO_HEARTBEAT_CTX'] */
    zval *ctx_val = zend_hash_str_find(Z_ARRVAL_P(server_arr), "TOKIO_HEARTBEAT_CTX", sizeof("TOKIO_HEARTBEAT_CTX")-1);
    if (!ctx_val || Z_TYPE_P(ctx_val) != IS_STRING) {
        RETURN_FALSE;
    }
    void *ctx = (void*)strtoull(Z_STRVAL_P(ctx_val), NULL, 16);
    if (ctx == NULL) {
        RETURN_FALSE;
    }

    /* Get max_secs from $_SERVER['TOKIO_HEARTBEAT_MAX_SECS'] */
    zval *max_val = zend_hash_str_find(Z_ARRVAL_P(server_arr), "TOKIO_HEARTBEAT_MAX_SECS", sizeof("TOKIO_HEARTBEAT_MAX_SECS")-1);
    if (!max_val || Z_TYPE_P(max_val) != IS_STRING) {
        RETURN_FALSE;
    }
    uint64_t max_secs = strtoull(Z_STRVAL_P(max_val), NULL, 10);
    if (max_secs == 0) {
        RETURN_FALSE;
    }

    /* Get callback pointer from $_SERVER['TOKIO_HEARTBEAT_CALLBACK'] */
    zval *cb_val = zend_hash_str_find(Z_ARRVAL_P(server_arr), "TOKIO_HEARTBEAT_CALLBACK", sizeof("TOKIO_HEARTBEAT_CALLBACK")-1);
    if (!cb_val || Z_TYPE_P(cb_val) != IS_STRING) {
        RETURN_FALSE;
    }
    tokio_heartbeat_fn_t callback = (tokio_heartbeat_fn_t)strtoull(Z_STRVAL_P(cb_val), NULL, 16);
    if (callback == NULL) {
        RETURN_FALSE;
    }

    /* Validate time parameter */
    if (time <= 0) {
        RETURN_FALSE;
    }

    /* Check against max extension limit */
    if ((uint64_t)time > max_secs) {
        RETURN_FALSE;
    }

    /* Call Rust callback to update deadline */
    int64_t result = callback(ctx, (uint64_t)time);

    RETURN_BOOL(result != 0);
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

/* ============================================================================
 * PHP Extension registration
 * ============================================================================ */

static const zend_function_entry tokio_sapi_functions[] = {
    PHP_FE(tokio_request_id, arginfo_tokio_request_id)
    PHP_FE(tokio_worker_id, arginfo_tokio_worker_id)
    PHP_FE(tokio_server_info, arginfo_tokio_server_info)
    PHP_FE(tokio_async_call, arginfo_tokio_async_call)
    PHP_FE(tokio_request_heartbeat, arginfo_tokio_request_heartbeat)
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
