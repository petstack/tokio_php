/*
 * tokio_bridge - Shared library implementation
 *
 * Uses C11 __thread for thread-local storage, providing a single TLS
 * instance that both Rust (via FFI) and PHP extension can access.
 */

#define _POSIX_C_SOURCE 200809L
#include "bridge.h"
#include <stdlib.h>
#include <string.h>
#include <strings.h>  /* for strncasecmp */

/* ============================================================================
 * Thread-local storage
 *
 * Single __thread variable ensures both Rust FFI calls and PHP extension
 * access the same context (unlike separate static libs which have isolated TLS).
 * ============================================================================ */

static __thread tokio_bridge_ctx_t *tls_ctx = NULL;

/* ============================================================================
 * Context lifecycle
 * ============================================================================ */

tokio_bridge_ctx_t* tokio_bridge_get_ctx(void)
{
    return tls_ctx;
}

void tokio_bridge_init_ctx(uint64_t request_id, uint64_t worker_id)
{
    /* Free any existing context */
    if (tls_ctx != NULL) {
        tokio_bridge_destroy_ctx();
    }

    /* Allocate and initialize new context */
    tls_ctx = (tokio_bridge_ctx_t*)calloc(1, sizeof(tokio_bridge_ctx_t));
    if (tls_ctx == NULL) {
        return;  /* Allocation failed */
    }

    tls_ctx->request_id = request_id;
    tls_ctx->worker_id = worker_id;
    tls_ctx->response_code = 200;  /* Default response code */
}

void tokio_bridge_destroy_ctx(void)
{
    if (tls_ctx != NULL) {
        /* Free header strings */
        for (int i = 0; i < tls_ctx->header_count; i++) {
            if (tls_ctx->headers[i].name) {
                free(tls_ctx->headers[i].name);
            }
            if (tls_ctx->headers[i].value) {
                free(tls_ctx->headers[i].value);
            }
        }
        free(tls_ctx);
        tls_ctx = NULL;
    }
}

/* ============================================================================
 * Finish Request API
 * ============================================================================ */

void tokio_bridge_mark_finished(size_t offset, int header_count, int response_code)
{
    if (tls_ctx == NULL) {
        return;
    }

    /* Idempotent - only mark once */
    if (tls_ctx->is_finished) {
        return;
    }

    tls_ctx->is_finished = 1;
    tls_ctx->output_offset = offset;
    tls_ctx->finished_header_count = header_count;
    tls_ctx->response_code = response_code;
}

int tokio_bridge_is_finished(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    return tls_ctx->is_finished;
}

size_t tokio_bridge_get_finished_offset(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    return tls_ctx->output_offset;
}

int tokio_bridge_get_finished_header_count(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    return tls_ctx->finished_header_count;
}

int tokio_bridge_get_finished_response_code(void)
{
    if (tls_ctx == NULL) {
        return 200;
    }
    return tls_ctx->response_code;
}

/* ============================================================================
 * Finish Request Callback API (streaming early response)
 * ============================================================================ */

void tokio_bridge_set_finish_callback(void *ctx, tokio_finish_callback_t callback)
{
    if (tls_ctx == NULL) {
        return;
    }
    tls_ctx->finish_ctx = ctx;
    tls_ctx->finish_callback = callback;
}

int tokio_bridge_trigger_finish(
    const char *body,
    size_t body_len,
    const char *headers,
    size_t headers_len,
    int header_count,
    int status_code)
{
    if (tls_ctx == NULL) {
        return 0;
    }

    /* Idempotent - only trigger once */
    if (tls_ctx->is_finished) {
        return 0;
    }

    /* Mark as finished first (prevents re-entry) */
    tls_ctx->is_finished = 1;
    tls_ctx->output_offset = body_len;
    tls_ctx->finished_header_count = header_count;
    tls_ctx->response_code = status_code;

    /* If callback is set, invoke it with response data */
    if (tls_ctx->finish_callback != NULL) {
        tls_ctx->finish_callback(
            tls_ctx->finish_ctx,
            body,
            body_len,
            headers,
            headers_len,
            header_count,
            status_code
        );
    }

    return 1;
}

/* ============================================================================
 * Heartbeat API
 * ============================================================================ */

void tokio_bridge_set_heartbeat(
    void *ctx,
    uint64_t max_secs,
    tokio_heartbeat_callback_t callback)
{
    if (tls_ctx == NULL) {
        return;
    }
    tls_ctx->heartbeat_ctx = ctx;
    tls_ctx->heartbeat_max_secs = max_secs;
    tls_ctx->heartbeat_callback = callback;
}

int tokio_bridge_send_heartbeat(uint64_t secs)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    if (tls_ctx->heartbeat_callback == NULL) {
        return 0;
    }
    if (secs == 0) {
        return 0;
    }
    if (secs > tls_ctx->heartbeat_max_secs) {
        return 0;
    }

    /* Call the Rust callback */
    int64_t result = tls_ctx->heartbeat_callback(tls_ctx->heartbeat_ctx, secs);
    return (result != 0) ? 1 : 0;
}

uint64_t tokio_bridge_get_heartbeat_max(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    return tls_ctx->heartbeat_max_secs;
}

/* ============================================================================
 * Header Storage API
 * ============================================================================ */

int tokio_bridge_add_header(
    const char *name,
    size_t name_len,
    const char *value,
    size_t value_len,
    int replace)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    if (name == NULL || name_len == 0) {
        return 0;
    }

    /* For replace mode, find and update existing header */
    if (replace) {
        for (int i = 0; i < tls_ctx->header_count; i++) {
            if (tls_ctx->headers[i].name &&
                strlen(tls_ctx->headers[i].name) == name_len &&
                strncasecmp(tls_ctx->headers[i].name, name, name_len) == 0) {
                /* Replace existing value */
                free(tls_ctx->headers[i].value);
                tls_ctx->headers[i].value = (char*)malloc(value_len + 1);
                if (tls_ctx->headers[i].value) {
                    memcpy(tls_ctx->headers[i].value, value, value_len);
                    tls_ctx->headers[i].value[value_len] = '\0';
                }
                return 1;
            }
        }
    }

    /* Check capacity */
    if (tls_ctx->header_count >= TOKIO_BRIDGE_MAX_HEADERS) {
        return 0;
    }

    /* Add new header */
    int idx = tls_ctx->header_count;
    tls_ctx->headers[idx].name = (char*)malloc(name_len + 1);
    tls_ctx->headers[idx].value = (char*)malloc(value_len + 1);

    if (tls_ctx->headers[idx].name && tls_ctx->headers[idx].value) {
        memcpy(tls_ctx->headers[idx].name, name, name_len);
        tls_ctx->headers[idx].name[name_len] = '\0';
        memcpy(tls_ctx->headers[idx].value, value, value_len);
        tls_ctx->headers[idx].value[value_len] = '\0';
        tls_ctx->header_count++;
        return 1;
    }

    /* Cleanup on failure */
    if (tls_ctx->headers[idx].name) {
        free(tls_ctx->headers[idx].name);
        tls_ctx->headers[idx].name = NULL;
    }
    if (tls_ctx->headers[idx].value) {
        free(tls_ctx->headers[idx].value);
        tls_ctx->headers[idx].value = NULL;
    }
    return 0;
}

int tokio_bridge_get_header_count(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    return tls_ctx->header_count;
}

int tokio_bridge_get_header(int index, const char **name, const char **value)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    if (index < 0 || index >= tls_ctx->header_count) {
        return 0;
    }
    if (name) {
        *name = tls_ctx->headers[index].name;
    }
    if (value) {
        *value = tls_ctx->headers[index].value;
    }
    return 1;
}

void tokio_bridge_clear_headers(void)
{
    if (tls_ctx == NULL) {
        return;
    }

    /* Free all header strings */
    for (int i = 0; i < tls_ctx->header_count; i++) {
        if (tls_ctx->headers[i].name) {
            free(tls_ctx->headers[i].name);
            tls_ctx->headers[i].name = NULL;
        }
        if (tls_ctx->headers[i].value) {
            free(tls_ctx->headers[i].value);
            tls_ctx->headers[i].value = NULL;
        }
    }
    tls_ctx->header_count = 0;
}

/* ============================================================================
 * Streaming API (SSE support)
 * ============================================================================ */

void tokio_bridge_enable_streaming(void *ctx, tokio_stream_chunk_callback_t callback)
{
    if (tls_ctx == NULL) {
        return;
    }
    tls_ctx->is_streaming = 1;
    tls_ctx->stream_offset = 0;
    tls_ctx->stream_ctx = ctx;
    tls_ctx->stream_callback = callback;
}

void tokio_bridge_set_stream_callback(void *ctx, tokio_stream_chunk_callback_t callback)
{
    if (tls_ctx == NULL) {
        return;
    }
    /* Set callback without enabling streaming - streaming is enabled later
     * when PHP sets Content-Type: text/event-stream header */
    tls_ctx->stream_ctx = ctx;
    tls_ctx->stream_callback = callback;
    tls_ctx->stream_offset = 0;
    /* Note: is_streaming remains 0 until try_enable_streaming is called */
}

int tokio_bridge_try_enable_streaming(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    /* Already streaming */
    if (tls_ctx->is_streaming) {
        return 1;
    }
    /* No callback configured - can't enable streaming */
    if (tls_ctx->stream_callback == NULL) {
        return 0;
    }
    /* Enable streaming */
    tls_ctx->is_streaming = 1;
    return 1;
}

int tokio_bridge_is_streaming(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    return tls_ctx->is_streaming;
}

int tokio_bridge_send_chunk(const char *data, size_t data_len)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    if (!tls_ctx->is_streaming) {
        return 0;
    }
    if (tls_ctx->stream_callback == NULL) {
        return 0;
    }
    if (data == NULL || data_len == 0) {
        return 0;
    }

    /* Call the Rust callback */
    tls_ctx->stream_callback(tls_ctx->stream_ctx, data, data_len);
    return 1;
}

size_t tokio_bridge_get_stream_offset(void)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    return tls_ctx->stream_offset;
}

void tokio_bridge_set_stream_offset(size_t offset)
{
    if (tls_ctx == NULL) {
        return;
    }
    tls_ctx->stream_offset = offset;
}

void tokio_bridge_end_stream(void)
{
    if (tls_ctx == NULL) {
        return;
    }
    tls_ctx->is_streaming = 0;
    tls_ctx->stream_offset = 0;
    tls_ctx->stream_ctx = NULL;
    tls_ctx->stream_callback = NULL;
}
