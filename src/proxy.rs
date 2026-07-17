use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use base64::Engine;
use rand::Rng;
use regex::Regex;
use serde_json::{json, Value};

use crate::colors::{status_color, BOLD, CYAN, DIM, GREEN, RED, RESET, YELLOW};
use crate::routes::{add_route, find_route, get_routes, remove_route, Route};
use crate::state::AppState;

const MAX_LOGGED_BODY_CHARS: usize = 1000;
const MAX_CLIENT_LOG_CHARS: usize = 500;
const MAX_LOGGED_RES_BYTES: usize = 10 * 1024 * 1024;
/// Hard cap on buffered (post-decompression) response bytes; protects the
/// proxy from decompression bombs now that reqwest inflates bodies.
const MAX_BUFFERED_RES_BYTES: usize = 64 * 1024 * 1024;
const REQUEST_TIMEOUT_SECS: u64 = 120;
const BINARY_CONTENT_TYPES: [&str; 4] = ["image/", "video/", "audio/", "application/octet-stream"];

#[derive(Clone)]
struct ServerState {
    app: Arc<AppState>,
    client: reqwest::Client,
}

pub async fn run(app: Arc<AppState>) {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("reqwest client");

    let state = ServerState { app: app.clone(), client };

    let router = Router::new()
        .route("/health", get(health))
        .route("/log", any(receive_log))
        .route("/api/status", get(api_status))
        .route("/api/logs", get(api_logs))
        .route("/api/cmd", post(api_cmd))
        .fallback(any(proxy_handler))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], app.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            app.log(&format!("{RED}Porta {} em uso: {e}{RESET}", app.port));
            return;
        }
    };
    if let Err(e) = axum::serve(listener, router).await {
        app.log(&format!("{RED}Server error: {e}{RESET}"));
    }
}

fn timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn req_id() -> String {
    let mut rng = rand::thread_rng();
    (0..6)
        .map(|_| {
            let chars = b"abcdefghijklmnopqrstuvwxyz0123456789";
            chars[rng.gen_range(0..chars.len())] as char
        })
        .collect()
}

fn is_binary(content_type: &str) -> bool {
    if content_type.is_empty() {
        return false;
    }
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    BINARY_CONTENT_TYPES.iter().any(|t| ct.starts_with(t))
}

fn jwt_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b").unwrap()
    })
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    let payload_segment = token.split('.').nth(1)?;
    let mut b64 = payload_segment.replace('-', "+").replace('_', "/");
    while b64.len() % 4 != 0 {
        b64.push('=');
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn annotate_jwts(full_text: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut blocks = Vec::new();
    for m in jwt_regex().find_iter(full_text) {
        let token = m.as_str();
        if !seen.insert(token.to_string()) {
            continue;
        }
        if let Some(payload) = decode_jwt_payload(token) {
            let pretty = serde_json::to_string_pretty(&payload)
                .unwrap_or_default()
                .replace('\n', "\n  ");
            let prefix: String = token.chars().take(16).collect();
            blocks.push(format!(
                "  {DIM}[JWT {prefix}… decoded]{RESET}\n  {pretty}"
            ));
        }
    }
    blocks.join("\n")
}

/// Compressed or otherwise binary payloads can arrive under a text
/// content-type (e.g. gzip-encoded JSON); logging them raw injects
/// control bytes into the TUI, so sniff the content as a fallback.
fn looks_binary(buf: &[u8]) -> bool {
    const SNIFF_SAMPLE_BYTES: usize = 1024;
    // Treat as binary above 1-in-20 (5%) control bytes, ignoring the
    // whitespace and ANSI escape bytes legitimate text logs contain.
    const CTRL_RATIO_DENOMINATOR: usize = 20;
    let sample = &buf[..buf.len().min(SNIFF_SAMPLE_BYTES)];
    if sample.contains(&0) {
        return true;
    }
    let ctrl = sample
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x1b))
        .count();
    ctrl * CTRL_RATIO_DENOMINATOR > sample.len()
}

fn format_body(buf: &[u8], content_type: &str) -> String {
    if buf.is_empty() {
        return String::new();
    }
    if is_binary(content_type) || looks_binary(buf) {
        return format!("  [binary {} bytes]", format_thousands(buf.len()));
    }
    let full_str = String::from_utf8_lossy(buf).to_string();
    let pretty = serde_json::from_str::<Value>(&full_str)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| full_str.clone());

    let truncated = if pretty.chars().count() > MAX_LOGGED_BODY_CHARS {
        let t: String = pretty.chars().take(MAX_LOGGED_BODY_CHARS).collect();
        format!("{t}…")
    } else {
        pretty
    };
    let jwt = annotate_jwts(&full_str);
    if jwt.is_empty() {
        truncated
    } else {
        format!("{truncated}\n{jwt}")
    }
}

fn format_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn log_request(
    app: &AppState,
    req_id: &str,
    method: &str,
    url: &str,
    target_full: &str,
    content_type: &str,
    body: &[u8],
    color: &str,
    route_label: &str,
) {
    if !app.filters.lock().unwrap().should_show(route_label) {
        return;
    }
    let ts = timestamp();
    app.log(&format!("{DIM}[{ts}]{RESET} {BOLD}{req_id}{RESET}"));
    app.log(&format!("  {color}{method}{RESET} {url}"));
    app.log(&format!("{DIM}  → {target_full}{RESET}"));
    if !body.is_empty() {
        let ct = content_type.split(';').next().unwrap_or("");
        app.log(&format!("{DIM}  Req Body ({ct}):{RESET}"));
        let formatted = format_body(body, content_type).replace('\n', "\n  ");
        app.log_multiline(&format!("  {formatted}"));
    }
}

#[allow(clippy::too_many_arguments)]
fn log_response(
    app: &AppState,
    status_code: u16,
    body: &[u8],
    content_type: &str,
    duration_ms: u128,
    color: &str,
    route_label: &str,
) {
    if !app.filters.lock().unwrap().should_show(route_label) {
        return;
    }
    let size_label = if body.len() > MAX_LOGGED_RES_BYTES {
        format!(" [{:.1}MB, omitted]", body.len() as f64 / 1024.0 / 1024.0)
    } else {
        String::new()
    };
    app.log(&format!(
        "  {BOLD}Response:{RESET} {color}{status_code}{RESET} {DIM}{duration_ms}ms{RESET}{size_label}"
    ));
    if !body.is_empty() && body.len() <= MAX_LOGGED_RES_BYTES {
        let ct = content_type.split(';').next().unwrap_or("");
        app.log(&format!("{DIM}  Res Body ({ct}):{RESET}"));
        let formatted = format_body(body, content_type).replace('\n', "\n  ");
        app.log_multiline(&format!("  {formatted}"));
    }
    app.log("");
}

fn json_error(status: StatusCode, value: Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}

async fn health(State(state): State<ServerState>) -> Json<Value> {
    Json(json!({ "status": "ok", "uptime": state.app.uptime_secs() }))
}

async fn receive_log(State(state): State<ServerState>, body: axum::body::Bytes) -> Json<Value> {
    let raw = String::from_utf8_lossy(&body);
    let truncated: String = raw.chars().take(MAX_CLIENT_LOG_CHARS).collect();
    let ts = timestamp();
    if state.app.filters.lock().unwrap().should_show("LOG") {
        state
            .app
            .log(&format!("{DIM}[{ts}]{RESET} {CYAN}LOG{RESET} {truncated}"));
    }
    Json(json!({ "received": true }))
}

async fn api_status(State(state): State<ServerState>) -> Response {
    let app = &state.app;
    let routes: Vec<Value> = get_routes()
        .iter()
        .map(|r| json!({ "prefix": r.prefix, "target": r.target, "label": r.label }))
        .collect();
    let body = json!({
        "uptime": app.uptime_secs(),
        "port": app.port,
        "logFile": app.logger.get_session_file().map(|p| p.display().to_string()),
        "filters": app.filters.lock().unwrap().as_json(),
        "routes": routes,
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string_pretty(&body).unwrap()))
        .unwrap()
}

async fn api_logs(State(state): State<ServerState>) -> Response {
    let app = state.app.clone();
    let content = tokio::task::spawn_blocking(move || app.logger.read_tail(51))
        .await
        .unwrap_or_default();
    let content = if content.is_empty() {
        "(sem logs ainda)".to_string()
    } else {
        content
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Body::from(content))
        .unwrap()
}

async fn api_cmd(State(state): State<ServerState>, body: axum::body::Bytes) -> Response {
    let app = &state.app;
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Invalid JSON body" })),
    };
    let Some(cmd) = parsed.get("cmd").and_then(|c| c.as_str()) else {
        return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Missing \"cmd\" field" }));
    };

    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Missing \"cmd\" field" }));
    }
    let action = parts[0].to_lowercase();

    if action == "add" && parts.len() >= 3 {
        let prefix = parts[1];
        let target = parts[2];
        let label = if parts.len() > 3 {
            parts[3..].join(" ")
        } else {
            prefix.trim_start_matches('/').to_uppercase()
        };
        return match add_route(prefix, target, &label) {
            Ok(route) => {
                app.filters.lock().unwrap().rebuild();
                app.log(&format!(
                    "{GREEN}Route added:{RESET} {} → {}",
                    route.prefix, route.target
                ));
                json_error(StatusCode::OK, json!({ "ok": true, "route": route }))
            }
            Err(e) => json_error(StatusCode::BAD_REQUEST, json!({ "ok": false, "error": e })),
        };
    }

    if action == "rm" && parts.len() >= 2 {
        return match remove_route(parts[1]) {
            Ok(()) => {
                app.filters.lock().unwrap().rebuild();
                app.log(&format!("{YELLOW}Route removed:{RESET} {}", parts[1]));
                json_error(StatusCode::OK, json!({ "ok": true }))
            }
            Err(e) => json_error(StatusCode::BAD_REQUEST, json!({ "ok": false, "error": e })),
        };
    }

    if action == "logmode" && parts.len() >= 2 {
        if parts[1] == "day" || parts[1] == "session" {
            app.logger.set_mode(parts[1]);
        }
        return json_error(
            StatusCode::OK,
            json!({
                "mode": app.logger.get_mode(),
                "file": app.logger.get_session_file().map(|p| p.display().to_string()),
            }),
        );
    }

    app.filters.lock().unwrap().handle_command(cmd);
    let filters = app.filters.lock().unwrap().as_json();
    json_error(StatusCode::OK, json!({ "ok": true, "cmd": cmd, "filters": filters }))
}

async fn proxy_handler(State(state): State<ServerState>, req: Request) -> Response {
    let app = &state.app;
    app.record_request();
    let start = std::time::Instant::now();
    let id = req_id();

    let path = req.uri().path().to_string();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let full_url = format!("{path}{query}");

    let Some(route) = find_route(&path) else {
        let ts = timestamp();
        app.log(&format!(
            "{DIM}[{ts}]{RESET} {BOLD}{id}{RESET} ⚠  NO ROUTE: {} {full_url}",
            req.method()
        ));
        let routes: Vec<String> = get_routes()
            .iter()
            .map(|r| format!("{} → {}", r.prefix, r.label))
            .collect();
        return json_error(
            StatusCode::NOT_FOUND,
            json!({ "error": "No route", "url": full_url, "routes": routes }),
        );
    };

    forward_request(&state, req, &route, &id, start).await
}

async fn read_body_capped(mut res: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        if buf.len() + chunk.len() > cap {
            return Err(format!(
                "response body exceeded {}MB buffer cap",
                cap / 1024 / 1024
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

async fn forward_request(
    state: &ServerState,
    req: Request,
    route: &Route,
    id: &str,
    start: std::time::Instant,
) -> Response {
    let app = &state.app;
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let original_url = format!("{path}{query}");

    let stripped = {
        let s = &path[route.prefix.len()..];
        if s.is_empty() { "/" } else { s }
    };
    let target_path = format!("{stripped}{query}");
    let target_full = format!("{}{}", route.target, target_path);
    let route_color = app.colors.get(&route.label);

    let (parts, body) = req.into_parts();
    let request_body = to_bytes(body, usize::MAX).await.unwrap_or_default();

    let content_type = parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    log_request(
        app,
        id,
        method.as_str(),
        &original_url,
        &target_full,
        &content_type,
        &request_body,
        route_color,
        &route.label,
    );

    let mut headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        let n = name.as_str().to_ascii_lowercase();
        // accept-encoding is dropped so reqwest negotiates compression itself
        // and transparently decompresses, keeping bodies loggable as text.
        if n == "host"
            || n == "connection"
            || n == "keep-alive"
            || n == "transfer-encoding"
            || n == "content-length"
            || n == "accept-encoding"
        {
            continue;
        }
        headers.insert(name.clone(), value.clone());
    }

    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Invalid method" })),
    };

    let result = state
        .client
        .request(reqwest_method, &target_full)
        .headers(headers)
        .body(request_body.to_vec())
        .send()
        .await;

    match result {
        Ok(proxy_res) => {
            let status = proxy_res.status();
            let res_headers = proxy_res.headers().clone();
            let res_content_type = res_headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            match read_body_capped(proxy_res, MAX_BUFFERED_RES_BYTES).await {
                Ok(res_body) => {
                    let duration = start.elapsed().as_millis();
                    log_response(
                        app,
                        status.as_u16(),
                        &res_body,
                        &res_content_type,
                        duration,
                        status_color(status.as_u16()),
                        &route.label,
                    );

                    let mut builder = Response::builder().status(status.as_u16());
                    for (name, value) in res_headers.iter() {
                        let n = name.as_str().to_ascii_lowercase();
                        // content-encoding is intentionally forwarded: reqwest
                        // already removed it whenever it decompressed the body,
                        // so if still present the body is passing through
                        // untouched (e.g. an encoding reqwest can't decode).
                        if n == "transfer-encoding" || n == "content-length" || n == "connection" {
                            continue;
                        }
                        builder = builder.header(name, value);
                    }
                    builder.body(Body::from(res_body)).unwrap_or_else(|_| {
                        json_error(
                            StatusCode::BAD_GATEWAY,
                            json!({ "error": "Failed to build response" }),
                        )
                    })
                }
                Err(e) => {
                    let duration = start.elapsed().as_millis();
                    app.log(&format!(
                        "  {RED}ERROR:{RESET} {e} {DIM}{duration}ms{RESET}\n"
                    ));
                    json_error(StatusCode::BAD_GATEWAY, json!({ "error": e.to_string() }))
                }
            }
        }
        Err(e) => {
            let duration = start.elapsed().as_millis();
            if e.is_timeout() {
                app.log(&format!("  {RED}TIMEOUT{RESET} {DIM}{duration}ms{RESET}\n"));
                json_error(
                    StatusCode::GATEWAY_TIMEOUT,
                    json!({ "error": "Timeout (120s)" }),
                )
            } else {
                app.log(&format!(
                    "  {RED}ERROR:{RESET} {e} {DIM}{duration}ms{RESET}\n"
                ));
                json_error(StatusCode::BAD_GATEWAY, json!({ "error": e.to_string() }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_binary_detects_gzip_header() {
        // gzip magic + zeroed MTIME field trips the NUL check
        let gz = [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03];
        assert!(looks_binary(&gz));
    }

    #[test]
    fn looks_binary_allows_text_and_ansi() {
        assert!(!looks_binary(b"{\"ok\":true}\n"));
        assert!(!looks_binary(b"\x1b[32mGET\x1b[0m /path\r\n\tbody"));
        assert!(!looks_binary(b""));
    }

    #[test]
    fn looks_binary_detects_control_dense_payloads() {
        let mut buf = vec![b'a'; 90];
        buf.extend(std::iter::repeat(0x07).take(10)); // >5% control bytes
        assert!(looks_binary(&buf));
    }

    #[test]
    fn format_body_labels_binary_under_text_content_type() {
        let gz = [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(format_body(&gz, "application/json").contains("[binary"));
    }

    /// Network test (run with `cargo test -- --ignored`): a HEAD response
    /// from a gzip-serving upstream carries content-encoding with an empty
    /// body; ensures reqwest's decoder handles the empty stream instead of
    /// erroring, which would turn HEAD requests into 502s.
    #[tokio::test]
    #[ignore]
    async fn head_against_gzip_upstream_reads_empty_body() {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let res = client
            .head("https://api.github.com/")
            .header("user-agent", "debugproxy-test")
            .send()
            .await
            .unwrap();
        let body = read_body_capped(res, MAX_BUFFERED_RES_BYTES).await.unwrap();
        assert!(body.is_empty());
    }
}
