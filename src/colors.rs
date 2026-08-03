use std::collections::HashMap;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";

fn ansi_code(name: &str) -> Option<&'static str> {
    match name {
        "green" => Some("\x1b[32m"),
        "yellow" => Some("\x1b[33m"),
        "red" => Some("\x1b[31m"),
        "cyan" => Some("\x1b[36m"),
        "magenta" => Some("\x1b[35m"),
        "blue" => Some("\x1b[34m"),
        "white" => Some("\x1b[37m"),
        "dim" => Some("\x1b[2m"),
        _ => None,
    }
}

fn default_service_colors() -> HashMap<String, String> {
    HashMap::new()
}

#[derive(Debug, Clone)]
pub struct ServiceColors {
    map: HashMap<String, &'static str>,
}

impl ServiceColors {
    pub fn new(user_colors: &HashMap<String, String>) -> Self {
        let mut merged = default_service_colors();
        for (k, v) in user_colors {
            merged.insert(k.clone(), v.clone());
        }
        let map = merged
            .into_iter()
            .map(|(label, name)| (label, ansi_code(&name).unwrap_or(DIM)))
            .collect();
        Self { map }
    }

    pub fn get(&self, label: &str) -> &'static str {
        self.map.get(label).copied().unwrap_or(RESET)
    }
}

pub fn status_color(code: u16) -> &'static str {
    if code >= 500 {
        RED
    } else if code >= 400 {
        YELLOW
    } else if code >= 200 {
        GREEN
    } else {
        YELLOW
    }
}

pub fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                i += 2;
                while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                continue;
            }
            out.push('\x1b');
            i += 1;
            continue;
        }
        let next = memchr::memchr(0x1b, &bytes[i..])
            .map(|p| i + p)
            .unwrap_or(bytes.len());
        out.push_str(&s[i..next]);
        i = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_passthrough_without_escapes() {
        assert_eq!(strip_ansi("plain text"), "plain text");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn strip_ansi_removes_full_sequences() {
        assert_eq!(strip_ansi("\x1b[32mGET\x1b[0m /path"), "GET /path");
        assert_eq!(
            strip_ansi("\x1b[1;31mERR\x1b[0m \x1b[2mdim\x1b[0m"),
            "ERR dim"
        );
    }

    #[test]
    fn strip_ansi_preserves_lone_esc_and_trailing_esc() {
        assert_eq!(strip_ansi("a\x1bb"), "a\x1bb");
        assert_eq!(strip_ansi("end\x1b"), "end\x1b");
    }

    #[test]
    fn strip_ansi_handles_unterminated_sequence() {
        assert_eq!(strip_ansi("x\x1b[31"), "x");
    }

    #[test]
    fn strip_ansi_preserves_multibyte_utf8() {
        assert_eq!(strip_ansi("\x1b[32mcafé\x1b[0m 🚀"), "café 🚀");
    }

    #[test]
    fn strip_ansi_pure_multibyte_no_escapes() {
        assert_eq!(strip_ansi("こんにちは"), "こんにちは");
    }

    #[test]
    fn strip_ansi_consecutive_sequences_collapse() {
        assert_eq!(strip_ansi("\x1b[31m\x1b[0m"), "");
        assert_eq!(strip_ansi("a\x1b[1m\x1b[31mb\x1b[0mc"), "abc");
    }

    #[test]
    fn strip_ansi_lone_esc_at_start() {
        assert_eq!(strip_ansi("\x1bhello"), "\x1bhello");
    }
}
