//! Reverse-proxy HTTP server.
//!
//! Mounts a single catch-all route at `/p/{token}/*rest`. On each
//! request:
//!   1. Look up `token` in the [`ProxyRegistry`].
//!   2. Build an upstream URL: `<entry.upstream_url>/<rest>` plus the
//!      original query string.
//!   3. Forward method + body + headers; the registered headers are
//!      added LAST so they override any caller-supplied collision
//!      (typical use: caller can't accidentally suppress the injected
//!      `Authorization`).
//!   4. Stream the upstream response body back to the caller.
//!
//! No HTTPS termination on the listening side — this is a host-only
//! service intended to be reached from sibling containers via
//! `host.containers.internal`. TLS to the upstream is handled by
//! `reqwest` (rustls).

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
};
use futures::TryStreamExt;
use http_body_util::BodyExt;
use reqwest::Client;
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

use super::registry::SharedProxyRegistry;

#[derive(Clone)]
struct ProxyState {
    registry: SharedProxyRegistry,
    http: Client,
}

pub async fn serve_proxy(listen: String, registry: SharedProxyRegistry) -> std::io::Result<()> {
    // Single shared reqwest client — keeps connection pooling sane.
    let http = Client::builder()
        .pool_max_idle_per_host(8)
        .build()
        .expect("build reqwest client");
    let state = ProxyState { registry, http };

    let app = Router::new()
        .route("/p/:token", any(handle_root))
        .route("/p/:token/*rest", any(handle))
        .with_state(state);

    let listener = TcpListener::bind(&listen).await?;
    info!("nexal-gateway proxy listening on http://{}", listen);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_root(
    Path(token): Path<String>,
    State(state): State<ProxyState>,
    req: Request,
) -> Response {
    forward(token, String::new(), state, req).await
}

async fn handle(
    Path((token, rest)): Path<(String, String)>,
    State(state): State<ProxyState>,
    req: Request,
) -> Response {
    forward(token, rest, state, req).await
}

async fn forward(token: String, rest: String, state: ProxyState, req: Request) -> Response {
    let entry = match state.registry.lookup(&token).await {
        Some(e) => e,
        None => {
            debug!("proxy token not found: {token}");
            return (StatusCode::NOT_FOUND, "unknown proxy token").into_response();
        }
    };

    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();

    let target_url = match build_upstream_url(&entry.upstream_url, &rest, &uri) {
        Ok(u) => u,
        Err(err) => {
            warn!("proxy {} bad upstream url: {err}", entry.name);
            return (StatusCode::BAD_REQUEST, format!("bad upstream url: {err}")).into_response();
        }
    };

    // Buffer the request body. For v1 this is fine — typical API
    // payloads are small. Stream-pass-through can come later.
    let body_bytes = match collect_body(req).await {
        Ok(b) => b,
        Err(err) => {
            warn!("proxy {} body read failed: {err}", entry.name);
            return (StatusCode::BAD_REQUEST, format!("body read: {err}")).into_response();
        }
    };

    let mut req_builder = state
        .http
        .request(reqwest_method(&method), &target_url)
        .body(body_bytes);

    // Carry over caller headers, dropping a few hop-by-hop and
    // identity-bound ones reqwest sets itself.
    for (name, value) in headers.iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        if name == http::header::HOST {
            continue;
        }
        req_builder = req_builder.header(name.as_str(), value.as_bytes());
    }

    // Inject registered headers AFTER caller headers so they win.
    for (k, v) in &entry.headers {
        req_builder = req_builder.header(k.as_str(), v.as_str());
    }

    let upstream_resp = match req_builder.send().await {
        Ok(r) => r,
        Err(err) => {
            error!("proxy {} upstream send failed: {err}", entry.name);
            return (StatusCode::BAD_GATEWAY, format!("upstream: {err}")).into_response();
        }
    };

    let status =
        StatusCode::from_u16(upstream_resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut resp_headers = HeaderMap::new();
    for (name, value) in upstream_resp.headers() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            resp_headers.append(n, v);
        }
    }

    let stream = upstream_resp
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let body = Body::from_stream(stream);

    let mut resp = Response::builder()
        .status(status)
        .body(body)
        .unwrap_or_else(|err| {
            error!("proxy {} build response: {err}", entry.name);
            Response::new(Body::from("response build error"))
        });
    *resp.headers_mut() = resp_headers;
    resp
}

fn build_upstream_url(upstream: &str, rest: &str, original_uri: &Uri) -> Result<String, String> {
    let base = upstream.trim_end_matches('/');
    let path = if rest.is_empty() {
        String::new()
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        format!("/{rest}")
    };
    let mut out = format!("{base}{path}");
    if let Some(q) = original_uri.query() {
        out.push('?');
        out.push_str(q);
    }
    Ok(out)
}

async fn collect_body(req: Request) -> Result<Bytes, String> {
    req.into_body()
        .collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(|e| e.to_string())
}

fn reqwest_method(m: &Method) -> reqwest::Method {
    reqwest::Method::from_bytes(m.as_str().as_bytes()).unwrap_or(reqwest::Method::GET)
}

fn is_hop_by_hop(name: &str) -> bool {
    // RFC 7230 §6.1 hop-by-hop headers — must NOT be forwarded.
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "content-length" // reqwest sets this from body
    )
}

/// Convenience wrapper so `Arc<ProxyRegistry>` can be passed where
/// `SharedProxyRegistry` is expected.
pub fn shared(registry: super::registry::ProxyRegistry) -> SharedProxyRegistry {
    Arc::new(registry)
}
