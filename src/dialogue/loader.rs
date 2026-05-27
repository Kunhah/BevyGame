use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::Deserialize;

use super::runtime::DialogueCatalog;
use super::schema::{
    ChoiceNode, ChoiceOption, DialogueNode, DialogueScene, Effect, LineNode, ReputationTargetRef,
    Speaker,
};

const DIALOGUES_DIR: &str = "assets/data/dialogues";
const REPUTATION_RULES_PATH: &str = "assets/data/dialogue_reputation.ron";

// ---------------------------------------------------------------------------
// Public entry: build the catalog at startup.
// ---------------------------------------------------------------------------

pub fn build_dialogue_catalog() -> DialogueCatalog {
    let reputation_rules = load_reputation_rules().unwrap_or_default();
    let mut catalog = DialogueCatalog::default();

    let dir = Path::new(DIALOGUES_DIR);
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            warn!("dialogue loader: cannot read {DIALOGUES_DIR}: {err}");
            return catalog;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ron") {
            continue;
        }
        match load_scene_file(&path, &reputation_rules) {
            Ok(scene) => {
                let id = scene.id.clone();
                if catalog.scenes.insert(id.clone(), scene).is_some() {
                    warn!("dialogue loader: duplicate scene id '{id}' (later file wins)");
                }
            }
            Err(err) => warn!("dialogue loader: skipping {}: {err}", path.display()),
        }
    }

    catalog.rebuild_node_index();
    info!("dialogue loader: loaded {} scene(s)", catalog.scenes.len());
    catalog
}

// ---------------------------------------------------------------------------
// File parsing: try new format first, fall back to legacy flat list.
// ---------------------------------------------------------------------------

fn load_scene_file(path: &Path, rules: &ReputationRules) -> Result<DialogueScene, String> {
    let contents =
        fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))?;

    // Try new schema first.
    if let Ok(scene) = ron::de::from_str::<DialogueScene>(&contents) {
        return Ok(scene);
    }

    // Legacy compat shim.
    let legacy: Vec<LegacyLine> = ron::de::from_str(&contents)
        .map_err(|e| format!("not a DialogueScene and not legacy Vec<DialogueLine>: {e}"))?;

    Ok(legacy_to_scene(legacy, scene_id_from_path(path), rules))
}

fn scene_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

// ---------------------------------------------------------------------------
// Legacy schema (kept private to this module — only used during conversion).
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LegacyLine {
    id: String,
    speaker: String,
    text: String,
    #[serde(default)]
    next: Option<String>,
    #[serde(default)]
    choices: Option<Vec<LegacyChoice>>,
}

#[derive(Deserialize)]
struct LegacyChoice {
    event: u32,
    text: String,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize, Default)]
struct ReputationRulesFile {
    #[serde(default)]
    rules: Vec<ReputationRuleEntry>,
}

#[derive(Deserialize)]
struct ReputationRuleEntry {
    event_id: u32,
    #[serde(default)]
    effects: Vec<ReputationRuleEffect>,
}

#[derive(Deserialize, Clone)]
struct ReputationRuleEffect {
    target: LegacyReputationTarget,
    delta: i16,
    reason: String,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum LegacyReputationTarget {
    LocalGovernor,
    LocalMerchant,
    LocalClan,
}

#[derive(Default)]
struct ReputationRules(HashMap<u32, Vec<ReputationRuleEffect>>);

fn load_reputation_rules() -> Option<ReputationRules> {
    let contents = fs::read_to_string(REPUTATION_RULES_PATH).ok()?;
    let parsed: ReputationRulesFile = ron::de::from_str(&contents)
        .map_err(|e| warn!("failed to parse {REPUTATION_RULES_PATH}: {e}"))
        .ok()?;
    let mut out = HashMap::new();
    for rule in parsed.rules {
        if !rule.effects.is_empty() {
            out.insert(rule.event_id, rule.effects);
        }
    }
    Some(ReputationRules(out))
}

// ---------------------------------------------------------------------------
// Legacy → new conversion.
// ---------------------------------------------------------------------------

fn legacy_to_scene(
    lines: Vec<LegacyLine>,
    fallback_scene_id: String,
    rules: &ReputationRules,
) -> DialogueScene {
    let start = lines.first().map(|l| l.id.clone()).unwrap_or_default();
    let mut nodes: HashMap<String, DialogueNode> = HashMap::new();

    for legacy in lines {
        match legacy.choices {
            None => {
                nodes.insert(
                    legacy.id.clone(),
                    DialogueNode::Line(LineNode {
                        speaker: Speaker {
                            name: legacy.speaker,
                            ..Default::default()
                        },
                        text: legacy.text,
                        on_enter: Vec::new(),
                        condition: None,
                        next: legacy.next,
                    }),
                );
            }
            Some(legacy_choices) => {
                // A legacy entry with choices renders text + choices together.
                // Map it to a single Choice node whose prompt carries that text
                // so `next` references to this id resolve here directly.
                let options = legacy_choices
                    .into_iter()
                    .map(|c| ChoiceOption {
                        text: c.text,
                        condition: None,
                        effects: legacy_event_to_effects(c.event, rules),
                        next: c.next,
                        legacy_event_id: c.event,
                    })
                    .collect();

                nodes.insert(
                    legacy.id.clone(),
                    DialogueNode::Choice(ChoiceNode {
                        prompt: Some(Speaker {
                            name: legacy.speaker,
                            ..Default::default()
                        }),
                        prompt_text: Some(legacy.text),
                        options,
                    }),
                );
            }
        }
    }

    DialogueScene {
        id: fallback_scene_id,
        background: None,
        music: None,
        start,
        nodes,
    }
}

fn legacy_event_to_effects(event_id: u32, rules: &ReputationRules) -> Vec<Effect> {
    if event_id == 0 {
        return Vec::new();
    }
    let Some(effects) = rules.0.get(&event_id) else {
        return Vec::new();
    };
    effects
        .iter()
        .map(|e| Effect::Reputation {
            target: match e.target {
                LegacyReputationTarget::LocalGovernor => ReputationTargetRef::LocalGovernor,
                LegacyReputationTarget::LocalMerchant => ReputationTargetRef::LocalMerchant,
                LegacyReputationTarget::LocalClan => ReputationTargetRef::LocalClan,
            },
            delta: e.delta,
            reason: e.reason.clone(),
        })
        .collect()
}

// Re-export so other modules don't need a separate path lookup.
#[allow(dead_code)]
pub fn dialogues_dir() -> PathBuf {
    PathBuf::from(DIALOGUES_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity check: every `.ron` file under `assets/data/dialogues/` either
    /// parses as the new `DialogueScene` schema or as a legacy flat
    /// `Vec<DialogueLine>` that the compat shim can convert. Catches typos
    /// in node-variant tags (line/choice/scene), speaker slot names, and
    /// effect/condition variants before the game runs.
    #[test]
    fn dialogue_files_parse() {
        let dir = Path::new(DIALOGUES_DIR);
        let entries = std::fs::read_dir(dir).expect("read dialogues dir");
        let rules = ReputationRules::default();
        let mut count = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ron") {
                continue;
            }
            let scene = load_scene_file(&path, &rules)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(
                scene.nodes.contains_key(&scene.start),
                "{}: start node '{}' not in nodes map",
                path.display(),
                scene.start,
            );
            count += 1;
        }
        assert!(count > 0, "no dialogue files found under {DIALOGUES_DIR}");
    }
}
