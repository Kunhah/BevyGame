//! World-rule engine.
//!
//! Reactive layer that lets designers declare `IF trigger AND condition THEN
//! actions` in RON files without touching code. Reuses the dialogue
//! `Condition` AST and `Effect` enum so flag / reputation / quest /
//! inventory / audio mutations don't need a parallel implementation.
//!
//! Rules live under `assets/data/world_rules/*.ron`, each file containing a
//! `Vec<WorldRule>`. Triggers fire from existing engine events (flag changes,
//! area changes, dialogue choices, item pickups, quest completions); world-
//! mutation actions can spawn/despawn entities by name, swap sprites, or
//! start dialogue scenes.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::dialogue::{
    ConditionContext, DialogueCatalog, DialogueRuntime, EffectDispatcher, evaluate_condition,
    Condition, Effect,
};
use crate::map::AreaChanged;
use crate::quests::{DialogueChoicePickedEvent, QuestStatus, QuestStatusChangedEvent};
use crate::story_flags::FlagChangedEvent;

const WORLD_RULES_DIR: &str = "assets/data/world_rules";

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldRule {
    pub id: String,
    /// Rule fires if ANY of these triggers matches an incoming event.
    pub triggers: Vec<WorldTrigger>,
    /// Optional condition, evaluated against the dialogue `ConditionContext`
    /// (story flags, inventory, quest log, reputation). AND-ed with the
    /// trigger.
    #[serde(default)]
    pub condition: Option<Condition>,
    pub actions: Vec<WorldRuleAction>,
    /// When true, the rule fires at most once per game session.
    #[serde(default)]
    pub once: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldTrigger {
    FlagSet(String),
    FlagCleared(String),
    AreaEntered { region_id: u16 },
    AnyAreaEntered,
    DialogueChoicePicked { event_id: u32 },
    QuestCompleted { quest_id: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldRuleAction {
    /// Wrap any dialogue `Effect` (flag set/clear, reputation, items, quests,
    /// audio, etc.). Lets a world rule re-fire a dialogue effect without
    /// authoring a duplicate dispatcher branch.
    Effect(Effect),
    DespawnNamed(String),
    SetSpriteImage { name: String, image: String },
    StartDialogueScene(String),
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/// Author-facing handle for an entity. World-rule actions like
/// `DespawnNamed` and `SetSpriteImage` look up entities by this name.
#[derive(Component, Debug, Clone)]
pub struct Named(pub String);

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct WorldRuleCatalog {
    pub rules: Vec<WorldRule>,
    /// Index from rule id back into `rules`. Used by diagnostics.
    pub by_id: HashMap<String, usize>,
}

impl WorldRuleCatalog {
    fn rebuild_index(&mut self) {
        self.by_id.clear();
        for (i, rule) in self.rules.iter().enumerate() {
            self.by_id.insert(rule.id.clone(), i);
        }
    }
}

#[derive(Resource, Default)]
pub struct WorldRuleFireLog(pub HashSet<String>);

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

fn build_catalog() -> WorldRuleCatalog {
    let mut catalog = WorldRuleCatalog::default();
    let dir = Path::new(WORLD_RULES_DIR);
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            warn!("world_rules: cannot read {WORLD_RULES_DIR}: {err}");
            return catalog;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ron") {
            continue;
        }
        let contents = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) => {
                warn!("world_rules: read {} failed: {err}", path.display());
                continue;
            }
        };
        match ron::de::from_str::<Vec<WorldRule>>(&contents) {
            Ok(mut rules) => catalog.rules.append(&mut rules),
            Err(err) => warn!("world_rules: parse {} failed: {err}", path.display()),
        }
    }
    catalog.rebuild_index();
    info!("world_rules: loaded {} rule(s)", catalog.rules.len());
    catalog
}

// ---------------------------------------------------------------------------
// Trigger evaluation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum IncomingTrigger {
    FlagSet(String),
    FlagCleared(String),
    AreaEntered { to: u16 },
    DialogueChoicePicked { event_id: u32 },
    QuestCompleted { quest_id: u32 },
}

fn matches(trigger: &WorldTrigger, incoming: &IncomingTrigger) -> bool {
    match (trigger, incoming) {
        (WorldTrigger::FlagSet(a), IncomingTrigger::FlagSet(b)) => a == b,
        (WorldTrigger::FlagCleared(a), IncomingTrigger::FlagCleared(b)) => a == b,
        (WorldTrigger::AreaEntered { region_id }, IncomingTrigger::AreaEntered { to, .. }) => {
            *region_id == *to
        }
        (WorldTrigger::AnyAreaEntered, IncomingTrigger::AreaEntered { .. }) => true,
        (
            WorldTrigger::DialogueChoicePicked { event_id: a },
            IncomingTrigger::DialogueChoicePicked { event_id: b },
        ) => a == b,
        (
            WorldTrigger::QuestCompleted { quest_id: a },
            IncomingTrigger::QuestCompleted { quest_id: b },
        ) => a == b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// System: collect events, evaluate rules, dispatch actions
// ---------------------------------------------------------------------------

#[derive(SystemParam)]
pub struct WorldRuleEventReaders<'w, 's> {
    pub flags: MessageReader<'w, 's, FlagChangedEvent>,
    pub area: MessageReader<'w, 's, AreaChanged>,
    pub choice: MessageReader<'w, 's, DialogueChoicePickedEvent>,
    pub quest: MessageReader<'w, 's, QuestStatusChangedEvent>,
}

#[derive(SystemParam)]
pub struct WorldRuleMutationParams<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub asset_server: Res<'w, AssetServer>,
    pub named_q: Query<'w, 's, (Entity, &'static Named, Option<&'static mut Sprite>)>,
    pub runtime: ResMut<'w, DialogueRuntime>,
    pub catalog: Res<'w, DialogueCatalog>,
}

pub fn evaluate_world_rules(
    catalog: Res<WorldRuleCatalog>,
    mut fire_log: ResMut<WorldRuleFireLog>,
    mut readers: WorldRuleEventReaders,
    cond_ctx: ConditionContext,
    mut effects: EffectDispatcher,
    mut mutations: WorldRuleMutationParams,
) {
    // Collect every incoming trigger this frame, in a single owned Vec so the
    // borrows on the message buffers can be released before we start firing
    // dialogue effects (which themselves reach back into Messages).
    let mut incoming: Vec<IncomingTrigger> = Vec::new();
    for ev in readers.flags.read() {
        incoming.push(if ev.set {
            IncomingTrigger::FlagSet(ev.name.clone())
        } else {
            IncomingTrigger::FlagCleared(ev.name.clone())
        });
    }
    for ev in readers.area.read() {
        incoming.push(IncomingTrigger::AreaEntered { to: ev.to });
    }
    for ev in readers.choice.read() {
        incoming.push(IncomingTrigger::DialogueChoicePicked { event_id: ev.event_id });
    }
    for ev in readers.quest.read() {
        if matches!(ev.status, QuestStatus::Completed) {
            incoming.push(IncomingTrigger::QuestCompleted { quest_id: ev.quest_id });
        }
    }

    if incoming.is_empty() || catalog.rules.is_empty() {
        return;
    }

    for rule in &catalog.rules {
        if rule.once && fire_log.0.contains(&rule.id) {
            continue;
        }
        let trigger_matched = rule
            .triggers
            .iter()
            .any(|t| incoming.iter().any(|i| matches(t, i)));
        if !trigger_matched {
            continue;
        }
        if let Some(cond) = &rule.condition {
            if !evaluate_condition(cond, &cond_ctx) {
                continue;
            }
        }
        info!("world_rules: firing '{}'", rule.id);
        for action in &rule.actions {
            apply_action(action, &mut effects, &mut mutations);
        }
        if rule.once {
            fire_log.0.insert(rule.id.clone());
        }
    }
}

fn apply_action(
    action: &WorldRuleAction,
    effects: &mut EffectDispatcher,
    mutations: &mut WorldRuleMutationParams,
) {
    match action {
        WorldRuleAction::Effect(effect) => {
            effects.dispatch(effect);
        }
        WorldRuleAction::DespawnNamed(name) => {
            let mut despawned = false;
            for (entity, named, _) in mutations.named_q.iter() {
                if named.0 == *name {
                    mutations.commands.entity(entity).despawn();
                    despawned = true;
                }
            }
            if !despawned {
                debug!("world_rules: DespawnNamed '{name}' matched no Named entities");
            }
        }
        WorldRuleAction::SetSpriteImage { name, image } => {
            let mut updated = false;
            for (_, named, sprite) in mutations.named_q.iter_mut() {
                if named.0 != *name {
                    continue;
                }
                if let Some(mut sprite) = sprite {
                    sprite.image = mutations.asset_server.load(image.clone());
                    updated = true;
                }
            }
            if !updated {
                debug!(
                    "world_rules: SetSpriteImage '{name}' matched no Named+Sprite entities"
                );
            }
        }
        WorldRuleAction::StartDialogueScene(scene_id) => {
            if mutations.runtime.active {
                warn!(
                    "world_rules: StartDialogueScene '{scene_id}' skipped — dialogue already active"
                );
                return;
            }
            if !mutations.runtime.start(scene_id.clone(), &mutations.catalog) {
                warn!("world_rules: StartDialogueScene '{scene_id}' — scene not found");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity check: every `.ron` file under `assets/data/world_rules/`
    /// parses cleanly. Catches typos in the snake_case enum tags before the
    /// game runs.
    #[test]
    fn world_rule_files_parse() {
        let dir = std::path::Path::new(WORLD_RULES_DIR);
        let mut total = 0usize;
        for entry in std::fs::read_dir(dir).expect("read world_rules dir").flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ron") {
                continue;
            }
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let parsed: Vec<WorldRule> = ron::de::from_str(&contents)
                .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            for rule in &parsed {
                assert!(
                    !rule.triggers.is_empty(),
                    "{}: rule '{}' has no triggers",
                    path.display(),
                    rule.id,
                );
                assert!(
                    !rule.actions.is_empty(),
                    "{}: rule '{}' has no actions",
                    path.display(),
                    rule.id,
                );
            }
            total += parsed.len();
        }
        assert!(total > 0, "no world rules found under {WORLD_RULES_DIR}");
    }
}

pub struct WorldRulesPlugin;

impl Plugin for WorldRulesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(build_catalog())
            .init_resource::<WorldRuleFireLog>()
            // Run after dialogue advance + scene player so any flag/area
            // change emitted this frame is visible to the rule evaluator.
            .add_systems(Update, evaluate_world_rules);
    }
}
