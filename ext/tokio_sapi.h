/*
 * tokio_sapi - PHP extension for tokio_php server
 *
 * Provides direct access to PHP internals for:
 * - Setting superglobals without eval
 * - Capturing output via output handler
 * - Reading POST body for php://input
 * - Rust <-> PHP communication
 */

#ifndef TOKIO_SAPI_H
#define TOKIO_SAPI_H

#ifdef HAVE_CONFIG_H
#include "config.h"
#endif

#include "php.h"
#include "php_ini.h"
#include "ext/standard/info.h"
#include "ext/standard/php_string.h"
#include "SAPI.h"
#include "zend_API.h"
#include "zend_hash.h"
#include "zend_smart_str.h"
#include "main/php_output.h"
#include "main/php_main.h"
#include "main/php_streams.h"
#include "main/php_variables.h"

/* Extension version */
#define TOKIO_SAPI_VERSION "0.1.0"
#define TOKIO_SAPI_EXTNAME "tokio_sapi"

/* Maximum sizes */
#define TOKIO_MAX_OUTPUT_SIZE (64 * 1024 * 1024)  /* 64MB max output */
#define TOKIO_MAX_POST_SIZE   (32 * 1024 * 1024)  /* 32MB max POST */
#define TOKIO_MAX_HEADERS     128

/* ============================================================================
 * Request context - per-request state (thread-local in ZTS)
 * ============================================================================ */

typedef struct {
    /* POST body buffer */
    char *post_data;
    size_t post_data_len;
    size_t post_data_read;

    /* Output capture buffer */
    smart_str output_buffer;
    int output_handler_started;

    /* Captured headers */
    struct {
        char *name;
        char *value;
    } headers[TOKIO_MAX_HEADERS];
    int header_count;
    int http_response_code;

    /* Request metadata */
    uint64_t request_id;
    int profiling_enabled;
} tokio_request_context;

/* ============================================================================
 * Rust FFI callbacks - set by Rust before request
 * ============================================================================ */

/* Callback to read POST data from Rust */
typedef size_t (*tokio_read_post_fn)(char *buffer, size_t max_len);

/* Callback to send captured output to Rust */
typedef void (*tokio_write_output_fn)(const char *data, size_t len);

/* Callback to send header to Rust */
typedef void (*tokio_send_header_fn)(const char *name, size_t name_len,
                                      const char *value, size_t value_len);

/* Callback for async operations (future use) */
typedef int (*tokio_async_call_fn)(const char *name, const char *data,
                                    size_t data_len, char **result, size_t *result_len);

/* ============================================================================
 * Global state (only for callbacks - request context uses __thread TLS)
 * ============================================================================ */

ZEND_BEGIN_MODULE_GLOBALS(tokio_sapi)
    /* Rust callbacks */
    tokio_read_post_fn read_post_callback;
    tokio_write_output_fn write_output_callback;
    tokio_send_header_fn send_header_callback;
    tokio_async_call_fn async_call_callback;
ZEND_END_MODULE_GLOBALS(tokio_sapi)

#ifdef ZTS
#define TOKIO_G(v) ZEND_MODULE_GLOBALS_ACCESSOR(tokio_sapi, v)
#else
#define TOKIO_G(v) (tokio_sapi_globals.v)
#endif

/* ============================================================================
 * Public C API - called from Rust via FFI
 * ============================================================================ */

/* Initialize extension (call once at startup) */
int tokio_sapi_init(void);

/* Shutdown extension */
void tokio_sapi_shutdown(void);

/* Set callbacks from Rust */
void tokio_sapi_set_callbacks(
    tokio_read_post_fn read_post,
    tokio_write_output_fn write_output,
    tokio_send_header_fn send_header,
    tokio_async_call_fn async_call
);

/* Request lifecycle */
int tokio_sapi_request_init(uint64_t request_id);
void tokio_sapi_request_shutdown(void);

/* Set POST body for php://input */
void tokio_sapi_set_post_data(const char *data, size_t len);

/* Set superglobals directly (no eval!) */
void tokio_sapi_set_server_var(const char *key, size_t key_len,
                                const char *value, size_t value_len);
void tokio_sapi_set_get_var(const char *key, size_t key_len,
                             const char *value, size_t value_len);
void tokio_sapi_set_post_var(const char *key, size_t key_len,
                              const char *value, size_t value_len);
void tokio_sapi_set_cookie_var(const char *key, size_t key_len,
                                const char *value, size_t value_len);

/* Batch API - set multiple variables in one FFI call
 * Buffer format: [key_len:u32][key][val_len:u32][val]...
 * Returns number of variables set */
int tokio_sapi_set_server_vars_batch(const char *buffer, size_t buffer_len, size_t count);
int tokio_sapi_set_get_vars_batch(const char *buffer, size_t buffer_len, size_t count);
int tokio_sapi_set_post_vars_batch(const char *buffer, size_t buffer_len, size_t count);
int tokio_sapi_set_cookie_vars_batch(const char *buffer, size_t buffer_len, size_t count);

/* Ultra-batch API - set ALL superglobals in one FFI call
 * Buffer format per superglobal: [count:u32][key_len:u32][key\0][val_len:u32][val]...
 * Order: SERVER, GET, POST, COOKIE
 * Performs: clear, init caches, set all vars, build $_REQUEST, init request state */
void tokio_sapi_set_all_superglobals(
    const char *server_buf, size_t server_len, size_t server_count,
    const char *get_buf, size_t get_len, size_t get_count,
    const char *post_buf, size_t post_len, size_t post_count,
    const char *cookie_buf, size_t cookie_len, size_t cookie_count
);

void tokio_sapi_set_files_var(const char *field, size_t field_len,
                               const char *name, const char *type,
                               const char *tmp_name, int error, size_t size);

/* Clear all superglobals (call before setting new values) */
void tokio_sapi_clear_superglobals(void);

/* Initialize superglobal caches (call once before batch operations) */
void tokio_sapi_init_superglobals(void);

/* Initialize request state (replaces header_remove();ob_start() eval) */
void tokio_sapi_init_request_state(void);

/* Build $_REQUEST from $_GET + $_POST */
void tokio_sapi_build_request(void);

/* Start/stop output capture */
void tokio_sapi_start_output_capture(void);
const char* tokio_sapi_get_output(size_t *len);
void tokio_sapi_clear_output(void);

/* Get captured headers */
int tokio_sapi_get_header_count(void);
const char* tokio_sapi_get_header_name(int index);
const char* tokio_sapi_get_header_value(int index);
int tokio_sapi_get_response_code(void);

/* Execute script */
int tokio_sapi_execute_script(const char *path);

#endif /* TOKIO_SAPI_H */
