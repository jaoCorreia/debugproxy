use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::config::base_dir;

pub struct FileLogger {
    mode: Mutex<String>,
    current_file: Mutex<Option<PathBuf>>,
    current_day: Mutex<Option<String>>,
}

impl FileLogger {
    pub fn new() -> Self {
        Self {
            mode: Mutex::new("day".to_string()),
            current_file: Mutex::new(None),
            current_day: Mutex::new(None),
        }
    }

    pub fn init_session(&self) {
        let _ = fs::create_dir_all(logs_dir());
        let file = self.resolve_file();
        let header = {
            let mode = self.mode.lock().unwrap();
            format!(
                "--- {} started at {} ---",
                if *mode == "day" { "Day" } else { "Session" },
                chrono::Utc::now().to_rfc3339()
            )
        };
        if let Some(ref path) = file {
            if let Ok(mut f) = fs::OpenOptions::new().append(true).create(true).open(path)
            {
                let _ = writeln!(f, "{header}");
            }
        }
    }

    pub fn append(&self, text: &str) {
        let _ = fs::create_dir_all(logs_dir());
        let file = self.resolve_file();
        if let Some(ref path) = file {
            if let Ok(mut f) = fs::OpenOptions::new().append(true).create(true).open(path)
            {
                let _ = writeln!(f, "{text}");
            }
        }
    }

    pub fn set_mode(&self, new_mode: &str) {
        {
            let mut mode = self.mode.lock().unwrap();
            *mode = new_mode.to_string();
        }
        {
            let mut cfile = self.current_file.lock().unwrap();
            *cfile = None;
        }
        {
            let mut cday = self.current_day.lock().unwrap();
            *cday = None;
        }
        let _ = fs::create_dir_all(logs_dir());
        self.resolve_file();
    }

    pub fn get_mode(&self) -> String {
        self.mode.lock().unwrap().clone()
    }

    pub fn get_session_file(&self) -> Option<PathBuf> {
        self.current_file.lock().unwrap().clone()
    }

    pub fn read_tail(&self, lines: usize) -> String {
        let file = self.resolve_file();
        if let Some(ref path) = file {
            if let Ok(raw) = fs::read_to_string(path) {
                let all: Vec<&str> = raw.lines().collect();
                let start = if all.len() > lines { all.len() - lines } else { 0 };
                return all[start..].join("\n");
            }
        }
        "(sem logs ainda)".to_string()
    }

    fn day_key(d: chrono::DateTime<chrono::Local>) -> String {
        d.format("%Y-%m-%d").to_string()
    }

    fn session_key(d: chrono::DateTime<chrono::Local>) -> String {
        d.format("%Y-%m-%d_%H-%M-%S").to_string()
    }

    fn resolve_file(&self) -> Option<PathBuf> {
        let now = chrono::Local::now();
        let mode = self.mode.lock().unwrap().clone();

        let new_path = if mode == "day" {
            let today = Self::day_key(now);
            let mut cfile = self.current_file.lock().unwrap();
            let mut cday = self.current_day.lock().unwrap();
            if cday.as_deref() != Some(&today) {
                *cday = Some(today.clone());
                let p = logs_dir().join(format!("proxy-{today}.txt"));
                *cfile = Some(p);
            }
            cfile.clone()
        } else {
            let mut cfile = self.current_file.lock().unwrap();
            if cfile.is_none() {
                let key = Self::session_key(now);
                let p = logs_dir().join(format!("proxy-{key}.txt"));
                *cfile = Some(p);
            }
            cfile.clone()
        };

        new_path
    }
}

fn logs_dir() -> PathBuf {
    base_dir().join("logs")
}
