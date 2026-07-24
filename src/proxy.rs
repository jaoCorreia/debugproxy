use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use std::net::SocketAddr;
use base64::Engine;
use rand::Rng;
use regex::Regex;
use serde_json::{json, Value};

use crate::colors::{status_color, BOLD, CYAN, DIM, GREEN, RED, RESET, YELLOW};
use crate::routes::{add_route, find_route, get_routes, remove_route, Route};
use crate::logcat::{logcat_status, spawn_logcat, stop_logcat};
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
        .route("/api/ai/ask", post(api_ai_ask))
        .route("/api/ai/status", get(api_ai_status))
        .route("/api/ai/forward", post(api_ai_forward))
        .route("/api/ai/report", post(api_ai_report_handler))
        .route("/api/logcat/status", get(api_logcat_status))
        .route("/api/logcat/start", post(api_logcat_start))
        .route("/api/logcat/stop", post(api_logcat_stop))
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
    if let Err(e) = axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await {
        app.log(&format!("{RED}Server error: {e}{RESET}"));
    }
}

pub fn timestamp() -> String {
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



/// Checks AI endpoint access: token bearer auth or localhost-only.
/// Returns Ok(()) if allowed, Err(StatusCode) if denied.
fn check_ai_access(
    headers: &HeaderMap,
    ai_api_token: &Option<String>,
    remote_addr: Option<&str>,
) -> Result<(), StatusCode> {
    if let Some(expected_token) = ai_api_token {
        // Token mode: require Bearer token
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if auth.starts_with("Bearer ") && auth[7..] == *expected_token {
            Ok(())
        } else {
            Err(StatusCode::UNAUTHORIZED)
        }
    } else {
        // No token configured: localhost only
        match remote_addr {
            Some(addr) => {
                if addr.starts_with("127.") || addr.starts_with("::1") || addr == "localhost" {
                    Ok(())
                } else {
                    Err(StatusCode::FORBIDDEN)
                }
            }
            None => Ok(()), // If we can't determine, allow (local TUI access)
        }
    }
}

/// Checks rate limit for AI endpoints. Returns Ok(()) if allowed, Err(429) if exceeded.
fn check_ai_rate_limit(
    state: &AppState,
    remote_addr: Option<&str>,
) -> Result<(), StatusCode> {
    let key = remote_addr.unwrap_or("unknown");
    if state.ai_rate_limiter.check(key) {
        Ok(())
    } else {
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
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
    let transfers: Vec<Value> = app
        .transfer_tracker
        .lock()
        .unwrap()
        .snapshot()
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "method": t.method,
                "path": t.path,
                "route_label": t.route_label,
                "status": t.status.unwrap_or(0),
                "duration_ms": t.duration_ms,
                "size": t.size,
            })
        })
        .collect();
    let body = json!({
        "uptime": app.uptime_secs(),
        "port": app.port,
        "logFile": app.logger.get_session_file().map(|p| p.display().to_string()),
        "filters": app.filters.lock().unwrap().as_json(),
        "routes": routes,
        "transfers": transfers,
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

    if action == "monitor" {
        let was_on = app.monitoring_enabled.load(std::sync::atomic::Ordering::Relaxed);
        app.monitoring_enabled.store(!was_on, std::sync::atomic::Ordering::Relaxed);
        if !was_on {
            app.log("\x1b[32mMonitoring ON\x1b[0m");
        } else {
            app.log("\x1b[2mMonitoring OFF\x1b[0m");
            app.transfer_tracker.lock().unwrap().transfers.clear();
        }
        return json_error(StatusCode::OK, json!({ "ok": true, "monitoring": !was_on }));
    }

    if action == "ultra" {
        if parts.len() >= 2 && parts[1] == "off" {
            app.ultra_mode.store(false, std::sync::atomic::Ordering::Relaxed);
            app.ultra_routes.lock().unwrap().clear();
            app.log("\x1b[2mUltra mode OFF\x1b[0m");
            return json_error(StatusCode::OK, json!({ "ok": true, "ultra": false }));
        }
        let routes: std::collections::HashSet<String> = if parts.len() >= 2 {
            parts[1..].iter().map(|s| s.to_string()).collect()
        } else {
            std::collections::HashSet::new()
        };
        app.ultra_routes.lock().unwrap().clone_from(&routes);
        app.ultra_mode.store(true, std::sync::atomic::Ordering::Relaxed);
        let list: Vec<_> = routes.iter().collect();
        app.log(&format!("\x1b[36mUltra mode ON{}\x1b[0m",
            if list.is_empty() { String::new() } else { format!(" [{}]", list.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) }
        ));
        return json_error(StatusCode::OK, json!({ "ok": true, "ultra": true, "routes": list }));
    }

    app.filters.lock().unwrap().handle_command(cmd);
    let filters = app.filters.lock().unwrap().as_json();
    json_error(StatusCode::OK, json!({ "ok": true, "cmd": cmd, "filters": filters }))
}

async fn api_ai_status(State(state): State<ServerState>) -> Response {
    let app = &state.app;
    let body = match &app.ai_client {
        Some(ai) => json!({
            "configured": true,
            "model": ai.model(),
            "endpoint": ai.endpoint(),
            "forwardingEnabled": ai.forwarding_enabled(),
            "hasApiKey": ai.is_configured(),
            "lastResponse": ai.last_response_text(),
        }),
        None => json!({
            "configured": false,
            "error": "AI not configured. Set DEEPSEEK_API_KEY env var and ai section in config.json"
        }),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string_pretty(&body).unwrap()))
        .unwrap()
}

async fn api_ai_ask(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let app = &state.app;
    let remote = addr.to_string();
    if let Err(status) = check_ai_access(&headers, &app.ai_api_token, Some(&remote)) {
        return json_error(status, json!({ "error": "Access denied" }));
    }
    if let Err(status) = check_ai_rate_limit(app, Some(&remote)) {
        return json_error(status, json!({ "error": "Rate limit exceeded (10 req/min)" }));
    }
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Invalid JSON body" })),
    };

    let question = parsed.get("question").and_then(|q| q.as_str()).unwrap_or("");
    if question.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Missing \"question\" field" }));
    }

    let Some(ai) = &app.ai_client else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, json!({ "error": "AI not configured" }));
    };

    let context = parsed
        .get("context")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            app.logger
                .read_tail(ai.max_context_lines())
        });

    match ai.chat(&context, question).await {
        Ok(response) => {
            let resp = json!({
                "ok": true,
                "response": response.text,
                "toolCalls": response.tool_calls.iter().map(|tc| json!({
                    "name": tc.name,
                    "arguments": tc.arguments,
                })).collect::<Vec<_>>(),
            });

            for tc in &response.tool_calls {
                execute_ai_tool(app, tc);
            }

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string_pretty(&resp).unwrap()))
                .unwrap()
        }
        Err(e) => json_error(StatusCode::BAD_GATEWAY, json!({ "error": e })),
    }
}

async fn api_ai_forward(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let app = &state.app;
    let remote = addr.to_string();
    if let Err(status) = check_ai_access(&headers, &app.ai_api_token, Some(&remote)) {
        return json_error(status, json!({ "error": "Access denied" }));
    }
    if let Err(status) = check_ai_rate_limit(app, Some(&remote)) {
        return json_error(status, json!({ "error": "Rate limit exceeded (10 req/min)" }));
    }
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Invalid JSON body" })),
    };

    let message = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("");
    if message.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Missing \"message\" field" }));
    }

    let urgency = parsed.get("urgency").and_then(|u| u.as_str()).unwrap_or("medium");

    let Some(ai) = &app.ai_client else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, json!({ "error": "AI not configured" }));
    };

    if !ai.forwarding_enabled() {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, json!({ "error": "Forwarding not configured" }));
    }

    match ai.forward(message, urgency).await {
        Ok(()) => {
            app.log(&format!("\x1b[35m>> AI Forward [{urgency}]: {message}\x1b[0m"));
            json_error(StatusCode::OK, json!({ "ok": true, "forwarded": true }))
        }
        Err(e) => json_error(StatusCode::BAD_GATEWAY, json!({ "error": e })),
    }
}

fn build_report_context(app: &crate::state::AppState) -> String {
    use std::fmt::Write;
    let mut report = String::new();
    let _ = writeln!(report, "=== Proxy Status ===");
    let _ = writeln!(report, "Port: {}", app.port);
    let _ = writeln!(report, "Uptime: {}s", app.uptime_secs());
    let _ = writeln!(report, "Total Requests: {}", app.request_total());

    let _ = writeln!(report, "=== Routes ===");
    for r in &get_routes() {
        let _ = writeln!(report, "  {} → {}", r.prefix, r.target);
    }

    let _ = writeln!(report, "=== Filters ===");
    let filters = app.filters.lock().unwrap();
    for (label, enabled) in &filters.state {
        let _ = writeln!(report, "  {} {}", if *enabled { "ON" } else { "OFF" }, label);
    }
    drop(filters);

    let mon_on = app.monitoring_enabled.load(std::sync::atomic::Ordering::Relaxed);
    let _ = writeln!(report, "=== Monitoring: {} ===", if mon_on { "ON" } else { "OFF" });
    if mon_on {
        let transfers = app.transfer_tracker.lock().unwrap().snapshot();
        for t in transfers.iter().take(20) {
            let status = t.status.map(|s| s.to_string()).unwrap_or_else(|| "???".to_string());
            let _ = writeln!(report, "  {} {} {}", t.method, t.path, status);
        }
    }

    let _ = writeln!(report, "=== Recent Logs ===");
    let _ = writeln!(report, "{}", app.logger.read_tail(200));

    report
}

async fn api_ai_report_handler(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let app = &state.app;
    let remote = addr.to_string();
    if let Err(status) = check_ai_access(&headers, &app.ai_api_token, Some(&remote)) {
        return json_error(status, json!({ "error": "Access denied" }));
    }
    if let Err(status) = check_ai_rate_limit(app, Some(&remote)) {
        return json_error(status, json!({ "error": "Rate limit exceeded (10 req/min)" }));
    }
    let Some(ai) = &app.ai_client else {
        return json_error(StatusCode::SERVICE_UNAVAILABLE, json!({ "error": "AI not configured" }));
    };

    let extra_context = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("context").and_then(|c| c.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();

    let mut report = build_report_context(app);
    if !extra_context.is_empty() {
        use std::fmt::Write;
        let _ = writeln!(report, "\n=== Additional Context ===\n{extra_context}");
    }

    let prompt = "Generate a structured diagnostic report covering: overall health, error patterns, performance, recommendations, risk assessment. Concise and actionable.";

    match ai.chat(&report, prompt).await {
        Ok(response) => {
            app.log("\x1b[35m== AI Report generated\x1b[0m");
            json_error(StatusCode::OK, json!({
                "ok": true,
                "report": response.text,
                "toolCalls": response.tool_calls.iter().map(|tc| json!({
                    "name": tc.name,
                    "arguments": tc.arguments,
                })).collect::<Vec<_>>(),
            }))
        }
        Err(e) => json_error(StatusCode::BAD_GATEWAY, json!({ "error": e })),
    }
}

fn execute_ai_tool(app: &AppState, tc: &crate::ai::ToolCall) {
    match tc.name.as_str() {
        "toggle_service" => {
            if let Some(action) = tc.arguments.get("action").and_then(|v| v.as_str()) {
                let action = action.to_string();
                app.filters.lock().unwrap().handle_command(&action);
                app.log(&format!("\x1b[35m+ AI: toggle {action}\x1b[0m"));
            }
        }
        "add_route" => {
            let prefix = tc.arguments.get("prefix").and_then(|v| v.as_str()).unwrap_or("");
            let target = tc.arguments.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let label = tc.arguments.get("label").and_then(|v| v.as_str()).unwrap_or("");
            if !prefix.is_empty() && !target.is_empty() {
                match crate::routes::add_route(prefix, target, label) {
                    Ok(route) => {
                        app.filters.lock().unwrap().rebuild();
                        app.log(&format!("\x1b[35m+ AI: + Route {} → {}\x1b[0m", route.prefix, route.target));
                    }
                    Err(e) => app.log(&format!("\x1b[31mAI tool error: {e}\x1b[0m")),
                }
            }
        }
        "remove_route" => {
            if let Some(prefix) = tc.arguments.get("prefix").and_then(|v| v.as_str()) {
                match crate::routes::remove_route(prefix) {
                    Ok(()) => {
                        app.filters.lock().unwrap().rebuild();
                        app.log(&format!("\x1b[35m+ AI: - Route {prefix}\x1b[0m"));
                    }
                    Err(e) => app.log(&format!("\x1b[31mAI tool error: {e}\x1b[0m")),
                }
            }
        }
        "enable_monitoring" => {
            if let Some(enable) = tc.arguments.get("enable").and_then(|v| v.as_bool()) {
                app.monitoring_enabled.store(enable, std::sync::atomic::Ordering::Relaxed);
                app.log(&format!("\x1b[35m+ AI: monitor {}\x1b[0m", if enable { "ON" } else { "OFF" }));
            }
        }
        "forward_observation" => {
            let message = tc.arguments.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let urgency = tc.arguments.get("urgency").and_then(|v| v.as_str()).unwrap_or("medium");
            if !message.is_empty() {
                app.log(&format!("\x1b[35m>> AI quer forward [{urgency}]: {message}\x1b[0m"));
            }
        }
        _ => {
            app.log(&format!("\x1b[33mAI sugeriu ação desconhecida: {}\x1b[0m", tc.name));
        }
    }
}



async fn api_logcat_status(State(state): State<ServerState>) -> Response {
    let app = &state.app;
    let s = logcat_status(app);
    json_error(StatusCode::OK, json!({
        "running": app.logcat_state.is_running(),
        "filter": app.logcat_state.filter.lock().unwrap().clone(),
        "linesCaptured": app.logcat_state.line_count(),
        "status": s,
    }))
}

async fn api_logcat_start(
    State(state): State<ServerState>,
    body: axum::body::Bytes,
) -> Response {
    let app = &state.app;
    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, json!({ "error": "Invalid JSON body" })),
    };
    let filter = parsed.get("filter").and_then(|f| f.as_str()).unwrap_or("");
    spawn_logcat(app.clone(), filter);
    json_error(StatusCode::OK, json!({ "ok": true }))
}

async fn api_logcat_stop(State(state): State<ServerState>) -> Response {
    let app = &state.app;
    stop_logcat(app);
    json_error(StatusCode::OK, json!({ "ok": true }))
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
    let method_str = req.method().to_string();

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

    let route_color = app.colors.get(&route.label);
    let now_ms = app.uptime_millis();

    if app.monitoring_enabled.load(std::sync::atomic::Ordering::Relaxed) {
        app.transfer_tracker.lock().unwrap().start_transfer(
            &id,
            &method_str,
            &full_url,
            &route.label,
            now_ms,
        );
    }
    if app.filters.lock().unwrap().should_show(&route.label) {
        app.log(&format!(
            "{DIM}[{}]{RESET} {BOLD}{}{RESET} {}{} {}{RESET}",
            timestamp(),
            id,
            route_color,
            method_str,
            full_url,
        ));
    }

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
                    let route_color = status_color(status.as_u16());
                    log_response(
                        app,
                        status.as_u16(),
                        &res_body,
                        &res_content_type,
                        duration,
                        route_color,
                        &route.label,
                    );
                    if app.monitoring_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                        app.transfer_tracker.lock().unwrap().end_transfer(
                            id,
                            status.as_u16(),
                            duration,
                            Some(res_body.len()),
                        );
                    }

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
                    if app.monitoring_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                        app.transfer_tracker.lock().unwrap().end_transfer(
                            id, 0, duration, None,
                        );
                    }
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
                if app.monitoring_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                    app.transfer_tracker.lock().unwrap().end_transfer(
                        id, 0, duration, None,
                    );
                }
                app.log(&format!("  {RED}TIMEOUT{RESET} {DIM}{duration}ms{RESET}\n"));
                json_error(
                    StatusCode::GATEWAY_TIMEOUT,
                    json!({ "error": "Timeout (120s)" }),
                )
            } else {
                if app.monitoring_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                    app.transfer_tracker.lock().unwrap().end_transfer(
                        id, 0, duration, None,
                    );
                }
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
