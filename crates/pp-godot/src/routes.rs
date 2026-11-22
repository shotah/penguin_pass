//! Custom routes under `res://routes/` and `user://routes/`.

use std::collections::BTreeMap;
#[cfg(test)]
use std::path::Path;

use godot::classes::DirAccess;
use godot::classes::FileAccess;
use godot::classes::file_access::ModeFlags;
use godot::prelude::*;
use pp_course::{Course, is_route_id};
use pp_session::RouteEntry;

const RES_ROOT: &str = "res://routes";
const USER_ROOT: &str = "user://routes";

/// Scan Godot route directories and return discovered entries (sorted by id).
///
/// User routes replace `res://` entries with the same id.
pub fn load_all() -> Vec<RouteEntry> {
    let mut by_id = BTreeMap::new();
    merge_root(RES_ROOT, false, &mut by_id);
    merge_root(USER_ROOT, true, &mut by_id);
    let entries: Vec<_> = by_id.into_values().collect();
    let custom = entries
        .iter()
        .filter(|entry| entry.id != "penguin_pass" && entry.id != "lonely_road")
        .count();
    if custom > 0 {
        godot_print!("PENGUIN_PASS_ROUTES loaded={}", entries.len());
    }
    entries
}

fn merge_root(root: &str, user_route: bool, out: &mut BTreeMap<String, RouteEntry>) {
    let Some(dir) = DirAccess::open(root) else {
        return;
    };
    for index in 0..dir.get_directories().len() {
        let Some(id) = dir.get_directories().get(index) else {
            continue;
        };
        let id = id.to_string();
        if !is_route_id(&id) {
            continue;
        }
        let path = format!("{root}/{id}/route.json");
        if !FileAccess::file_exists(&path) {
            continue;
        }
        if let Some(entry) = load_route_file(&path, &id, user_route) {
            out.insert(id, entry);
        }
    }
    for index in 0..dir.get_files().len() {
        let Some(name) = dir.get_files().get(index) else {
            continue;
        };
        let name = name.to_string();
        let Some(stem) = route_json_stem(&name) else {
            continue;
        };
        if !is_route_id(stem) {
            continue;
        }
        let path = format!("{root}/{stem}.json");
        if let Some(entry) = load_route_file(&path, stem, user_route) {
            out.insert(stem.to_owned(), entry);
        }
    }
}

fn load_route_file(path: &str, id: &str, user_route: bool) -> Option<RouteEntry> {
    let Some(file) = FileAccess::open(path, ModeFlags::READ) else {
        godot_error!("Penguin Pass: route '{path}' missing or unreadable");
        return None;
    };
    let text = file.get_as_text().to_string();
    match parse_route_entry(id, &text, user_route) {
        Ok(entry) => Some(entry),
        Err(err) => {
            godot_error!("Penguin Pass: route '{path}' rejected ({err})");
            None
        }
    }
}

/// List `route.json` files under subfolders and flat `*.json` stems (testable).
#[cfg(test)]
pub fn scan_dir(dir: &Path, user_route: bool) -> Vec<RouteEntry> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<_> = read_dir.filter_map(Result::ok).collect();
    names.sort_by_key(|entry| entry.file_name());
    let mut by_id = BTreeMap::new();
    for entry in names {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let path = entry.path();
        if path.is_dir() {
            if !is_route_id(name.as_ref()) {
                continue;
            }
            let route_json = path.join("route.json");
            if !route_json.is_file() {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(route_json) {
                push_parsed(&mut by_id, name.as_ref(), &text, user_route);
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(stem) = route_json_stem(name.as_ref()) else {
            continue;
        };
        if !is_route_id(stem) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(path) {
            push_parsed(&mut by_id, stem, &text, user_route);
        }
    }
    by_id.into_values().collect()
}

fn route_json_stem(name: &str) -> Option<&str> {
    if name.ends_with(".import") || name.ends_with(".uid") || !name.ends_with(".json") {
        return None;
    }
    name.strip_suffix(".json")
}

#[cfg(test)]
fn push_parsed(by_id: &mut BTreeMap<String, RouteEntry>, id: &str, text: &str, user_route: bool) {
    if let Ok(entry) = parse_route_entry(id, text, user_route) {
        by_id.insert(id.to_owned(), entry);
    }
}

/// Parse route JSON into a catalog entry.
pub fn parse_route_entry(id: &str, text: &str, user_route: bool) -> Result<RouteEntry, String> {
    let course = text.parse::<Course>().map_err(|err| err.to_string())?;
    Ok(RouteEntry {
        id: id.to_owned(),
        course,
        user_route,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn closed_loop_json(name: &str) -> String {
        let length = 50.0 * std::f64::consts::TAU;
        format!(
            r#"{{"name":"{name}","start":{{"x":0.0,"z":0.0,"heading_rad":0.0}},"segments":[{{"type":"arc","radius_m":50.0,"angle_rad":{angle}}}],"grade":[{{"type":"hold","length_m":{length},"grade_pct":0.0}}]}}"#,
            angle = std::f64::consts::TAU,
            length = length
        )
    }

    fn open_line_json() -> &'static str {
        r#"{"name":"Open","start":{"x":0.0,"z":0.0,"heading_rad":0.0},"segments":[{"type":"line","length_m":500.0}],"grade":[{"type":"hold","length_m":500.0,"grade_pct":0.0}]}"#
    }

    fn temp_routes_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pp_routes_test_{}_{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn scan_dir_finds_subfolder_route_json() {
        let root = temp_routes_dir("subfolder");
        let route_dir = root.join("valley_loop");
        fs::create_dir_all(&route_dir).expect("subdir");
        fs::write(
            route_dir.join("route.json"),
            closed_loop_json("Valley Loop"),
        )
        .expect("write");
        let entries = scan_dir(&root, false);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "valley_loop");
        assert!(!entries[0].user_route);
    }

    #[test]
    fn scan_dir_finds_flat_json() {
        let root = temp_routes_dir("flat");
        fs::write(
            root.join("sprint_ring.json"),
            closed_loop_json("Sprint Ring"),
        )
        .expect("write");
        let entries = scan_dir(&root, true);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "sprint_ring");
        assert!(entries[0].user_route);
    }

    #[test]
    fn scan_dir_skips_invalid_json_and_open_loops() {
        let root = temp_routes_dir("invalid");
        let bad_dir = root.join("bad_json");
        fs::create_dir_all(&bad_dir).expect("subdir");
        fs::write(bad_dir.join("route.json"), "{ not json").expect("write");
        let open_dir = root.join("open_course");
        fs::create_dir_all(&open_dir).expect("subdir");
        fs::write(open_dir.join("route.json"), open_line_json()).expect("write");
        let ok = root.join("ok_loop");
        fs::create_dir_all(&ok).expect("subdir");
        fs::write(ok.join("route.json"), closed_loop_json("Ok")).expect("write");
        let entries = scan_dir(&root, false);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "ok_loop");
    }

    #[test]
    fn scan_dir_ignores_bad_names_and_sorts_by_id() {
        let root = temp_routes_dir("names");
        fs::create_dir_all(root.join("Bad Name")).expect("space");
        fs::write(root.join("Bad Name/route.json"), closed_loop_json("Bad")).expect("write");
        fs::create_dir_all(root.join("bad.dot")).expect("dots");
        fs::write(root.join("bad.dot/route.json"), closed_loop_json("Dot")).expect("write");
        let z_dir = root.join("z_route");
        fs::create_dir_all(&z_dir).expect("z");
        fs::write(z_dir.join("route.json"), closed_loop_json("Z")).expect("write");
        let a_dir = root.join("a_route");
        fs::create_dir_all(&a_dir).expect("a");
        fs::write(a_dir.join("route.json"), closed_loop_json("A")).expect("write");
        let entries = scan_dir(&root, false);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "a_route");
        assert_eq!(entries[1].id, "z_route");
    }
}
