use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::config::base_dir;

const FLUSH_THRESHOLD: usize = 4096;
const READ_CHUNK: u64 = 4096;

struct LogState {
    mode: String,
    writer: Option<BufWriter<File>>,
    current_file: Option<PathBuf>,
    current_day: Option<String>,
    pending: usize,
    logs_dir: PathBuf,
}

pub struct FileLogger {
    state: Mutex<LogState>,
}

impl FileLogger {
    pub fn new() -> Self {
        Self::with_dir(base_dir().join("logs"))
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            state: Mutex::new(LogState {
                mode: "day".to_string(),
                writer: None,
                current_file: None,
                current_day: None,
                pending: 0,
                logs_dir: dir,
            }),
        }
    }

    pub fn init_session(&self) {
        let mut st = self.state.lock().unwrap();
        let _ = fs::create_dir_all(&st.logs_dir);
        Self::rotate_if_needed(&mut st);
        let header = format!(
            "--- {} started at {} ---",
            if st.mode == "day" { "Day" } else { "Session" },
            chrono::Utc::now().to_rfc3339()
        );
        if let Some(w) = st.writer.as_mut() {
            let _ = writeln!(w, "{header}");
            st.pending += header.len() + 1;
        }
        Self::flush_now(&mut st);
    }

    pub fn append(&self, text: &str) {
        let mut st = self.state.lock().unwrap();
        if st.writer.is_none() {
            let _ = fs::create_dir_all(&st.logs_dir);
            Self::rotate_if_needed(&mut st);
        }
        if let Some(w) = st.writer.as_mut() {
            let _ = writeln!(w, "{text}");
            st.pending += text.len() + 1;
            Self::flush_if_needed(&mut st);
        }
    }

    fn flush_if_needed(st: &mut LogState) {
        if st.pending >= FLUSH_THRESHOLD {
            Self::flush_now(st);
        }
    }

    fn flush_now(st: &mut LogState) {
        if let Some(w) = st.writer.as_mut() {
            let _ = w.flush();
        }
        st.pending = 0;
    }

    pub fn set_mode(&self, new_mode: &str) {
        let mut st = self.state.lock().unwrap();
        Self::flush_now(&mut st);
        st.writer = None;
        st.current_file = None;
        st.current_day = None;
        st.mode = new_mode.to_string();
        let _ = fs::create_dir_all(&st.logs_dir);
        Self::rotate_if_needed(&mut st);
    }

    pub fn get_mode(&self) -> String {
        self.state.lock().unwrap().mode.clone()
    }

    pub fn get_session_file(&self) -> Option<PathBuf> {
        self.state.lock().unwrap().current_file.clone()
    }

    pub fn read_tail(&self, lines: usize) -> String {
        if lines == 0 {
            return String::new();
        }
        let path = {
            let mut st = self.state.lock().unwrap();
            Self::flush_now(&mut st);
            st.current_file.clone()
        };
        let Some(path) = path else {
            return String::new();
        };
        let mut f = match fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => return String::new(),
        };
        let file_size = match f.seek(SeekFrom::End(0)) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        if file_size == 0 {
            return String::new();
        }
        let mut buf = Vec::new();
        let mut pos = file_size;
        let mut line_count = 0usize;
        while pos > 0 && line_count <= lines {
            let read_size = READ_CHUNK.min(pos) as usize;
            pos -= read_size as u64;
            if f.seek(SeekFrom::Start(pos)).is_err() {
                break;
            }
            let mut chunk = vec![0u8; read_size];
            if f.read_exact(&mut chunk).is_err() {
                break;
            }
            line_count += memchr::memchr_iter(b'\n', &chunk).count();
            let mut new_buf = chunk;
            new_buf.extend_from_slice(&buf);
            buf = new_buf;
        }
        let buf_str = match String::from_utf8(buf) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        if line_count > lines {
            let skip = line_count - lines;
            let mut idx = 0;
            for _ in 0..skip {
                match buf_str[idx..].find('\n') {
                    Some(offset) => idx += offset + 1,
                    None => break,
                }
            }
            buf_str[idx..].to_string()
        } else {
            buf_str
        }
    }

    fn rotate_if_needed(st: &mut LogState) {
        let now = chrono::Local::now();
        if st.mode == "day" {
            let today = Self::day_key(now);
            if st.current_day.as_deref() != Some(&today) || st.writer.is_none() {
                Self::flush_now(st);
                st.writer = None;
                st.current_day = Some(today.clone());
                let p = st.logs_dir.join(format!("proxy-{today}.txt"));
                st.current_file = Some(p.clone());
                st.writer = Self::open_writer(&p);
                st.pending = 0;
            }
        } else if st.writer.is_none() {
            let p = match st.current_file.clone() {
                Some(p) => p,
                None => {
                    let key = Self::session_key(now);
                    let p = st.logs_dir.join(format!("proxy-{key}.txt"));
                    st.current_file = Some(p.clone());
                    p
                }
            };
            st.writer = Self::open_writer(&p);
            st.pending = 0;
        }
    }

    fn open_writer(path: &PathBuf) -> Option<BufWriter<File>> {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .ok()
            .map(BufWriter::new)
    }

    fn day_key(d: chrono::DateTime<chrono::Local>) -> String {
        d.format("%Y-%m-%d").to_string()
    }

    fn session_key(d: chrono::DateTime<chrono::Local>) -> String {
        d.format("%Y-%m-%d_%H-%M-%S").to_string()
    }
}

impl Drop for FileLogger {
    fn drop(&mut self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Self::flush_now(&mut st);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "debugproxy-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn session_mode_buffered_read_tail_and_threshold() {
        let dir = temp_dir();
        let logger = FileLogger::with_dir(dir.clone());
        logger.set_mode("session");
        for i in 0..5 {
            logger.append(&format!("below-{i}"));
        }
        let tail = logger.read_tail(5);
        assert!(
            tail.contains("below-4"),
            "read_tail should flush and see buffered data: {tail:?}"
        );
        let big = "y".repeat(1000);
        for _ in 0..10 {
            logger.append(&big);
        }
        let tail2 = logger.read_tail(2);
        assert!(
            tail2.contains("yyyy"),
            "threshold-flushed data should be readable: {tail2:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn day_mode_appends_are_readable() {
        let dir = temp_dir();
        let logger = FileLogger::with_dir(dir.clone());
        logger.append("day-line-1");
        logger.append("day-line-2");
        let tail = logger.read_tail(2);
        assert!(
            tail.contains("day-line-2"),
            "day-mode appends should be readable: {tail:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_mode_flushes_previous_writer() {
        let dir = temp_dir();
        let logger = FileLogger::with_dir(dir.clone());
        logger.append("before-switch");
        let prev_path = logger
            .get_session_file()
            .expect("previous file should exist");
        logger.set_mode("session");
        let contents = std::fs::read_to_string(&prev_path).unwrap_or_default();
        assert!(
            contents.contains("before-switch"),
            "set_mode should flush pending data to the previous file: {contents:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
