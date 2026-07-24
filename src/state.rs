use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

use crate::ai::AiClient;
use crate::colors::{strip_ansi, ServiceColors};
use crate::config::Config;
use crate::filters::Filters;
use crate::logger::FileLogger;

#[derive(Debug, Clone)]
pub struct Transfer {
    pub id: String,
    pub method: String,
    pub path: String,
    pub route_label: String,
    pub status: Option<u16>,
    pub duration_ms: Option<u128>,
    pub size: Option<usize>,
    pub start_ms: u64,
}

pub struct TransferTracker {
    pub transfers: Vec<Transfer>,
    max: usize,
}

impl TransferTracker {
    pub fn new(max: usize) -> Self {
        Self {
            transfers: Vec::with_capacity(max),
            max,
        }
    }

    pub fn start_transfer(
        &mut self,
        id: &str,
        method: &str,
        path: &str,
        route_label: &str,
        start_ms: u64,
    ) {
        self.transfers.insert(
            0,
            Transfer {
                id: id.to_string(),
                method: method.to_string(),
                path: path.to_string(),
                route_label: route_label.to_string(),
                status: None,
                duration_ms: None,
                size: None,
                start_ms,
            },
        );
        if self.transfers.len() > self.max {
            self.transfers.truncate(self.max);
        }
    }

    pub fn end_transfer(
        &mut self,
        id: &str,
        status: u16,
        duration_ms: u128,
        size: Option<usize>,
    ) {
        if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
            t.status = Some(status);
            t.duration_ms = Some(duration_ms);
            t.size = size;
        }
    }

    pub fn snapshot(&self) -> Vec<Transfer> {
        self.transfers.clone()
    }
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
    pub transfer_tracker: Mutex<TransferTracker>,
    pub monitoring_enabled: AtomicBool,
    pub ultra_mode: AtomicBool,
    pub ultra_routes: Mutex<HashSet<String>>,
    pub ai_client: Option<AiClient>,
    pub rt: Handle,
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

    pub fn uptime_millis(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}
