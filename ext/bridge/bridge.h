/*
 * tokio_bridge - Shared library for Rust <-> PHP communication
 *
 * This library provides a shared TLS (Thread-Local Storage) context
 * that both Rust and PHP can access, solving the TLS isolation problem
 * between statically linked Rust code and dynamically loaded PHP extensions.
 *
 * Features:
 * - Shared request context accessible from both Rust and PHP
 * - Early Hints (HTTP 103) support via callback mechanism
 * - Finish request state (fastcgi_finish_request analog)
 * - Heartbeat for request timeout extension
 */

#ifndef TOKIO_BRIDGE_H
#define TOKIO_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>
#include <stddef.h>

/* ============================================================================
 * Version and limits
 * ============================================================================ */

#define TOKIO_BRIDGE_VERSION "0.1.0"
#define TOKIO_BRIDGE_MAX_EARLY_HINTS 16
#define TOKIO_BRIDGE_MAX_HINT_LEN 512

/* ============================================================================
 * Callback types
 * ============================================================================ */

/**
 * Callback for sending Early Hints (HTTP 103)
 *
 * @param ctx     Opaque context pointer (Rust channel sender)
 * @param headers Array of header strings (e.g., "Link: </style.css>; rel=preload")
 * @param count   Number of headers in the array
 */
typedef void (*tokio_early_hints_callback_t)(
    void *ctx,
    const char **headers,
    size_t count
);

/**
 * Callback for heartbeat (request timeout extension)
 *
 * @param ctx  Opaque context pointer
 * @param secs Seconds to extend the deadline
 * @return     Non-zero on success, 0 on failure
 */
typedef int64_t (*tokio_heartbeat_callback_t)(void *ctx, uint64_t secs);

/* ============================================================================
 * Bridge context structure
 * ============================================================================ */

/**
 * Thread-local request context shared between Rust and PHP
 */
typedef struct tokio_bridge_ctx {
    /* Request identification */
    uint64_t request_id;
    uint64_t worker_id;

    /* Finish request state (fastcgi_finish_request analog) */
    int is_finished;
    size_t output_offset;
    int header_count;
    int response_code;

    /* Early Hints callback and context */
    void *hints_ctx;
    tokio_early_hints_callback_t hints_callback;

    /* Heartbeat callback and limits */
    void *heartbeat_ctx;
    uint64_t heartbeat_max_secs;
    tokio_heartbeat_callback_t heartbeat_callback;

} tokio_bridge_ctx_t;

/* ============================================================================
 * Context lifecycle
 * ============================================================================ */

/**
 * Get the current thread's bridge context.
 * Returns NULL if no context has been initialized for this thread.
 */
tokio_bridge_ctx_t* tokio_bridge_get_ctx(void);

/**
 * Initialize a new bridge context for the current thread.
 * Must be called from the worker thread before PHP execution.
 *
 * @param request_id Unique request identifier
 * @param worker_id  Worker thread identifier
 */
void tokio_bridge_init_ctx(uint64_t request_id, uint64_t worker_id);

/**
 * Destroy the current thread's bridge context.
 * Should be called after PHP execution completes.
 */
void tokio_bridge_destroy_ctx(void);

/* ============================================================================
 * Early Hints API
 * ============================================================================ */

/**
 * Set the Early Hints callback and context.
 * Called from Rust before PHP execution to enable early hints.
 *
 * @param ctx      Opaque pointer to Rust channel sender (will be passed to callback)
 * @param callback Function to call when PHP sends early hints
 */
void tokio_bridge_set_hints_callback(void *ctx, tokio_early_hints_callback_t callback);

/**
 * Send Early Hints from PHP to client.
 * Called from PHP's tokio_early_hints() function.
 *
 * @param headers Array of header strings
 * @param count   Number of headers
 * @return        1 on success, 0 if no callback is set
 */
int tokio_bridge_send_early_hints(const char **headers, size_t count);

/* ============================================================================
 * Finish Request API
 * ============================================================================ */

/**
 * Mark the request as finished (response ready to send).
 * Called from PHP's tokio_finish_request() function.
 *
 * @param offset       Byte offset in output where response ends
 * @param header_count Number of headers set before finish
 * @param response_code HTTP response code
 */
void tokio_bridge_mark_finished(size_t offset, int header_count, int response_code);

/**
 * Check if the request has been marked as finished.
 *
 * @return 1 if finished, 0 otherwise
 */
int tokio_bridge_is_finished(void);

/**
 * Get the output byte offset where response should be truncated.
 */
size_t tokio_bridge_get_finished_offset(void);

/**
 * Get the number of headers at finish time.
 */
int tokio_bridge_get_finished_header_count(void);

/**
 * Get the response code at finish time.
 */
int tokio_bridge_get_finished_response_code(void);

/* ============================================================================
 * Heartbeat API
 * ============================================================================ */

/**
 * Set the heartbeat callback and limits.
 * Called from Rust before PHP execution.
 *
 * @param ctx      Opaque pointer to heartbeat context
 * @param max_secs Maximum seconds that can be extended
 * @param callback Function to call when PHP sends heartbeat
 */
void tokio_bridge_set_heartbeat(
    void *ctx,
    uint64_t max_secs,
    tokio_heartbeat_callback_t callback
);

/**
 * Send a heartbeat to extend the request timeout.
 * Called from PHP's tokio_request_heartbeat() function.
 *
 * @param secs Seconds to extend the deadline
 * @return     1 on success, 0 on failure or if exceeds max_secs
 */
int tokio_bridge_send_heartbeat(uint64_t secs);

/**
 * Get the maximum heartbeat extension in seconds.
 */
uint64_t tokio_bridge_get_heartbeat_max(void);

#ifdef __cplusplus
}
#endif

#endif /* TOKIO_BRIDGE_H */
