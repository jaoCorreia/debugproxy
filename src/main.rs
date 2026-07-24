mod ai;
mod colors;
mod config;
mod filters;
mod logger;
mod proxy;
mod routes;
mod screensaver;
mod state;
mod tui;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ai::AiClient;
use colors::ServiceColors;
use filters::Filters;
use logger::FileLogger;
use state::{AppState, TransferTracker};

fn pause_on_exit() {
    #[cfg(target_os = "windows")]
    {
        eprintln!("\nPress Enter to close...");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

fn main() {
    let _ = dotenvy::dotenv();

    std::panic::set_hook(Box::new(|info| {
        let msg = format!("FATAL: {info}");
        eprintln!("{msg}");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("debugproxy-crash.log")
        {
            use std::io::Write;
            let _ = writeln!(f, "{msg}");
            eprintln!("Crash details written to debugproxy-crash.log");
        }
    }));

    let cfg = config::load();
    let port = config::resolve_port(&cfg);

    if let Err(e) = std::net::TcpListener::bind(("0.0.0.0", port)) {
        eprintln!("Port {port} is in use or unavailable: {e}");
        pause_on_exit();
        std::process::exit(1);
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    let ai_cfg = cfg.ai.clone().unwrap_or_default();
    let ai_client = if ai_cfg.enabled.unwrap_or(true) {
        let keys: &[&str] = &["DEEPSEEK_API_KEY", "AI_API_KEY", "OPENAI_API_KEY"];
        let api_key = keys
            .iter()
            .find_map(|k| std::env::var(k).ok())
            .filter(|v| !v.is_empty())
            .unwrap_or_default();
        if api_key.is_empty() {
            eprintln!("AI: no API key found (set DEEPSEEK_API_KEY or AI_API_KEY). AI features disabled.");
            None
        } else {
            eprintln!("AI: enabled ({})", ai_cfg.model.as_deref().unwrap_or("deepseek-chat"));
            Some(AiClient::new(ai_cfg, api_key))
        }
    } else {
        None
    };

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    let app = Arc::new(AppState {
        port,
        start: Instant::now(),
        filters: Mutex::new(Filters::new()),
        logger: FileLogger::new(),
        colors: ServiceColors::new(&cfg.colors),
        config: cfg,
        log_tx: tx,
        request_count: std::sync::atomic::AtomicU64::new(0),
        transfer_tracker: Mutex::new(TransferTracker::new(200)),
        monitoring_enabled: std::sync::atomic::AtomicBool::new(false),
        ultra_mode: std::sync::atomic::AtomicBool::new(false),
        ultra_routes: Mutex::new(HashSet::new()),
        ai_client,
        rt: rt.handle().clone(),
    });

    app.logger.init_session();

    let server_app = app.clone();
    rt.spawn(async move {
        proxy::run(server_app).await;
    });

    if let Err(e) = tui::run(app, rx) {
        eprintln!("TUI error: {e}");
        pause_on_exit();
        std::process::exit(1);
    }
    pause_on_exit();
}
