use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;

use crate::colors::{strip_ansi, ServiceColors};
use crate::config::Config;
use crate::filters::Filters;
use crate::logger::FileLogger;

#[derive(Debug, Clone, Default)]
pub struct PackageInfo {
    pub active_requests: u64,
    pub total_requests: u64,
    pub first_seen: Option<Instant>,
}

pub struct AppState {
    pub port: u16,
    pub start: Instant,
    pub filters: Mutex<Filters>,
    pub logger: FileLogger,
    pub colors: ServiceColors,
    pub config: Config,
    pub log_tx: UnboundedSender<String>,
    pub request_count: AtomicU64,
    pub package_states: Mutex<HashMap<String, PackageInfo>>,
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

    pub fn package_start(&self, label: &str) -> PackageInfo {
        let mut states = self.package_states.lock().unwrap();
        let entry = states.entry(label.to_string()).or_default();
        let was_idle = entry.active_requests == 0;
        entry.active_requests += 1;
        entry.total_requests += 1;
        if entry.first_seen.is_none() {
            entry.first_seen = Some(Instant::now());
        }
        let info = entry.clone();
        if was_idle {
            drop(states);
            self.log(&format!(
                "\x1b[35;1m⬢ PACKAGE \x1b[36m{}\x1b[0m\x1b[35;1m loading…\x1b[0m",
                label
            ));
        }
        info
    }

    pub fn package_end(&self, label: &str, duration_ms: u128) {
        let mut states = self.package_states.lock().unwrap();
        let entry = states.entry(label.to_string()).or_default();
        entry.active_requests = entry.active_requests.saturating_sub(1);
        if entry.active_requests == 0 {
            drop(states);
            self.log(&format!(
                "\x1b[32m⬢ PACKAGE \x1b[36m{}\x1b[32m done \x1b[2m{}ms\x1b[0m",
                label, duration_ms
            ));
        }
    }

    pub fn package_states_snapshot(&self) -> HashMap<String, PackageInfo> {
        self.package_states.lock().unwrap().clone()
    }
}
