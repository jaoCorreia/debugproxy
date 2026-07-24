use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::colors::{CYAN, DIM, GREEN, RED, RESET, YELLOW};
use crate::state::AppState;

pub const ANDROID_KEY: &str = "Android";

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct LogcatConfig {
    pub enabled: Option<bool>,
    pub level: Option<String>,
    pub buffer: Option<String>,
}

impl Default for LogcatConfig {
    fn default() -> Self {
        Self {
            enabled: Some(false),
            level: Some("*:W".to_string()),
            buffer: Some("main".to_string()),
        }
    }
}

impl LogcatConfig {
    pub fn default_filter(&self) -> String {
        self.level.as_deref().unwrap_or("*:W").to_string()
    }

    pub fn buffer_arg(&self) -> String {
        self.buffer.as_deref().unwrap_or("main").to_string()
    }
}

pub struct LogcatState {
    pub running: AtomicBool,
    pub pid: Mutex<Option<u32>>,
    pub lines_captured: AtomicU64,
    pub filter: Mutex<String>,
    pub started_at: Mutex<Option<String>>,
}

impl LogcatState {
    pub fn new(filter: &str) -> Self {
        Self {
            running: AtomicBool::new(false),
            pid: Mutex::new(None),
            lines_captured: AtomicU64::new(0),
            filter: Mutex::new(filter.to_string()),
            started_at: Mutex::new(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn line_count(&self) -> u64 {
        self.lines_captured.load(Ordering::Relaxed)
    }
}

fn parse_logcat_line(raw: &str) -> (String, String) {
    let clean = raw.trim();
    if clean.is_empty() {
        return ("V".to_string(), String::new());
    }
    let priority = if clean.len() >= 2 && clean.as_bytes()[1] == b'/' {
        let p = clean.as_bytes()[0];
        match p {
            b'V' => "V",
            b'D' => "D",
            b'I' => "I",
            b'W' => "W",
            b'E' => "E",
            b'F' => "F",
            _ => "V",
        }
        .to_string()
    } else {
        "V".to_string()
    };
    (priority, clean.to_string())
}

fn priority_color(p: &str) -> &str {
    match p {
        "F" | "E" => RED,
        "W" => YELLOW,
        "I" => GREEN,
        "D" => CYAN,
        _ => DIM,
    }
}

pub fn spawn_logcat(app: Arc<AppState>, filter: &str) {
    let is_enabled = app
        .config
        .logcat
        .as_ref()
        .map(|c| c.enabled.unwrap_or(false))
        .unwrap_or(false);
    if !is_enabled {
        app.log(&format!("{YELLOW}Logcat: disabled in config{RESET}"));
        return;
    }

    if app.logcat_state.is_running() {
        app.log(&format!("{YELLOW}Logcat: already running{RESET}"));
        return;
    }

    let actual_filter: String = if !filter.is_empty() {
        filter.to_string()
    } else {
        app.config.logcat.as_ref().map(|c| c.default_filter()).unwrap_or_else(|| "*:W".to_string())
    };

    let buffer = app
        .config
        .logcat
        .as_ref()
        .map(|c| c.buffer_arg())
        .unwrap_or_else(|| "main".to_string());

    let args: Vec<&str> = vec!["logcat", "-v", "brief", "-b", &buffer, &actual_filter];

    match Command::new("adb")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => {
            let pid = child.id();
            app.logcat_state.running.store(true, Ordering::Relaxed);
            app.logcat_state
                .pid
                .lock()
                .unwrap()
                .replace(pid);
            app.logcat_state
                .filter
                .lock()
                .unwrap()
                .clone_from(&actual_filter);
            let ts = crate::proxy::timestamp();
            app.logcat_state
                .started_at
                .lock()
                .unwrap()
                .replace(ts.clone());

            app.log_file_only(&format!(
                "{DIM}[{ts}]{RESET} {GREEN}Logcat: started (pid {pid}, filter: {actual_filter}){RESET}"
            ));
            if app.filters.lock().unwrap().should_show(ANDROID_KEY) {
                app.log(&format!(
                    "{DIM}[{ts}]{RESET} {GREEN}Logcat: started (pid {pid}, filter: {actual_filter}){RESET}"
                ));
            }

            let rt = app.rt.clone();
            let app2 = app.clone();
            rt.spawn_blocking(move || {
                read_logcat_stdout(child, app2);
            });
        }
        Err(e) => {
            app.log(&format!(
                "{RED}Logcat: failed to spawn adb — {e}. Is adb installed and in PATH?{RESET}"
            ));
        }
    }
}

fn read_logcat_stdout(mut child: Child, app: Arc<AppState>) {
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            app.logcat_state.running.store(false, Ordering::Relaxed);
            app.log_file_only(&format!("{RED}Logcat: no stdout{RESET}"));
            return;
        }
    };

    let reader = std::io::BufReader::new(stdout);
    for line in reader.lines() {
        match line {
            Ok(text) if !text.is_empty() => {
                let (priority, clean) = parse_logcat_line(&text);
                let color = priority_color(&priority);
                let ts = crate::proxy::timestamp();
                let formatted = format!(
                    "{DIM}[{ts}]{RESET} {color}{ANDROID_KEY}/{priority}{RESET} {clean}"
                );

                app.logcat_state.lines_captured.fetch_add(1, Ordering::Relaxed);
                app.log_file_only(&formatted);

                if app.filters.lock().unwrap().should_show(ANDROID_KEY) {
                    app.log(&formatted);
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    app.logcat_state.running.store(false, Ordering::Relaxed);
    app.logcat_state.pid.lock().unwrap().take();
    let ts = crate::proxy::timestamp();
    let total = app.logcat_state.line_count();
    app.log_file_only(&format!(
        "{DIM}[{ts}]{RESET} {YELLOW}Logcat: stopped ({total} lines captured){RESET}"
    ));
    if app.filters.lock().unwrap().should_show(ANDROID_KEY) {
        app.log(&format!(
            "{DIM}[{ts}]{RESET} {YELLOW}Logcat: stopped ({total} lines captured){RESET}"
        ));
    }

    let _ = child.wait();
}

pub fn stop_logcat(app: &Arc<AppState>) {
    if !app.logcat_state.is_running() {
        app.log(&format!("{YELLOW}Logcat: not running{RESET}"));
        return;
    }

    let pid = app.logcat_state.pid.lock().unwrap().take();
    app.logcat_state.running.store(false, Ordering::Relaxed);

    if let Some(pid) = pid {
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("taskkill");
            c.arg("/PID").arg(pid.to_string()).arg("/F");
            c
        } else {
            let mut c = Command::new("kill");
            c.arg(pid.to_string());
            c
        };
        let _ = cmd.spawn();
    }

    let ts = crate::proxy::timestamp();
    app.log(&format!(
        "{DIM}[{ts}]{RESET} {YELLOW}Logcat: stopped by user{RESET}"
    ));
}

pub fn logcat_status(app: &AppState) -> String {
    let running = app.logcat_state.is_running();
    let lines = app.logcat_state.line_count();
    let filter = app.logcat_state.filter.lock().unwrap().clone();
    let pid = app.logcat_state.pid.lock().unwrap();
    let started = app.logcat_state.started_at.lock().unwrap().clone();

    if running {
        format!(
            "{GREEN}Logcat: RUNNING{RESET} (pid {}, filter: {filter}, {lines} lines, since [{}])",
            pid.map(|p| p.to_string()).unwrap_or_else(|| "?".to_string()),
            started.unwrap_or_else(|| "?".to_string())
        )
    } else {
        format!("{DIM}Logcat: stopped{RESET} (filter: {filter})")
    }
}
