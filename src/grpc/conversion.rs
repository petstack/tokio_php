//! Type conversions between gRPC and internal types.

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;

use crate::types::{ParamList, ScriptRequest, ScriptResponse};

use super::proto::{ExecuteRequest, ExecuteResponse, ExecutionMetadata};

/// Convert gRPC ExecuteRequest to ScriptRequest.
pub fn grpc_to_script_request(
    req: &ExecuteRequest,
    document_root: &str,
) -> Result<ScriptRequest, String> {
    // Validate script path
    if req.script_path.is_empty() {
        return Err("script_path is required".to_string());
    }

    // Build full path
    let script_path = if req.script_path.starts_with('/') {
        format!("{}{}", document_root, req.script_path)
    } else {
        format!("{}/{}", document_root, req.script_path)
    };

    // Build query params from map
    let mut get_params: ParamList = Vec::new();
    for (k, v) in &req.query_params {
        get_params.push((Cow::Owned(k.clone()), Cow::Owned(v.clone())));
    }

    // Build POST params from form_data
    let mut post_params: ParamList = Vec::new();
    for (k, v) in &req.form_data {
        post_params.push((Cow::Owned(k.clone()), Cow::Owned(v.clone())));
    }

    // Build cookies
    let mut cookies: ParamList = Vec::new();
    for (k, v) in &req.cookies {
        cookies.push((Cow::Owned(k.clone()), Cow::Owned(v.clone())));
    }

    // Build server vars
    let mut server_vars: ParamList = Vec::new();
    for (k, v) in &req.server_vars {
        server_vars.push((Cow::Owned(k.clone()), Cow::Owned(v.clone())));
    }

    // Get method (default to GET)
    let method = if req.method.is_empty() {
        "GET"
    } else {
        &req.method
    };

    // Add standard server vars
    server_vars.push((
        Cow::Borrowed("REQUEST_METHOD"),
        Cow::Owned(method.to_uppercase()),
    ));
    server_vars.push((
        Cow::Borrowed("SCRIPT_FILENAME"),
        Cow::Owned(script_path.clone()),
    ));
    server_vars.push((
        Cow::Borrowed("DOCUMENT_ROOT"),
        Cow::Owned(document_root.to_string()),
    ));
    server_vars.push((
        Cow::Borrowed("REQUEST_URI"),
        Cow::Owned(format!("/{}", req.script_path)),
    ));

    if !req.content_type.is_empty() {
        server_vars.push((
            Cow::Borrowed("CONTENT_TYPE"),
            Cow::Owned(req.content_type.clone()),
        ));
    }

    // Use request body if provided
    let raw_body = if req.body.is_empty() {
        None
    } else {
        Some(req.body.clone())
    };

    // Parse timeout from options
    let timeout = req.options.as_ref().and_then(|opts| {
        if opts.timeout_ms > 0 {
            Some(Duration::from_millis(opts.timeout_ms as u64))
        } else {
            None
        }
    });

    // Get request time
    let received_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    Ok(ScriptRequest {
        script_path,
        get_params,
        post_params,
        cookies,
        server_vars,
        files: Vec::new(),
        raw_body,
        profile: req
            .options
            .as_ref()
            .map(|o| o.enable_profiling)
            .unwrap_or(false),
        timeout,
        received_at,
        request_id: String::new(), // Will be set by executor
        trace_id: req
            .options
            .as_ref()
            .map(|o| o.trace_parent.clone())
            .unwrap_or_default(),
        span_id: String::new(),
    })
}

/// Convert ScriptResponse to gRPC ExecuteResponse.
pub fn script_response_to_grpc(
    response: ScriptResponse,
    execution_time: Duration,
    request_id: String,
) -> ExecuteResponse {
    // Convert headers to HashMap
    let mut headers = HashMap::new();
    let mut status_code = 200i32;

    for (k, v) in response.headers {
        // Parse Status header if present
        if k.eq_ignore_ascii_case("Status") {
            if let Some(code) = v.split_whitespace().next() {
                status_code = code.parse().unwrap_or(200);
            }
        }
        headers.insert(k, v);
    }

    ExecuteResponse {
        status_code,
        headers,
        body: response.body.into_bytes(),
        metadata: Some(ExecutionMetadata {
            request_id,
            worker_id: 0,
            execution_time_us: execution_time.as_micros() as i64,
            queue_wait_us: 0,
            profile: None,
        }),
    }
}
