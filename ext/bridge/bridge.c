/*
 * tokio_bridge - Shared library implementation
 *
 * Uses C11 __thread for thread-local storage, providing a single TLS
 * instance that both Rust (via FFI) and PHP extension can access.
 */

#include "bridge.h"
#include <stdlib.h>
#include <string.h>

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
        free(tls_ctx);
        tls_ctx = NULL;
    }
}

/* ============================================================================
 * Early Hints API
 * ============================================================================ */

void tokio_bridge_set_hints_callback(void *ctx, tokio_early_hints_callback_t callback)
{
    if (tls_ctx == NULL) {
        return;
    }
    tls_ctx->hints_ctx = ctx;
    tls_ctx->hints_callback = callback;
}

int tokio_bridge_send_early_hints(const char **headers, size_t count)
{
    if (tls_ctx == NULL) {
        return 0;
    }
    if (tls_ctx->hints_callback == NULL) {
        return 0;
    }
    if (headers == NULL || count == 0) {
        return 0;
    }

    /* Limit count to prevent abuse */
    if (count > TOKIO_BRIDGE_MAX_EARLY_HINTS) {
        count = TOKIO_BRIDGE_MAX_EARLY_HINTS;
    }

    /* Call the Rust callback */
    tls_ctx->hints_callback(tls_ctx->hints_ctx, headers, count);
    return 1;
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
    tls_ctx->header_count = header_count;
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
    return tls_ctx->header_count;
}

int tokio_bridge_get_finished_response_code(void)
{
    if (tls_ctx == NULL) {
        return 200;
    }
    return tls_ctx->response_code;
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
