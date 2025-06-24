use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::Response,
};
use serde_json::json;
use std::time::Instant;
use tracing::{debug, error, info_span, Instrument};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Clone)]
enum HandlerError {
    NoNodes,
    PoolError(String),
    RequestError(String),
    AllRequestsFailed(Vec<(String, String)>), // Vec of (node_url, error_message)
}

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandlerError::NoNodes => write!(f, "No nodes available"),
            HandlerError::PoolError(msg) => write!(f, "Pool error: {}", msg),
            HandlerError::RequestError(msg) => write!(f, "Request error: {}", msg),
            HandlerError::AllRequestsFailed(errors) => {
                write!(f, "All requests failed: [")?;
                for (i, (node, error)) in errors.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", node, error)?;
                }
                write!(f, "]")
            }
        }
    }
}

fn is_jsonrpc_error(body: &[u8]) -> bool {
    // Try to parse as JSON
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) {
        // Check if there's an "error" field
        return json.get("error").is_some();
    }

    // If we can't parse JSON, treat it as an error
    true
}

fn extract_jsonrpc_method(body: &[u8]) -> Option<String> {
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) {
        if let Some(method) = json.get("method").and_then(|m| m.as_str()) {
            return Some(method.to_string());
        }
    }
    None
}

async fn raw_http_request(
    node_url: (String, String, i64),
    path: &str,
    method: &str,
    headers: &HeaderMap,
    body: Option<&[u8]>,
) -> Result<Response, HandlerError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| HandlerError::RequestError(format!("{:#?}", e)))?;

    let (scheme, host, port) = &node_url;
    let url = format!("{}://{}:{}{}", scheme, host, port, path);

    // Use generic request method to support any HTTP verb
    let http_method = method
        .parse::<reqwest::Method>()
        .map_err(|e| HandlerError::RequestError(format!("Invalid method '{}': {}", method, e)))?;

    let mut request_builder = client.request(http_method, &url);

    // Forward body if present
    if let Some(body_bytes) = body {
        request_builder = request_builder.body(body_bytes.to_vec());
    }

    // Forward essential headers
    for (name, value) in headers.iter() {
        let header_name = name.as_str();
        let header_name_lc = header_name.to_ascii_lowercase();

        // Skip hop-by-hop headers and any body-related headers when we are **not** forwarding a body.
        let is_hop_by_hop = matches!(
            header_name_lc.as_str(),
            "host"
                | "connection"
                | "transfer-encoding"
                | "upgrade"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailers"
        );

        // If we are not forwarding a body (e.g. GET request) then forwarding `content-length` or
        // `content-type` with an absent body makes many Monero nodes hang waiting for bytes and
        // eventually close the connection.  This manifests as the time-outs we have observed.
        let is_body_header_without_body =
            body.is_none() && matches!(header_name_lc.as_str(), "content-length" | "content-type");

        if !is_hop_by_hop && !is_body_header_without_body {
            if let Ok(header_value) = std::str::from_utf8(value.as_bytes()) {
                request_builder = request_builder.header(header_name, header_value);
            }
        }
    }

    let response = request_builder
        .send()
        .await
        .map_err(|e| HandlerError::RequestError(format!("{:#?}", e)))?;

    // Convert to axum Response preserving everything
    let status = response.status();
    let response_headers = response.headers().clone();

    let body_bytes = response.bytes().await.map_err(|e| {
        HandlerError::RequestError(format!("Failed to read response body: {:#?}", e))
    })?;

    let mut axum_response = Response::new(Body::from(body_bytes));
    *axum_response.status_mut() =
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    // Copy response headers exactly
    for (name, value) in response_headers.iter() {
        if let (Ok(header_name), Ok(header_value)) = (
            axum::http::HeaderName::try_from(name.as_str()),
            axum::http::HeaderValue::try_from(value.as_bytes()),
        ) {
            axum_response
                .headers_mut()
                .insert(header_name, header_value);
        }
    }

    Ok(axum_response)
}

async fn record_success(state: &AppState, scheme: &str, host: &str, port: i64, latency_ms: f64) {
    let node_pool_guard = state.node_pool.read().await;
    if let Err(e) = node_pool_guard
        .record_success(scheme, host, port, latency_ms)
        .await
    {
        error!(
            "Failed to record success for {}://{}:{}: {}",
            scheme, host, port, e
        );
    }
}

async fn record_failure(state: &AppState, scheme: &str, host: &str, port: i64) {
    let node_pool_guard = state.node_pool.read().await;
    if let Err(e) = node_pool_guard.record_failure(scheme, host, port).await {
        error!(
            "Failed to record failure for {}://{}:{}: {}",
            scheme, host, port, e
        );
    }
}

async fn single_raw_request(
    state: &AppState,
    node_url: (String, String, i64),
    path: &str,
    method: &str,
    headers: &HeaderMap,
    body: Option<&[u8]>,
) -> Result<(Response, (String, String, i64), f64), HandlerError> {
    let (scheme, host, port) = &node_url;

    let start_time = Instant::now();

    match raw_http_request(node_url.clone(), path, method, headers, body).await {
        Ok(response) => {
            let elapsed = start_time.elapsed();
            let latency_ms = elapsed.as_millis() as f64;

            // Check HTTP status code - only 200 is success!
            if response.status().is_success() {
                // For JSON-RPC endpoints, also check for JSON-RPC errors
                if path == "/json_rpc" {
                    let (parts, body_stream) = response.into_parts();
                    let body_bytes = axum::body::to_bytes(body_stream, usize::MAX)
                        .await
                        .map_err(|e| HandlerError::RequestError(format!("{:#?}", e)))?;

                    if is_jsonrpc_error(&body_bytes) {
                        record_failure(state, scheme, host, *port).await;
                        return Err(HandlerError::RequestError("JSON-RPC error".to_string()));
                    }

                    // Reconstruct response with the body we consumed
                    let response = Response::from_parts(parts, Body::from(body_bytes));
                    record_success(state, scheme, host, *port, latency_ms).await;
                    Ok((response, node_url, latency_ms))
                } else {
                    // For non-JSON-RPC endpoints, HTTP success is enough
                    record_success(state, scheme, host, *port, latency_ms).await;
                    Ok((response, node_url, latency_ms))
                }
            } else {
                // Non-200 status codes are failures
                record_failure(state, scheme, host, *port).await;
                Err(HandlerError::RequestError(format!(
                    "HTTP {}",
                    response.status()
                )))
            }
        }
        Err(e) => {
            record_failure(state, scheme, host, *port).await;
            Err(e)
        }
    }
}

async fn race_requests(
    state: &AppState,
    path: &str,
    method: &str,
    headers: &HeaderMap,
    body: Option<&[u8]>,
) -> Result<Response, HandlerError> {
    // Extract JSON-RPC method for better logging
    let jsonrpc_method = if path == "/json_rpc" {
        if let Some(body_data) = body {
            extract_jsonrpc_method(body_data)
        } else {
            None
        }
    } else {
        None
    };
    const POOL_SIZE: usize = 20;
    let mut tried_nodes = std::collections::HashSet::new();
    let mut pool_index = 0;
    let mut collected_errors: Vec<(String, String)> = Vec::new();

    // Get the exclusive pool of 20 nodes once at the beginning
    let available_pool = {
        let node_pool_guard = state.node_pool.read().await;
        let reliable_nodes = node_pool_guard
            .get_top_reliable_nodes(POOL_SIZE)
            .await
            .map_err(|e| HandlerError::PoolError(e.to_string()))?;

        let pool: Vec<(String, String, i64)> = reliable_nodes
            .into_iter()
            .map(|node| (node.scheme, node.host, node.port))
            .collect();

        pool
    };

    if available_pool.is_empty() {
        return Err(HandlerError::NoNodes);
    }

    // Power of Two Choices within the exclusive pool
    while pool_index < available_pool.len() && tried_nodes.len() < POOL_SIZE {
        let mut node1_option = None;
        let mut node2_option = None;

        // Select first untried node from pool
        for (i, node) in available_pool.iter().enumerate().skip(pool_index) {
            if !tried_nodes.contains(node) {
                node1_option = Some(node.clone());
                pool_index = i + 1;
                break;
            }
        }

        // Select second untried node from pool (different from first)
        for node in available_pool.iter().skip(pool_index) {
            if !tried_nodes.contains(node) && Some(node) != node1_option.as_ref() {
                node2_option = Some(node.clone());
                break;
            }
        }

        // If we can't get any new nodes from the pool, we've exhausted our options
        if node1_option.is_none() && node2_option.is_none() {
            break;
        }

        // Store node URLs for error tracking before consuming them
        let current_nodes: Vec<(String, String, i64)> = [&node1_option, &node2_option]
            .iter()
            .filter_map(|opt| opt.as_ref())
            .cloned()
            .collect();

        let mut requests = Vec::new();

        if let Some(node1) = node1_option {
            tried_nodes.insert(node1.clone());
            requests.push(single_raw_request(
                state,
                node1.clone(),
                path,
                method,
                headers,
                body,
            ));
        }

        if let Some(node2) = node2_option {
            tried_nodes.insert(node2.clone());
            requests.push(single_raw_request(
                state,
                node2.clone(),
                path,
                method,
                headers,
                body,
            ));
        }

        if requests.is_empty() {
            break;
        }

        match &jsonrpc_method {
            Some(rpc_method) => debug!(
                "Racing {} requests to {} (JSON-RPC: {}): {} nodes (tried {} so far)",
                method,
                path,
                rpc_method,
                requests.len(),
                tried_nodes.len()
            ),
            None => debug!(
                "Racing {} requests to {}: {} nodes (tried {} so far)",
                method,
                path,
                requests.len(),
                tried_nodes.len()
            ),
        }

        // Handle the requests based on how many we have
        let result = match requests.len() {
            1 => {
                // Only one request
                requests.into_iter().next().unwrap().await
            }
            2 => {
                // Two requests - race them
                let mut iter = requests.into_iter();
                let req1 = iter.next().unwrap();
                let req2 = iter.next().unwrap();

                tokio::select! {
                    result1 = req1 => result1,
                    result2 = req2 => result2,
                }
            }
            _ => unreachable!("We only add 1 or 2 requests"),
        };

        match result {
            Ok((response, winning_node, latency_ms)) => {
                let (scheme, host, port) = &winning_node;
                let winning_node = format!("{}://{}:{}", scheme, host, port);

                match &jsonrpc_method {
                    Some(rpc_method) => {
                        debug!(
                        "{} response from {} ({}ms) - SUCCESS after trying {} nodes! JSON-RPC: {}",
                        method, winning_node, latency_ms, tried_nodes.len(), rpc_method
                    )
                    }
                    None => debug!(
                        "{} response from {} ({}ms) - SUCCESS after trying {} nodes!",
                        method,
                        winning_node,
                        latency_ms,
                        tried_nodes.len()
                    ),
                }
                record_success(state, scheme, host, *port, latency_ms).await;
                return Ok(response);
            }
            Err(e) => {
                // Since we don't know which specific node failed in the race,
                // record the error for all nodes in this batch
                for (scheme, host, port) in &current_nodes {
                    let node_display = format!("{}://{}:{}", scheme, host, port);
                    collected_errors.push((node_display, e.to_string()));
                }
                debug!(
                    "Request failed: {} - retrying with different nodes from pool...",
                    e
                );
                continue;
            }
        }
    }

    // Log detailed error information
    let detailed_errors: Vec<String> = collected_errors
        .iter()
        .map(|(node, error)| format!("{}: {}", node, error))
        .collect();

    match &jsonrpc_method {
        Some(rpc_method) => error!(
            "All {} requests failed after trying {} nodes (JSON-RPC: {}). Detailed errors:\n{}",
            method,
            tried_nodes.len(),
            rpc_method,
            detailed_errors.join("\n")
        ),
        None => error!(
            "All {} requests failed after trying {} nodes. Detailed errors:\n{}",
            method,
            tried_nodes.len(),
            detailed_errors.join("\n")
        ),
    }

    Err(HandlerError::AllRequestsFailed(collected_errors))
}

/// Forward a request to the node pool, returning either a successful response or a simple
/// `500` with text "All nodes failed".  Keeps the error handling logic in one place so the
/// public handlers stay readable.
async fn proxy_request(
    state: &AppState,
    path: &str,
    method: &str,
    headers: &HeaderMap,
    body: Option<&[u8]>,
) -> Response {
    match race_requests(state, path, method, headers, body).await {
        Ok(res) => res,
        Err(handler_error) => {
            let error_response = match &handler_error {
                HandlerError::AllRequestsFailed(node_errors) => {
                    json!({
                        "error": "All nodes failed",
                        "details": {
                            "type": "AllRequestsFailed",
                            "message": "All proxy requests to available nodes failed",
                            "node_errors": node_errors.iter().map(|(node, error)| {
                                json!({
                                    "node": node,
                                    "error": error
                                })
                            }).collect::<Vec<_>>(),
                            "total_nodes_tried": node_errors.len()
                        }
                    })
                }
                HandlerError::NoNodes => {
                    json!({
                        "error": "No nodes available",
                        "details": {
                            "type": "NoNodes",
                            "message": "No healthy nodes available in the pool"
                        }
                    })
                }
                HandlerError::PoolError(msg) => {
                    json!({
                        "error": "Pool error",
                        "details": {
                            "type": "PoolError",
                            "message": msg
                        }
                    })
                }
                HandlerError::RequestError(msg) => {
                    json!({
                        "error": "Request error",
                        "details": {
                            "type": "RequestError",
                            "message": msg
                        }
                    })
                }
            };

            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .body(Body::from(error_response.to_string()))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
    }
}

#[axum::debug_handler]
pub async fn simple_proxy_handler(
    State(state): State<AppState>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let body_size = body.len();
    let request_id = Uuid::new_v4();
    let path = uri.path().to_string();
    let method_str = method.to_string();
    let path_clone = path.clone();

    // Extract JSON-RPC method for tracing span
    let body_option = (!body.is_empty()).then_some(&body[..]);
    let jsonrpc_method = if path == "/json_rpc" {
        if let Some(body_data) = body_option {
            extract_jsonrpc_method(body_data)
        } else {
            None
        }
    } else {
        None
    };
    let jsonrpc_method_for_span = jsonrpc_method.as_deref().unwrap_or("N/A").to_string();

    async move {
        match &jsonrpc_method {
            Some(rpc_method) => debug!(
                "Proxying {} {} ({} bytes) - JSON-RPC method: {}",
                method, path, body_size, rpc_method
            ),
            None => debug!("Proxying {} {} ({} bytes)", method, path, body_size),
        }

        proxy_request(&state, &path, method.as_str(), &headers, body_option).await
    }
    .instrument(info_span!("proxy_request",
        request_id = %request_id,
        method = %method_str,
        path = %path_clone,
        body_size = body_size,
        jsonrpc_method = %jsonrpc_method_for_span
    ))
    .await
}

#[axum::debug_handler]
pub async fn simple_stats_handler(State(state): State<AppState>) -> Response {
    async move {
        let node_pool_guard = state.node_pool.read().await;

        match node_pool_guard.get_current_status().await {
            Ok(status) => {
                let stats_json = serde_json::json!({
                    "status": "healthy",
                    "total_node_count": status.total_node_count,
                    "healthy_node_count": status.healthy_node_count,
                    "successful_health_checks": status.successful_health_checks,
                    "unsuccessful_health_checks": status.unsuccessful_health_checks,
                    "top_reliable_nodes": status.top_reliable_nodes
                });

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(stats_json.to_string()))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
            Err(e) => {
                error!("Failed to get pool status: {}", e);
                let error_json = r#"{"status":"error","message":"Failed to get pool status"}"#;
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .body(Body::from(error_json))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
        }
    }
    .instrument(info_span!("stats_request"))
    .await
}
