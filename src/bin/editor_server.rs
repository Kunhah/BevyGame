// Editor server for the Seirei Kuni data editors.
//
// This is the Rust half of the JS+Rust editor: it serves the static editor
// (tools/editors/) and exposes REST endpoints that read and write the
// canonical RON files. The JS frontend talks to these endpoints over JSON;
// all RON parsing/serialization happens here on the Rust side.
//
// Endpoints:
//   GET  /api/abilities          → JSON array of abilities (parsed from RON file)
//   POST /api/abilities          ← JSON body, written back as RON
//   GET  /api/scenes             → JSON array of DialogueScenes (one per file)
//   POST /api/scenes             ← JSON body of scenes; one .ron per scene id
//   GET  /                       → tools/editors/index.html
//   GET  /<file>                 → static file under tools/editors/
//
// Run with: cargo run --bin editor_server  (defaults to 127.0.0.1:8000)

#[path = "../dialogue/schema.rs"]
mod dialogue_schema;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use dialogue_schema::DialogueScene;

const DEFAULT_BIND: &str = "127.0.0.1:8000";
const ABILITIES_PATH: &str = "assets/data/abilities/AbilitiesExample.ron";
const DIALOGUES_DIR: &str = "assets/data/dialogues";
const STATIC_ROOT: &str = "tools/editors";

// ---------------- Ability data model (mirrors combat_ability::*) ----------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Stat {
    Health, HealthRegen, Magic, MagicRegen,
    Kiho, Onmyodo, Yokaijutsu, Kamishin,
    ActionPoints, Lethality, Hit, Agility, Defense,
    Mind, Morale, Strength, Bravery, Speed, Luck,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DamageType {
    Physical, Fire, Ice, Lightning, Holy, Dark, Poison, Bleed, True,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityEffect {
    Heal { floor: u32, ceiling: u32, scaled_with: Stat },
    Damage {
        floor: u32, ceiling: u32, damage_type: DamageType,
        scaled_with: Stat, defended_with: Stat,
    },
    Buff {
        stat: Stat, multiplier: f32,
        effects: Option<Vec<u16>>, scaled_with: Stat,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AbilityShape {
    Radius(f32),
    Line { length: f32, thickness: f32 },
    Cone { angle: f32, radius: f32 },
    Select,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum MagicSchool {
    #[default] Kiho, Onmyodo, Yokaijutsu, Kamishin,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ability {
    pub id: u16,
    pub next_id: Option<u16>,
    pub name: String,
    pub health_cost: i32,
    pub magic_cost: f32,
    #[serde(default)]
    pub magic_school: MagicSchool,
    #[serde(alias = "stamina_cost")]
    pub action_point_cost: i32,
    pub cooldown: u8,
    pub description: String,
    pub effects: Vec<AbilityEffect>,
    pub shape: AbilityShape,
    pub duration: u8,
    pub targets: u8,
}

// Dialogue data model lives in src/dialogue/schema.rs and is included via
// `#[path]` above so the editor and the runtime never drift.

// ---------------- Server ----------------

fn main() {
    let bind = std::env::var("EDITOR_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_string());
    let server = match Server::http(&bind) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("editor_server: failed to bind {bind}: {e}");
            std::process::exit(1);
        }
    };
    println!("editor_server: serving http://{bind}/");
    println!("  → static root:  {STATIC_ROOT}/");
    println!("  → abilities:    {ABILITIES_PATH}");
    println!("  → scenes dir:   {DIALOGUES_DIR}/");

    for mut request in server.incoming_requests() {
        let method = request.method().clone();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("").to_string();

        let response = match (method, path.as_str()) {
            (Method::Get, "/api/abilities") => load_typed::<Vec<Ability>>(ABILITIES_PATH),
            (Method::Post, "/api/abilities") => {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                save_typed::<Vec<Ability>>(ABILITIES_PATH, &body)
            }
            (Method::Get, "/api/scenes") => load_scenes(),
            (Method::Post, "/api/scenes") => {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                save_scenes(&body)
            }
            (Method::Get, p) => serve_static(p),
            _ => http_error(405, "method not allowed"),
        };

        if let Err(e) = request.respond(response) {
            eprintln!("editor_server: failed to send response: {e}");
        }
    }
}

// ---------------- Handlers ----------------

fn load_typed<T>(path: &str) -> Response<std::io::Cursor<Vec<u8>>>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let contents = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return http_error(404, &format!("read {path}: {e}")),
    };
    let value: T = match ron::de::from_str(&contents) {
        Ok(v) => v,
        Err(e) => return http_error(500, &format!("parse RON {path}: {e}")),
    };
    let json = match serde_json::to_string(&value) {
        Ok(s) => s,
        Err(e) => return http_error(500, &format!("encode JSON: {e}")),
    };
    json_response(200, json)
}

fn save_typed<T>(path: &str, body: &str) -> Response<std::io::Cursor<Vec<u8>>>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let value: T = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return http_error(400, &format!("parse JSON body: {e}")),
    };
    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(8)
        .indentor("    ".to_string())
        .struct_names(false);
    let ron_text = match ron::ser::to_string_pretty(&value, pretty) {
        Ok(s) => s,
        Err(e) => return http_error(500, &format!("encode RON: {e}")),
    };
    if let Err(e) = fs::write(path, ron_text) {
        return http_error(500, &format!("write {path}: {e}"));
    }
    json_response(200, "{\"ok\":true}".to_string())
}

// ---------------- Scene catalog handlers ----------------

fn load_scenes() -> Response<std::io::Cursor<Vec<u8>>> {
    let entries = match fs::read_dir(DIALOGUES_DIR) {
        Ok(e) => e,
        Err(err) => return http_error(500, &format!("read dir {DIALOGUES_DIR}: {err}")),
    };
    let mut scenes: Vec<DialogueScene> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ron") {
            continue;
        }
        let contents = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) => {
                errors.push(format!("read {}: {err}", path.display()));
                continue;
            }
        };
        match ron::de::from_str::<DialogueScene>(&contents) {
            Ok(scene) => scenes.push(scene),
            Err(err) => errors.push(format!("parse {}: {err}", path.display())),
        }
    }
    if !errors.is_empty() {
        return http_error(500, &errors.join("; "));
    }
    scenes.sort_by(|a, b| a.id.cmp(&b.id));
    let json = match serde_json::to_string(&scenes) {
        Ok(s) => s,
        Err(e) => return http_error(500, &format!("encode JSON: {e}")),
    };
    json_response(200, json)
}

fn save_scenes(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let scenes: Vec<DialogueScene> = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return http_error(400, &format!("parse JSON body: {e}")),
    };

    // Validate ids before touching the disk.
    let mut ids = std::collections::HashSet::new();
    for scene in &scenes {
        if !is_safe_scene_id(&scene.id) {
            return http_error(
                400,
                &format!(
                    "invalid scene id '{}'; expected [a-zA-Z0-9_-]+ characters",
                    scene.id
                ),
            );
        }
        if !ids.insert(scene.id.clone()) {
            return http_error(400, &format!("duplicate scene id '{}'", scene.id));
        }
    }

    // Determine which scene files currently exist; we'll delete any whose id
    // doesn't appear in the incoming payload.
    let dir = Path::new(DIALOGUES_DIR);
    if !dir.exists() {
        if let Err(e) = fs::create_dir_all(dir) {
            return http_error(500, &format!("create {DIALOGUES_DIR}: {e}"));
        }
    }
    let existing_files: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(it) => it
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ron"))
            .collect(),
        Err(e) => return http_error(500, &format!("read dir {DIALOGUES_DIR}: {e}")),
    };

    let pretty = ron::ser::PrettyConfig::new()
        .depth_limit(8)
        .indentor("    ".to_string())
        .struct_names(false);
    for scene in &scenes {
        let path = dir.join(format!("{}.ron", scene.id));
        let ron_text = match ron::ser::to_string_pretty(scene, pretty.clone()) {
            Ok(s) => s,
            Err(e) => return http_error(500, &format!("encode RON {}: {e}", path.display())),
        };
        if let Err(e) = fs::write(&path, ron_text) {
            return http_error(500, &format!("write {}: {e}", path.display()));
        }
    }

    for path in existing_files {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if !ids.contains(&stem) {
            if let Err(e) = fs::remove_file(&path) {
                eprintln!("editor_server: failed to delete orphan {}: {e}", path.display());
            }
        }
    }

    json_response(200, "{\"ok\":true}".to_string())
}

fn is_safe_scene_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn serve_static(url_path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let trimmed = url_path.trim_start_matches('/');
    let relative = if trimmed.is_empty() { "index.html" } else { trimmed };

    // Disallow path escapes.
    let candidate = PathBuf::from(STATIC_ROOT).join(relative);
    let canonical_root = match Path::new(STATIC_ROOT).canonicalize() {
        Ok(p) => p,
        Err(_) => return http_error(500, "static root unavailable"),
    };
    let canonical_target = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return http_error(404, "not found"),
    };
    if !canonical_target.starts_with(&canonical_root) {
        return http_error(403, "forbidden");
    }

    let bytes = match fs::read(&canonical_target) {
        Ok(b) => b,
        Err(_) => return http_error(404, "not found"),
    };
    let mime = guess_mime(&canonical_target);
    let response = Response::from_data(bytes).with_status_code(StatusCode(200));
    response.with_header(content_type(mime))
}

// ---------------- Helpers ----------------

fn http_error(code: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!("{{\"error\":\"{}\"}}", msg.replace('"', "'"));
    Response::from_string(body)
        .with_status_code(StatusCode(code))
        .with_header(content_type("application/json"))
}

fn json_response(code: u16, body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(StatusCode(code))
        .with_header(content_type("application/json"))
}

fn content_type(mime: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], mime.as_bytes())
        .expect("valid header")
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "ron" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}
