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
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}
