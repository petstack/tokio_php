/*
 * tokio_bridge - Shared library for Rust <-> PHP communication
 *
 * This library provides a shared TLS (Thread-Local Storage) context
 * that both Rust and PHP can access, solving the TLS isolation problem
 * between statically linked Rust code and dynamically loaded PHP extensions.
 *
 * Features:
 * - Shared request context accessible from both Rust and PHP
 * - Finish request state (fastcgi_finish_request analog)
 * - Heartbeat for request timeout extension
 * - Streaming support for SSE (Server-Sent Events)
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
#define TOKIO_BRIDGE_MAX_HEADERS 128
#define TOKIO_BRIDGE_MAX_HEADER_LEN 8192

/* ============================================================================
 * Callback types
 * ============================================================================ */

/**
 * Callback for heartbeat (request timeout extension)
 *
 * @param ctx  Opaque context pointer
 * @param secs Seconds to extend the deadline
 * @return     Non-zero on success, 0 on failure
 */
typedef int64_t (*tokio_heartbeat_callback_t)(void *ctx, uint64_t secs);

/**
 * Callback for finish request signal (streaming early response)
 *
 * Called when PHP invokes tokio_finish_request() to send response immediately
 * while continuing script execution in the background.
 *
 * @param ctx          Opaque context pointer (Rust channel sender)
 * @param body         Response body bytes (output before finish_request)
 * @param body_len     Length of body in bytes
 * @param headers      Serialized headers buffer (name\0value\0name\0value\0...)
 * @param headers_len  Total length of headers buffer
 * @param header_count Number of header pairs
 * @param status_code  HTTP response status code
 */
typedef void (*tokio_finish_callback_t)(
    void *ctx,
    const char *body,
    size_t body_len,
    const char *headers,
    size_t headers_len,
    int header_count,
    int status_code
);

/**
 * Callback for streaming chunks (SSE support)
 *
 * Called when PHP flushes output in streaming mode.
 * Each call sends a chunk of data to the client immediately.
 *
 * @param ctx       Opaque context pointer (Rust channel sender)
 * @param data      Chunk data bytes
 * @param data_len  Length of data in bytes
 */
typedef void (*tokio_stream_chunk_callback_t)(
    void *ctx,
    const char *data,
    size_t data_len
);

/* ============================================================================
 * Bridge context structure
 * ============================================================================ */

/**
 * Header entry for captured HTTP headers
 */
typedef struct tokio_bridge_header {
    char *name;
    char *value;
} tokio_bridge_header_t;

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
    int finished_header_count;  /* Header count at finish time */
    int response_code;

    /* Captured headers storage (shared between Rust SAPI and PHP) */
    tokio_bridge_header_t headers[TOKIO_BRIDGE_MAX_HEADERS];
    int header_count;

    /* Heartbeat callback and limits */
    void *heartbeat_ctx;
    uint64_t heartbeat_max_secs;
    tokio_heartbeat_callback_t heartbeat_callback;

    /* Finish request callback (streaming early response) */
    void *finish_ctx;
    tokio_finish_callback_t finish_callback;

    /* Streaming support (SSE) */
    int is_streaming;                               /* Streaming mode enabled */
    size_t stream_offset;                           /* Last read offset for polling */
    void *stream_ctx;                               /* Stream callback context */
    tokio_stream_chunk_callback_t stream_callback;  /* Chunk callback */

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

/**
 * Set the finish request callback.
 * Called from Rust before PHP execution to enable streaming early response.
 *
 * @param ctx      Opaque pointer to Rust channel sender (will be passed to callback)
 * @param callback Function to call when PHP calls tokio_finish_request()
 */
void tokio_bridge_set_finish_callback(void *ctx, tokio_finish_callback_t callback);

/**
 * Trigger the finish callback with response data.
 * Called from PHP's tokio_finish_request() function.
 *
 * This function:
 * 1. Marks the request as finished (idempotent)
 * 2. Invokes the Rust callback with body, headers, and status
 * 3. Allows PHP to continue executing in background
 *
 * @param body         Response body bytes
 * @param body_len     Length of body
 * @param headers      Serialized headers (name\0value\0...)
 * @param headers_len  Length of headers buffer
 * @param header_count Number of header pairs
 * @param status_code  HTTP response status code
 * @return             1 on success, 0 if no callback or already finished
 */
int tokio_bridge_trigger_finish(
    const char *body,
    size_t body_len,
    const char *headers,
    size_t headers_len,
    int header_count,
    int status_code
);

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

/* ============================================================================
 * Header Storage API (shared between Rust SAPI and PHP)
 * ============================================================================ */

/**
 * Add a captured header to the bridge context.
 * Called from Rust SAPI header_handler callback.
 *
 * @param name      Header name (e.g., "Content-Type")
 * @param name_len  Length of name
 * @param value     Header value (e.g., "text/html")
 * @param value_len Length of value
 * @param replace   If true, replace existing header with same name
 * @return          1 on success, 0 if storage is full or ctx is NULL
 */
int tokio_bridge_add_header(
    const char *name,
    size_t name_len,
    const char *value,
    size_t value_len,
    int replace
);

/**
 * Get the number of captured headers.
 */
int tokio_bridge_get_header_count(void);

/**
 * Get a captured header by index.
 *
 * @param index     Header index (0-based)
 * @param name      Output: pointer to header name (do not free)
 * @param value     Output: pointer to header value (do not free)
 * @return          1 on success, 0 if index out of range
 */
int tokio_bridge_get_header(int index, const char **name, const char **value);

/**
 * Clear all captured headers.
 * Called at request start.
 */
void tokio_bridge_clear_headers(void);

/* ============================================================================
 * Streaming API (SSE support)
 * ============================================================================ */

/**
 * Enable streaming mode for current request.
 * Called from Rust before PHP execution to enable SSE.
 *
 * @param ctx      Opaque pointer to Rust channel sender
 * @param callback Function to call for each chunk
 */
void tokio_bridge_enable_streaming(void *ctx, tokio_stream_chunk_callback_t callback);

/**
 * Check if streaming mode is enabled.
 *
 * @return 1 if streaming, 0 otherwise
 */
int tokio_bridge_is_streaming(void);

/**
 * Send a streaming chunk to the client.
 * Called from PHP when flush() is detected.
 *
 * @param data     Chunk data bytes
 * @param data_len Length of data
 * @return         1 on success, 0 if streaming not enabled
 */
int tokio_bridge_send_chunk(const char *data, size_t data_len);

/**
 * Get the current stream offset (for polling mode).
 * Returns the last read position in output buffer.
 */
size_t tokio_bridge_get_stream_offset(void);

/**
 * Set the stream offset (for polling mode).
 * Updates the last read position after reading new data.
 *
 * @param offset New offset value
 */
void tokio_bridge_set_stream_offset(size_t offset);

/**
 * End streaming mode.
 * Called when PHP script finishes or streaming is stopped.
 */
void tokio_bridge_end_stream(void);

#ifdef __cplusplus
}
#endif

#endif /* TOKIO_BRIDGE_H */
