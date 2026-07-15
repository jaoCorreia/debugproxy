use std::collections::HashMap;

use crate::routes::get_routes;

pub const LOGS_KEY: &str = "Logs";

#[derive(Debug, Default)]
pub struct Filters {
    pub state: Vec<(String, bool)>,
    pub aliases: HashMap<String, String>,
}

impl Filters {
    pub fn new() -> Self {
        let mut f = Self::default();
        f.rebuild();
        f
    }

    pub fn rebuild(&mut self) {
        let routes = get_routes();
        let labels: Vec<String> = routes.iter().map(|r| r.label.clone()).collect();

        let prev: HashMap<String, bool> = self.state.iter().cloned().collect();
        let mut state: Vec<(String, bool)> = Vec::new();
        for l in &labels {
            if state.iter().any(|(k, _)| k == l) {
                continue;
            }
            state.push((l.clone(), *prev.get(l).unwrap_or(&true)));
        }
        state.push((
            LOGS_KEY.to_string(),
            *prev.get(LOGS_KEY).unwrap_or(&true),
        ));

        let mut aliases: HashMap<String, String> = HashMap::new();
        for r in &routes {
            let short = r.prefix.trim_start_matches('/').to_string();
            aliases.insert(short, r.label.clone());
        }
        aliases.insert("l".to_string(), LOGS_KEY.to_string());

        let mut all_labels: Vec<String> = vec![LOGS_KEY.to_string()];
        all_labels.extend(labels);

        let mut counts: HashMap<char, usize> = HashMap::new();
        for l in &all_labels {
            if let Some(ch) = l.chars().next() {
                *counts.entry(ch.to_ascii_lowercase()).or_insert(0) += 1;
            }
        }
        for (ch, count) in counts {
            if count != 1 {
                continue;
            }
            if let Some(label) = all_labels
                .iter()
                .find(|l| l.chars().next().map(|c| c.to_ascii_lowercase()) == Some(ch))
            {
                aliases.insert(ch.to_string(), label.clone());
            }
        }

        self.state = state;
        self.aliases = aliases;
    }

    pub fn should_show(&self, route_label: &str) -> bool {
        if route_label.is_empty() {
            return true;
        }
        let key = if route_label == "LOG" { LOGS_KEY } else { route_label };
        self.state
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
            .unwrap_or(true)
    }

    pub fn toggle(&mut self, label: &str) -> bool {
        for (k, v) in self.state.iter_mut() {
            if k == label {
                *v = !*v;
                return true;
            }
        }
        false
    }

    pub fn handle_command(&mut self, cmd: &str) {
        match cmd {
            "all" => {
                for (_, v) in self.state.iter_mut() {
                    *v = true;
                }
            }
            "none" => {
                for (_, v) in self.state.iter_mut() {
                    *v = false;
                }
            }
            _ => {
                if let Some(label) = self.aliases.get(cmd).cloned() {
                    self.toggle(&label);
                }
            }
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        let map: serde_json::Map<String, serde_json::Value> = self
            .state
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v)))
            .collect();
        serde_json::Value::Object(map)
    }
}
