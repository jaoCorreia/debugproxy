mod colors;
mod config;
mod filters;
mod logger;
mod proxy;
mod routes;
mod screensaver;
mod state;
mod tui;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use colors::ServiceColors;
use filters::Filters;
use logger::FileLogger;
use state::AppState;

fn main() {
    let cfg = config::load();
    let port = config::resolve_port(&cfg);

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
    });

    app.logger.init_session();

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let server_app = app.clone();
    rt.spawn(async move {
        proxy::run(server_app).await;
    });

    if let Err(e) = tui::run(app, rx) {
        eprintln!("TUI error: {e}");
        std::process::exit(1);
    }
}
