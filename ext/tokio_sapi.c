/*
 * tokio_sapi - PHP extension for tokio_php server
 *
 * This extension provides direct access to PHP internals,
 * bypassing the need for zend_eval_string() to set superglobals.
 */

#include "tokio_sapi.h"

/* ============================================================================
 * Module globals
 * ============================================================================ */

ZEND_DECLARE_MODULE_GLOBALS(tokio_sapi)

static void php_tokio_sapi_init_globals(zend_tokio_sapi_globals *globals)
{
    memset(globals, 0, sizeof(zend_tokio_sapi_globals));
}

/* ============================================================================
 * Request context management
 * ============================================================================ */

static tokio_request_context* get_request_context(void)
{
    if (TOKIO_G(request_ctx) == NULL) {
        TOKIO_G(request_ctx) = ecalloc(1, sizeof(tokio_request_context));
        TOKIO_G(request_ctx)->http_response_code = 200;
    }
    return TOKIO_G(request_ctx);
}

static void free_request_context(void)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);
    if (ctx == NULL) return;

    /* Free POST data */
    if (ctx->post_data) {
        efree(ctx->post_data);
    }

    /* Free output buffer */
    smart_str_free(&ctx->output_buffer);

    /* Free headers */
    for (int i = 0; i < ctx->header_count; i++) {
        if (ctx->headers[i].name) efree(ctx->headers[i].name);
        if (ctx->headers[i].value) efree(ctx->headers[i].value);
    }

    efree(ctx);
    TOKIO_G(request_ctx) = NULL;
}

/* ============================================================================
 * Superglobals manipulation (the main performance win!)
 * ============================================================================ */

/* Helper: get or create a superglobal array */
static zval* get_superglobal(int track_var)
{
    zval *arr = &PG(http_globals)[track_var];

    if (Z_TYPE_P(arr) != IS_ARRAY) {
        array_init(arr);
    }

    return arr;
}

/* Helper: set a value in a superglobal */
static void set_superglobal_value(int track_var, const char *key, size_t key_len,
                                   const char *value, size_t value_len)
{
    zval *arr = get_superglobal(track_var);
    zval zv;

    ZVAL_STRINGL(&zv, value, value_len);
    zend_hash_str_update(Z_ARRVAL_P(arr), key, key_len, &zv);
}

/* Clear a superglobal array */
static void clear_superglobal(int track_var)
{
    zval *arr = &PG(http_globals)[track_var];

    if (Z_TYPE_P(arr) == IS_ARRAY) {
        zend_hash_clean(Z_ARRVAL_P(arr));
    } else {
        array_init(arr);
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

/* Public API: set $_FILES variable (single file) */
void tokio_sapi_set_files_var(const char *field, size_t field_len,
                               const char *name, const char *type,
                               const char *tmp_name, int error, size_t size)
{
    zval *files_arr = get_superglobal(TRACK_VARS_FILES);
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
}

/* Public API: build $_REQUEST from $_GET + $_POST */
void tokio_sapi_build_request(void)
{
    zval *get_arr = get_superglobal(TRACK_VARS_GET);
    zval *post_arr = get_superglobal(TRACK_VARS_POST);
    zval request_arr;
    zend_string *key;
    zval *val;

    array_init(&request_arr);

    /* Copy $_GET */
    ZEND_HASH_FOREACH_STR_KEY_VAL(Z_ARRVAL_P(get_arr), key, val) {
        Z_TRY_ADDREF_P(val);
        zend_hash_update(Z_ARRVAL(request_arr), key, val);
    } ZEND_HASH_FOREACH_END();

    /* Merge $_POST (overwrites GET) */
    ZEND_HASH_FOREACH_STR_KEY_VAL(Z_ARRVAL_P(post_arr), key, val) {
        Z_TRY_ADDREF_P(val);
        zend_hash_update(Z_ARRVAL(request_arr), key, val);
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

    if (ctx->post_data) {
        efree(ctx->post_data);
    }

    if (data && len > 0) {
        ctx->post_data = emalloc(len + 1);
        memcpy(ctx->post_data, data, len);
        ctx->post_data[len] = '\0';
        ctx->post_data_len = len;
    } else {
        ctx->post_data = NULL;
        ctx->post_data_len = 0;
    }
    ctx->post_data_read = 0;

    /* Also set SG(request_info).raw_post_data for compatibility */
    SG(request_info).request_body = NULL;
    SG(request_info).content_type_dup = NULL;
}

/* SAPI read_post callback - reads POST body for php://input */
static size_t tokio_sapi_read_post(char *buffer, size_t count)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);

    if (ctx == NULL || ctx->post_data == NULL) {
        return 0;
    }

    size_t remaining = ctx->post_data_len - ctx->post_data_read;
    size_t to_read = (count < remaining) ? count : remaining;

    if (to_read > 0) {
        memcpy(buffer, ctx->post_data + ctx->post_data_read, to_read);
        ctx->post_data_read += to_read;
    }

    return to_read;
}

/* ============================================================================
 * Output capture via PHP output handler
 * ============================================================================ */

/* Output handler callback */
static int tokio_output_handler(void **handler_context, php_output_context *output_context)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);

    if (ctx == NULL) {
        return FAILURE;
    }

    if (output_context->in.used > 0) {
        /* Append to our buffer */
        smart_str_appendl(&ctx->output_buffer, output_context->in.data, output_context->in.used);

        /* Also call Rust callback if set */
        if (TOKIO_G(write_output_callback)) {
            TOKIO_G(write_output_callback)(output_context->in.data, output_context->in.used);
        }
    }

    /* Pass through to next handler */
    output_context->out.data = output_context->in.data;
    output_context->out.used = output_context->in.used;
    output_context->out.free = 0;

    return SUCCESS;
}

void tokio_sapi_start_output_capture(void)
{
    tokio_request_context *ctx = get_request_context();

    if (!ctx->output_handler_started) {
        php_output_handler *handler = php_output_handler_create_internal(
            ZEND_STRL("tokio_sapi"),
            tokio_output_handler,
            0,
            PHP_OUTPUT_HANDLER_STDFLAGS
        );

        if (handler) {
            php_output_handler_start(handler);
            ctx->output_handler_started = 1;
        }
    }
}

const char* tokio_sapi_get_output(size_t *len)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);

    if (ctx == NULL || ctx->output_buffer.s == NULL) {
        *len = 0;
        return "";
    }

    *len = ZSTR_LEN(ctx->output_buffer.s);
    return ZSTR_VAL(ctx->output_buffer.s);
}

void tokio_sapi_clear_output(void)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);

    if (ctx) {
        smart_str_free(&ctx->output_buffer);
        memset(&ctx->output_buffer, 0, sizeof(smart_str));
    }
}

/* ============================================================================
 * Header capture
 * ============================================================================ */

/* SAPI header handler - intercepts header() calls */
static int tokio_sapi_header_handler(sapi_header_struct *sapi_header,
                                      sapi_header_op_enum op,
                                      sapi_headers_struct *sapi_headers)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);

    if (ctx == NULL) {
        return SAPI_HEADER_ADD;
    }

    /* Always capture response code */
    if (sapi_headers) {
        ctx->http_response_code = sapi_headers->http_response_code;
    }

    if (sapi_header == NULL || sapi_header->header == NULL) {
        return SAPI_HEADER_ADD;
    }

    switch (op) {
        case SAPI_HEADER_REPLACE:
        case SAPI_HEADER_ADD: {
            /* Parse "Name: Value" */
            char *colon = strchr(sapi_header->header, ':');
            if (colon && ctx->header_count < TOKIO_MAX_HEADERS) {
                size_t name_len = colon - sapi_header->header;
                char *value = colon + 1;
                while (*value == ' ') value++;

                int idx = ctx->header_count;

                /* For REPLACE, check if header exists */
                if (op == SAPI_HEADER_REPLACE) {
                    for (int i = 0; i < ctx->header_count; i++) {
                        if (ctx->headers[i].name &&
                            strncasecmp(ctx->headers[i].name, sapi_header->header, name_len) == 0) {
                            /* Replace existing */
                            efree(ctx->headers[i].value);
                            ctx->headers[i].value = estrdup(value);
                            return SAPI_HEADER_ADD;
                        }
                    }
                }

                /* Add new header */
                ctx->headers[idx].name = estrndup(sapi_header->header, name_len);
                ctx->headers[idx].value = estrdup(value);
                ctx->header_count++;

                /* Call Rust callback */
                if (TOKIO_G(send_header_callback)) {
                    TOKIO_G(send_header_callback)(
                        ctx->headers[idx].name, name_len,
                        ctx->headers[idx].value, strlen(ctx->headers[idx].value)
                    );
                }
            }
            break;
        }

        case SAPI_HEADER_DELETE: {
            /* Find and remove header */
            for (int i = 0; i < ctx->header_count; i++) {
                if (ctx->headers[i].name &&
                    strcasecmp(ctx->headers[i].name, sapi_header->header) == 0) {
                    efree(ctx->headers[i].name);
                    efree(ctx->headers[i].value);
                    /* Shift remaining headers */
                    memmove(&ctx->headers[i], &ctx->headers[i+1],
                            (ctx->header_count - i - 1) * sizeof(ctx->headers[0]));
                    ctx->header_count--;
                    break;
                }
            }
            break;
        }

        case SAPI_HEADER_DELETE_ALL:
            for (int i = 0; i < ctx->header_count; i++) {
                if (ctx->headers[i].name) efree(ctx->headers[i].name);
                if (ctx->headers[i].value) efree(ctx->headers[i].value);
            }
            ctx->header_count = 0;
            break;

        case SAPI_HEADER_SET_STATUS:
            /* Status code already captured above */
            break;
    }

    return SAPI_HEADER_ADD;
}

/* Public API: get header count */
int tokio_sapi_get_header_count(void)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);
    return ctx ? ctx->header_count : 0;
}

/* Public API: get header name by index */
const char* tokio_sapi_get_header_name(int index)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);
    if (ctx && index >= 0 && index < ctx->header_count) {
        return ctx->headers[index].name;
    }
    return NULL;
}

/* Public API: get header value by index */
const char* tokio_sapi_get_header_value(int index)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);
    if (ctx && index >= 0 && index < ctx->header_count) {
        return ctx->headers[index].value;
    }
    return NULL;
}

/* Public API: get response code */
int tokio_sapi_get_response_code(void)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);
    return ctx ? ctx->http_response_code : 200;
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
 * Request lifecycle
 * ============================================================================ */

int tokio_sapi_request_init(uint64_t request_id)
{
    tokio_request_context *ctx = get_request_context();
    ctx->request_id = request_id;
    ctx->http_response_code = 200;

    /* Clear captured headers */
    for (int i = 0; i < ctx->header_count; i++) {
        if (ctx->headers[i].name) efree(ctx->headers[i].name);
        if (ctx->headers[i].value) efree(ctx->headers[i].value);
    }
    ctx->header_count = 0;

    /* Clear output buffer */
    smart_str_free(&ctx->output_buffer);
    memset(&ctx->output_buffer, 0, sizeof(smart_str));
    ctx->output_handler_started = 0;

    return SUCCESS;
}

void tokio_sapi_request_shutdown(void)
{
    free_request_context();
}

/* ============================================================================
 * Callback registration
 * ============================================================================ */

void tokio_sapi_set_callbacks(
    tokio_read_post_fn read_post,
    tokio_write_output_fn write_output,
    tokio_send_header_fn send_header,
    tokio_async_call_fn async_call)
{
    TOKIO_G(read_post_callback) = read_post;
    TOKIO_G(write_output_callback) = write_output;
    TOKIO_G(send_header_callback) = send_header;
    TOKIO_G(async_call_callback) = async_call;
}

/* ============================================================================
 * PHP Functions (available from PHP scripts)
 * ============================================================================ */

/* tokio_request_id(): int - get current request ID */
PHP_FUNCTION(tokio_request_id)
{
    tokio_request_context *ctx = TOKIO_G(request_ctx);
    if (ctx) {
        RETURN_LONG(ctx->request_id);
    }
    RETURN_LONG(0);
}

/* tokio_worker_id(): int - get worker thread ID */
PHP_FUNCTION(tokio_worker_id)
{
    /* TODO: Pass from Rust */
    RETURN_LONG(0);
}

/* tokio_server_info(): array - get server information */
PHP_FUNCTION(tokio_server_info)
{
    array_init(return_value);
    add_assoc_string(return_value, "server", "tokio_php");
    add_assoc_string(return_value, "version", TOKIO_SAPI_VERSION);
    add_assoc_string(return_value, "sapi", "tokio_sapi");
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
            efree(result);
            return;
        }
    }

    RETURN_FALSE;
}

/* ============================================================================
 * PHP Extension registration
 * ============================================================================ */

static const zend_function_entry tokio_sapi_functions[] = {
    PHP_FE(tokio_request_id, NULL)
    PHP_FE(tokio_worker_id, NULL)
    PHP_FE(tokio_server_info, NULL)
    PHP_FE(tokio_async_call, NULL)
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

/* Request init */
PHP_RINIT_FUNCTION(tokio_sapi)
{
    return SUCCESS;
}

/* Request shutdown */
PHP_RSHUTDOWN_FUNCTION(tokio_sapi)
{
    return SUCCESS;
}

/* Module info */
PHP_MINFO_FUNCTION(tokio_sapi)
{
    php_info_print_table_start();
    php_info_print_table_header(2, "tokio_sapi support", "enabled");
    php_info_print_table_row(2, "Version", TOKIO_SAPI_VERSION);
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
    /* Module is auto-initialized via zend_startup_module */
    return SUCCESS;
}

void tokio_sapi_shutdown(void)
{
    /* Cleanup handled by module shutdown */
}
