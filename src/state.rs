use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;

use crate::colors::{strip_ansi, ServiceColors};
use crate::config::Config;
use crate::filters::Filters;
use crate::logger::FileLogger;

pub struct AppState {
    pub port: u16,
    pub start: Instant,
    pub filters: Mutex<Filters>,
    pub logger: FileLogger,
    pub colors: ServiceColors,
    pub config: Config,
    pub log_tx: UnboundedSender<String>,
    pub request_count: AtomicU64,
}

impl AppState {
    pub fn log(&self, text: &str) {
        self.logger.append(&strip_ansi(text));
        let _ = self.log_tx.send(text.to_string());
    }

    pub fn record_request(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn request_total(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    pub fn log_multiline(&self, text: &str) {
        for line in text.split('\n') {
            self.log(line);
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start.elapsed().as_secs()
    }
}
