use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::base_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub prefix: String,
    pub target: String,
    pub label: String,
}

fn static_path() -> Option<PathBuf> {
    let p = base_dir().join("routes.json");
    if p.exists() {
        return Some(p);
    }
    let e = base_dir().join("routes.example.json");
    if e.exists() {
        return Some(e);
    }
    None
}

fn dynamic_path() -> PathBuf {
    base_dir().join("routes-dynamic.json")
}

fn load_file(path: Option<PathBuf>) -> Vec<Route> {
    let Some(path) = path else {
        return Vec::new();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_dynamic(routes: &[Route]) {
    if let Ok(json) = serde_json::to_string_pretty(routes) {
        let _ = std::fs::write(dynamic_path(), json);
    }
}

pub fn get_routes() -> Vec<Route> {
    let mut all = load_file(static_path());
    all.extend(load_file(Some(dynamic_path())));
    all
}

pub fn add_route(prefix: &str, target: &str, label: &str) -> Result<Route, String> {
    let prefix = if prefix.starts_with('/') {
        prefix.to_string()
    } else {
        format!("/{prefix}")
    };
    if get_routes().iter().any(|r| r.prefix == prefix) {
        return Err(format!("Rota \"{prefix}\" já existe"));
    }
    let route = Route {
        prefix,
        target: target.to_string(),
        label: label.to_string(),
    };
    let mut dynamic = load_file(Some(dynamic_path()));
    dynamic.push(route.clone());
    save_dynamic(&dynamic);
    Ok(route)
}

pub fn remove_route(prefix: &str) -> Result<(), String> {
    let statics = load_file(static_path());
    if statics.iter().any(|r| r.prefix == prefix) {
        return Err(format!("Rota \"{prefix}\" é fixa, não pode ser removida"));
    }
    let mut dynamic = load_file(Some(dynamic_path()));
    let Some(idx) = dynamic.iter().position(|r| r.prefix == prefix) else {
        return Err(format!("Rota \"{prefix}\" não encontrada"));
    };
    dynamic.remove(idx);
    save_dynamic(&dynamic);
    Ok(())
}

pub fn find_route(pathname: &str) -> Option<Route> {
    let mut routes = get_routes();
    routes.sort_by(|a, b| b.prefix.len().cmp(&a.prefix.len()));
    routes
        .into_iter()
        .find(|r| pathname == r.prefix || pathname.starts_with(&format!("{}/", r.prefix)))
}
