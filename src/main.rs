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
    });

    app.logger.init_session();

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
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
