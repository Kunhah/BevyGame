//! Data-driven quest framework.
//!
//! A quest is a declarative bundle of:
//!   * **objectives** with typed [`ObjectiveKind`]s and progress counters,
//!   * **preconditions** that gate when the quest auto-offers, and
//!   * **rewards** that fire on completion.
//!
//! The full definition catalogue lives in `assets/data/quests.ron` and is
//! loaded into a [`QuestRegistry`] resource at startup. The runtime
//! [`QuestLog`] tracks per-quest, per-objective progress; an auto-offer
//! system activates quests once their preconditions are met. Game events
//! (kills, area changes, dialogue completion, reputation changes) flow into
//! advance dispatchers that bump matching objectives.
//!
//! Existing per-entity hook components (`OnItemPickup`, `OnDeath`, `OnReach`)
//! still work and are kept for one-off scripted triggers — they call
//! [`UpdateObjectiveEvent`] just like the old version.

use std::collections::{HashMap, HashSet};
use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::battle::EnemyEncounter;
use crate::combat_plugin::{DeathEvent, Experience, Level};
use crate::core::Player;
use crate::dialogue::DialogueRuntime;
use crate::governance::{ReputationChangeEvent, ReputationLedger, ReputationTarget};
use crate::map::AreaChanged;

const QUEST_REGISTRY_PATH: &str = "assets/data/quests.ron";

// ---------------------------------------------------------------------------
// Authoring types (deserialised from RON)
// ---------------------------------------------------------------------------

/// What event advances an objective. Each variant is matched against incoming
/// game events; matching variants increment the objective's progress counter
/// (or set it to its `required` value for binary kinds).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectiveKind {
    /// Advances by 1 each time an enemy dies. `enemy_id: None` matches any.
    Kill { enemy_id: Option<u32> },
    /// Advances when the player enters the named area (location id).
    Reach { area_id: u16 },
    /// Advances when a dialogue ends with the named id as the last node.
    Talk { dialogue_id: String },
    /// Advances when the player picks a choice carrying this event id.
    DialogueChoice { event_id: u32 },
    /// Auto-completes when reputation with the target is at or above the
    /// threshold. Re-checked every reputation change.
    ReputationAtLeast {
        target: ReputationTarget,
        threshold: i32,
    },
    /// Hand-fired progress: external code calls `advance_manual_flag`.
    ManualFlag { tag: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectiveDefinition {
    pub id: u32,
    pub description: String,
    pub kind: ObjectiveKind,
    /// How many advance events are needed to complete. 1 for binary
    /// objectives like `Reach` / `Talk`.
    #[serde(default = "default_required")]
    pub required: u32,
}

fn default_required() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuestPrecondition {
    QuestCompleted(u32),
    ReputationAtLeast {
        target: ReputationTarget,
        threshold: i32,
    },
    PlayerLevelAtLeast(u32),
    FlagSet(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuestReward {
    Experience(u32),
    Reputation { target: ReputationTarget, delta: i16 },
    Flag(String),
    /// Convenience: chain into another quest by id (auto-offer it after
    /// completion if its preconditions are met).
    UnlockQuest(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestDefinition {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub objectives: Vec<ObjectiveDefinition>,
    #[serde(default)]
    pub preconditions: Vec<QuestPrecondition>,
    #[serde(default)]
    pub rewards: Vec<QuestReward>,
    /// If true, the quest activates automatically as soon as its
    /// preconditions are satisfied. If false, the quest must be added
    /// manually via [`AddQuestEvent`].
    #[serde(default)]
    pub auto_offer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuestCatalog {
    pub quests: Vec<QuestDefinition>,
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestStatus {
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestObjective {
    pub id: u32,
    pub description: String,
    pub kind: ObjectiveKind,
    pub progress: u32,
    pub required: u32,
    pub failed: bool,
}

impl QuestObjective {
    pub fn is_complete(&self) -> bool {
        self.progress >= self.required && !self.failed
    }

    /// Bump progress by `delta`, clamped to `required`.
    pub fn advance(&mut self, delta: u32) {
        self.progress = self.progress.saturating_add(delta).min(self.required);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quest {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub objectives: Vec<QuestObjective>,
    pub status: QuestStatus,
    pub rewards: Vec<QuestReward>,
}

impl Quest {
    fn recalc_status(&mut self) {
        if self.objectives.iter().any(|o| o.failed) {
            self.status = QuestStatus::Failed;
            return;
        }
        if self.objectives.iter().all(|o| o.is_complete()) {
            self.status = QuestStatus::Completed;
            return;
        }
        self.status = QuestStatus::Active;
    }
}

#[derive(Resource, Default)]
pub struct QuestRegistry(pub HashMap<u32, QuestDefinition>);

#[derive(Resource, Default, Clone, Debug, Serialize, Deserialize)]
pub struct QuestLog {
    pub quests: HashMap<u32, Quest>,
    pub offered: HashSet<u32>, // quest ids that have been auto-offered already
    pub completed: HashSet<u32>,
    pub failed: HashSet<u32>,
}

#[derive(Resource, Default)]
pub struct QuestFlags(pub HashSet<String>);

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Event, Message, Debug, Clone)]
pub struct AddQuestEvent {
    pub quest_id: u32,
}

#[derive(Event, Message, Debug, Clone)]
pub struct AdvanceObjectiveEvent {
    pub quest_id: u32,
    pub objective_id: u32,
    pub delta: u32,
}

#[derive(Event, Message, Debug, Clone)]
pub struct UpdateObjectiveEvent {
    pub quest_id: u32,
    pub objective_id: u32,
    pub failed: bool,
}

#[derive(Event, Message, Debug, Clone, Copy)]
pub struct QuestStatusChangedEvent {
    pub quest_id: u32,
    pub status: QuestStatus,
}

#[derive(Event, Message, Debug, Clone)]
pub struct ItemPickupEvent {
    pub entity: Entity,
}

#[derive(Event, Message, Debug, Clone)]
pub struct DialogueCompletedEvent {
    pub dialogue_id: String,
}

#[derive(Event, Message, Debug, Clone)]
pub struct DialogueChoicePickedEvent {
    pub event_id: u32,
}

#[derive(Event, Message, Debug, Clone)]
pub struct QuestRewardGrantedEvent {
    pub quest_id: u32,
    pub reward: QuestReward,
}

#[derive(Event, Message, Debug, Clone)]
pub struct ManualFlagEvent {
    pub tag: String,
}

// ---------------------------------------------------------------------------
// Hook components — preserved for entity-specific scripted triggers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct QuestUpdateAction {
    pub quest_id: u32,
    pub objective_id: u32,
    pub delta: u32,
}

#[derive(Component, Default)]
pub struct OnItemPickup(pub Option<QuestUpdateAction>);

#[derive(Component, Default)]
pub struct OnDeath(pub Option<QuestUpdateAction>);

#[derive(Component)]
pub struct OnReach {
    pub radius: f32,
    pub action: Option<QuestUpdateAction>,
    pub fired: bool,
}

impl Default for OnReach {
    fn default() -> Self {
        Self {
            radius: 48.0,
            action: None,
            fired: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

fn load_quest_registry(mut registry: ResMut<QuestRegistry>) {
    let text = match fs::read_to_string(QUEST_REGISTRY_PATH) {
        Ok(t) => t,
        Err(err) => {
            warn!("Failed to read {QUEST_REGISTRY_PATH}: {err}");
            return;
        }
    };
    let catalog: QuestCatalog = match ron::de::from_str(&text) {
        Ok(c) => c,
        Err(err) => {
            warn!("Failed to parse {QUEST_REGISTRY_PATH}: {err}");
            return;
        }
    };
    for def in catalog.quests {
        registry.0.insert(def.id, def);
    }
    info!("Loaded {} quest definition(s)", registry.0.len());
}

// ---------------------------------------------------------------------------
// Activation: turn definitions into runtime quests
// ---------------------------------------------------------------------------

fn instantiate_quest(def: &QuestDefinition) -> Quest {
    let objectives = def
        .objectives
        .iter()
        .map(|o| QuestObjective {
            id: o.id,
            description: o.description.clone(),
            kind: o.kind.clone(),
            progress: 0,
            required: o.required.max(1),
            failed: false,
        })
        .collect();
    Quest {
        id: def.id,
        title: def.title.clone(),
        description: def.description.clone(),
        objectives,
        status: QuestStatus::Active,
        rewards: def.rewards.clone(),
    }
}

fn check_preconditions(
    pre: &[QuestPrecondition],
    log: &QuestLog,
    flags: &QuestFlags,
    rep: &ReputationLedger,
    player_level: u32,
) -> bool {
    pre.iter().all(|p| match p {
        QuestPrecondition::QuestCompleted(id) => log.completed.contains(id),
        QuestPrecondition::ReputationAtLeast { target, threshold } => {
            current_reputation(rep, target) >= *threshold
        }
        QuestPrecondition::PlayerLevelAtLeast(level) => player_level >= *level,
        QuestPrecondition::FlagSet(name) => flags.0.contains(name),
    })
}

fn current_reputation(rep: &ReputationLedger, target: &ReputationTarget) -> i32 {
    match target {
        ReputationTarget::Governor { city_id } => rep.get_governor(*city_id),
        ReputationTarget::Merchant { merchant_id } => rep.get_merchant(*merchant_id),
        ReputationTarget::Clan { clan_name } => rep.get_clan(clan_name),
    }
}

fn auto_offer_eligible_quests(
    registry: Res<QuestRegistry>,
    mut log: ResMut<QuestLog>,
    flags: Res<QuestFlags>,
    rep: Res<ReputationLedger>,
    player_level_q: Query<&Level, With<Player>>,
) {
    let player_level = player_level_q
        .iter()
        .next()
        .map(|level| level.0)
        .unwrap_or(0);

    for (id, def) in registry.0.iter() {
        if !def.auto_offer {
            continue;
        }
        if log.offered.contains(id)
            || log.completed.contains(id)
            || log.quests.contains_key(id)
        {
            continue;
        }
        if !check_preconditions(&def.preconditions, &log, &flags, &rep, player_level) {
            continue;
        }
        log.quests.insert(*id, instantiate_quest(def));
        log.offered.insert(*id);
        info!("Auto-offered quest {id}: {}", def.title);
    }
}

fn ingest_add_quest_events(
    mut events: ResMut<Messages<AddQuestEvent>>,
    registry: Res<QuestRegistry>,
    mut log: ResMut<QuestLog>,
) {
    for event in events.drain() {
        if log.quests.contains_key(&event.quest_id) || log.completed.contains(&event.quest_id) {
            continue;
        }
        let Some(def) = registry.0.get(&event.quest_id) else {
            warn!("AddQuestEvent for unknown quest id {}", event.quest_id);
            continue;
        };
        log.quests.insert(event.quest_id, instantiate_quest(def));
        log.offered.insert(event.quest_id);
    }
}

// ---------------------------------------------------------------------------
// Progress dispatchers — funnel game events into objective progress
// ---------------------------------------------------------------------------

fn advance_for(
    log: &mut QuestLog,
    delta: u32,
    matches: impl Fn(&ObjectiveKind) -> bool,
) -> Vec<(u32, QuestStatus)> {
    let mut status_changes = Vec::new();
    for quest in log.quests.values_mut() {
        if quest.status != QuestStatus::Active {
            continue;
        }
        let prev = quest.status;
        let mut any = false;
        for objective in quest.objectives.iter_mut() {
            if objective.failed || objective.is_complete() {
                continue;
            }
            if matches(&objective.kind) {
                objective.advance(delta);
                any = true;
            }
        }
        if any {
            quest.recalc_status();
            if quest.status != prev {
                status_changes.push((quest.id, quest.status));
            }
        }
    }
    status_changes
}

fn dispatch_kill_progress(
    mut deaths: MessageReader<DeathEvent>,
    enemy_encounters: Query<&EnemyEncounter>,
    mut log: ResMut<QuestLog>,
    mut status_writer: MessageWriter<QuestStatusChangedEvent>,
) {
    for ev in deaths.read() {
        let enemy_id = enemy_encounters.get(ev.entity).ok().map(|e| e.id);
        let changes = advance_for(&mut log, 1, |kind| match kind {
            ObjectiveKind::Kill { enemy_id: None } => true,
            ObjectiveKind::Kill { enemy_id: Some(id) } => Some(*id) == enemy_id,
            _ => false,
        });
        for (quest_id, status) in changes {
            status_writer.write(QuestStatusChangedEvent { quest_id, status });
        }
    }
}

fn dispatch_area_progress(
    mut areas: MessageReader<AreaChanged>,
    mut log: ResMut<QuestLog>,
    mut status_writer: MessageWriter<QuestStatusChangedEvent>,
) {
    for ev in areas.read() {
        let to = ev.to;
        let changes = advance_for(&mut log, 1, |kind| {
            matches!(kind, ObjectiveKind::Reach { area_id } if *area_id == to)
        });
        for (quest_id, status) in changes {
            status_writer.write(QuestStatusChangedEvent { quest_id, status });
        }
    }
}

fn dispatch_dialogue_progress(
    mut completed: MessageReader<DialogueCompletedEvent>,
    mut chose: MessageReader<DialogueChoicePickedEvent>,
    mut log: ResMut<QuestLog>,
    mut status_writer: MessageWriter<QuestStatusChangedEvent>,
) {
    for ev in completed.read() {
        let id = ev.dialogue_id.clone();
        let changes = advance_for(&mut log, 1, |kind| {
            matches!(kind, ObjectiveKind::Talk { dialogue_id } if *dialogue_id == id)
        });
        for (quest_id, status) in changes {
            status_writer.write(QuestStatusChangedEvent { quest_id, status });
        }
    }
    for ev in chose.read() {
        let event_id = ev.event_id;
        let changes = advance_for(&mut log, 1, |kind| {
            matches!(kind, ObjectiveKind::DialogueChoice { event_id: e } if *e == event_id)
        });
        for (quest_id, status) in changes {
            status_writer.write(QuestStatusChangedEvent { quest_id, status });
        }
    }
}

fn dispatch_reputation_progress(
    mut events: MessageReader<ReputationChangeEvent>,
    rep: Res<ReputationLedger>,
    mut log: ResMut<QuestLog>,
    mut status_writer: MessageWriter<QuestStatusChangedEvent>,
) {
    if events.is_empty() {
        return;
    }
    // Drain so we don't re-process; we re-check thresholds based on the
    // current ledger snapshot regardless of which target moved.
    for _ in events.read() {}

    let mut status_changes = Vec::new();
    for quest in log.quests.values_mut() {
        if quest.status != QuestStatus::Active {
            continue;
        }
        let prev = quest.status;
        let mut any = false;
        for objective in quest.objectives.iter_mut() {
            if objective.failed || objective.is_complete() {
                continue;
            }
            if let ObjectiveKind::ReputationAtLeast { target, threshold } = &objective.kind {
                if current_reputation(&rep, target) >= *threshold {
                    objective.progress = objective.required;
                    any = true;
                }
            }
        }
        if any {
            quest.recalc_status();
            if quest.status != prev {
                status_changes.push((quest.id, quest.status));
            }
        }
    }
    for (quest_id, status) in status_changes {
        status_writer.write(QuestStatusChangedEvent { quest_id, status });
    }
}

fn dispatch_manual_flag_progress(
    mut events: MessageReader<ManualFlagEvent>,
    mut log: ResMut<QuestLog>,
    mut status_writer: MessageWriter<QuestStatusChangedEvent>,
) {
    for ev in events.read() {
        let tag = ev.tag.clone();
        let changes = advance_for(&mut log, 1, |kind| {
            matches!(kind, ObjectiveKind::ManualFlag { tag: t } if *t == tag)
        });
        for (quest_id, status) in changes {
            status_writer.write(QuestStatusChangedEvent { quest_id, status });
        }
    }
}

// ---------------------------------------------------------------------------
// Hook components and explicit advance/update events
// ---------------------------------------------------------------------------

fn advance_specific_objective(
    log: &mut QuestLog,
    quest_id: u32,
    objective_id: u32,
    delta: u32,
) -> Option<QuestStatus> {
    let quest = log.quests.get_mut(&quest_id)?;
    if quest.status != QuestStatus::Active {
        return None;
    }
    let prev = quest.status;
    let objective = quest.objectives.iter_mut().find(|o| o.id == objective_id)?;
    if objective.failed || objective.is_complete() {
        return None;
    }
    objective.advance(delta);
    quest.recalc_status();
    if quest.status != prev {
        Some(quest.status)
    } else {
        None
    }
}

fn ingest_advance_events(
    mut events: ResMut<Messages<AdvanceObjectiveEvent>>,
    mut log: ResMut<QuestLog>,
    mut status_writer: ResMut<Messages<QuestStatusChangedEvent>>,
) {
    for ev in events.drain() {
        if let Some(status) =
            advance_specific_objective(&mut log, ev.quest_id, ev.objective_id, ev.delta.max(1))
        {
            status_writer.write(QuestStatusChangedEvent {
                quest_id: ev.quest_id,
                status,
            });
        }
    }
}

fn ingest_update_events(
    mut events: ResMut<Messages<UpdateObjectiveEvent>>,
    mut log: ResMut<QuestLog>,
    mut status_writer: ResMut<Messages<QuestStatusChangedEvent>>,
) {
    for ev in events.drain() {
        let Some(quest) = log.quests.get_mut(&ev.quest_id) else {
            continue;
        };
        if quest.status != QuestStatus::Active {
            continue;
        }
        let prev = quest.status;
        let Some(objective) = quest.objectives.iter_mut().find(|o| o.id == ev.objective_id)
        else {
            continue;
        };
        if ev.failed {
            objective.failed = true;
        } else {
            objective.progress = objective.required;
        }
        quest.recalc_status();
        if quest.status != prev {
            status_writer.write(QuestStatusChangedEvent {
                quest_id: ev.quest_id,
                status: quest.status,
            });
        }
    }
}

fn trigger_on_item_pickup(
    mut pickup_events: ResMut<Messages<ItemPickupEvent>>,
    hooks: Query<&OnItemPickup>,
    mut updates: ResMut<Messages<AdvanceObjectiveEvent>>,
) {
    for event in pickup_events.drain() {
        if let Ok(OnItemPickup(Some(action))) = hooks.get(event.entity) {
            updates.write(AdvanceObjectiveEvent {
                quest_id: action.quest_id,
                objective_id: action.objective_id,
                delta: action.delta,
            });
        }
    }
}

fn trigger_on_death(
    mut death_events: MessageReader<DeathEvent>,
    hooks: Query<&OnDeath>,
    mut updates: ResMut<Messages<AdvanceObjectiveEvent>>,
) {
    for event in death_events.read() {
        if let Ok(OnDeath(Some(action))) = hooks.get(event.entity) {
            updates.write(AdvanceObjectiveEvent {
                quest_id: action.quest_id,
                objective_id: action.objective_id,
                delta: action.delta,
            });
        }
    }
}

fn trigger_on_reach(
    player: Query<&Transform, With<Player>>,
    mut hooks: Query<(&Transform, &mut OnReach)>,
    mut updates: ResMut<Messages<AdvanceObjectiveEvent>>,
) {
    let Ok(player_tf) = player.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();
    for (tf, mut hook) in hooks.iter_mut() {
        if hook.fired {
            continue;
        }
        if let Some(action) = hook.action {
            let pos = tf.translation.truncate();
            if player_pos.distance(pos) <= hook.radius {
                updates.write(AdvanceObjectiveEvent {
                    quest_id: action.quest_id,
                    objective_id: action.objective_id,
                    delta: action.delta,
                });
                hook.fired = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reward fulfillment
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn fulfill_quest_rewards(
    mut events: MessageReader<QuestStatusChangedEvent>,
    mut log: ResMut<QuestLog>,
    mut flags: ResMut<QuestFlags>,
    mut rep_ledger: ResMut<ReputationLedger>,
    mut rep_writer: MessageWriter<ReputationChangeEvent>,
    mut player_xp: Query<&mut Experience, With<Player>>,
    mut granted: MessageWriter<QuestRewardGrantedEvent>,
    mut add_writer: MessageWriter<AddQuestEvent>,
) {
    for ev in events.read() {
        match ev.status {
            QuestStatus::Completed => {
                log.completed.insert(ev.quest_id);
                let rewards = log
                    .quests
                    .get(&ev.quest_id)
                    .map(|q| q.rewards.clone())
                    .unwrap_or_default();
                for reward in rewards {
                    grant_reward(
                        &reward,
                        ev.quest_id,
                        &mut flags,
                        &mut rep_ledger,
                        &mut rep_writer,
                        &mut player_xp,
                        &mut add_writer,
                    );
                    granted.write(QuestRewardGrantedEvent {
                        quest_id: ev.quest_id,
                        reward,
                    });
                }
                info!("Quest {} completed", ev.quest_id);
            }
            QuestStatus::Failed => {
                log.failed.insert(ev.quest_id);
                info!("Quest {} failed", ev.quest_id);
            }
            QuestStatus::Active => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn grant_reward(
    reward: &QuestReward,
    quest_id: u32,
    flags: &mut QuestFlags,
    rep_ledger: &mut ReputationLedger,
    rep_writer: &mut MessageWriter<ReputationChangeEvent>,
    player_xp: &mut Query<&mut Experience, With<Player>>,
    add_writer: &mut MessageWriter<AddQuestEvent>,
) {
    match reward {
        QuestReward::Experience(amount) => {
            for mut xp in player_xp.iter_mut() {
                xp.0 = xp.0.saturating_add(*amount);
            }
        }
        QuestReward::Reputation { target, delta } => {
            rep_ledger.apply_delta(target, *delta);
            rep_writer.write(ReputationChangeEvent {
                target: target.clone(),
                delta: *delta,
                reason: format!("quest_{quest_id}_reward"),
            });
        }
        QuestReward::Flag(name) => {
            flags.0.insert(name.clone());
        }
        QuestReward::UnlockQuest(next_id) => {
            add_writer.write(AddQuestEvent { quest_id: *next_id });
        }
    }
}

// ---------------------------------------------------------------------------
// Dialogue watchers — turn dialogue-state transitions into quest events
// without surgery on the existing dialogue systems.
// ---------------------------------------------------------------------------

fn watch_dialogue_completion(
    runtime: Res<DialogueRuntime>,
    mut last_node: Local<Option<String>>,
    mut writer: MessageWriter<DialogueCompletedEvent>,
) {
    let current = runtime.current_node.clone();
    if let (Some(prev), None) = (last_node.as_ref(), current.as_ref()) {
        // Existing quest data targets the legacy node id (e.g. "The last
        // goodbye 24"), which equals the current runtime node id since the
        // compat shim preserves ids during conversion.
        writer.write(DialogueCompletedEvent {
            dialogue_id: prev.clone(),
        });
    }
    *last_node = current;
}

// ---------------------------------------------------------------------------
// Hunts (Merchant-issued quests)
// ---------------------------------------------------------------------------

/// A constraint attached to a hunt. Each hunt carries a list of these so
/// designers can author varied hunt rules — "kill exactly this yokai", "but
/// don't trip the village alarm", "and finish before the next dawn".
///
/// The shipped `Hunt` already pins the primary `target_enemy_id` and
/// `deadline_timestamp`; `HuntCondition` is for additional per-hunt rules
/// beyond those defaults.
#[derive(Debug, Clone)]
#[allow(dead_code)] // NoFlag / DeadlineTicks wired progressively as content lands
pub enum HuntCondition {
    /// The hunt completes when this encounter id dies in battle. The hunt's
    /// `target_enemy_id` is the implicit primary `KillEnemy`; additional
    /// `KillEnemy` entries demand multi-target hunts.
    KillEnemy { encounter_id: u32 },
    /// Hunt fails if this story flag becomes set during the hunt window.
    NoFlag(String),
    /// Hunt fails if not completed by `Timestamp.0 == ticks`. Independent
    /// of the `Hunt.deadline_timestamp` neglect deadline (which is a soft
    /// "one week then forfeit" timer).
    DeadlineTicks(u32),
}

/// Hunt-specific metadata layered on top of a regular Quest. The objective(s)
/// live in the parent `Quest` (typically a `Kill` objective on the target
/// yokai); this struct only holds Merchant-related fields.
#[derive(Debug, Clone)]
pub struct Hunt {
    pub quest_id: u32,
    pub target_enemy_id: u32,
    pub deadline_timestamp: u32,
    /// Coin reward in Merchant Coins (the "favor" currency from GDD Rule V).
    pub coin_reward: u32,
    /// Per GDD Rule X: if the target is beyond the bound's reach, the
    /// Merchant warns plainly. Set true on assignment when the target's
    /// difficulty exceeds an internal threshold.
    pub warning_issued: bool,
    /// Set to a specific bound character when the hunt is assigned to one;
    /// drives whose performance changes on completion / failure / neglect.
    pub assigned_to: Option<Entity>,
    /// Optional dialogue-scene id played when the player approaches the
    /// hunt target world entity. The cutscene runs first, then combat
    /// starts via `start_pending_hunt_battle`. `None` means combat starts
    /// directly on approach.
    pub pre_battle_scene: Option<String>,
    /// Per-hunt rules beyond the implicit "kill the target" + neglect
    /// deadline. See [`HuntCondition`].
    pub conditions: Vec<HuntCondition>,
}

#[derive(Resource, Default)]
pub struct HuntRegistry(pub HashMap<u32, Hunt>);

#[derive(Resource, Default)]
pub struct MerchantCoins(pub u32);

#[derive(Debug, Clone, Message)]
pub struct HuntAssignedEvent {
    pub quest_id: u32,
    pub assigned_to: Option<Entity>,
}

#[derive(Debug, Clone, Message)]
pub struct HuntCompletedEvent {
    pub quest_id: u32,
    pub completer: Option<Entity>,
    pub coin_reward: u32,
    /// True if completed after deadline. Used by Contract Rule VI (Stolen
    /// Hours) to apply the late-completion debuff.
    pub completed_late: bool,
}

#[derive(Debug, Clone, Message)]
pub struct HuntFailedEvent {
    pub quest_id: u32,
    pub assigned_to: Option<Entity>,
    /// True if the hunt was abandoned past Rule IV's "one week" threshold,
    /// triggering Deny the Contract.
    pub neglected: bool,
}

/// Convenience helper used by external systems (Merchant NPC, debug console,
/// generated content): registers a Quest + its Hunt sidecar.
pub fn register_hunt(
    log: &mut QuestLog,
    hunts: &mut HuntRegistry,
    quest: Quest,
    hunt: Hunt,
) {
    let id = quest.id;
    log.quests.insert(id, quest);
    hunts.0.insert(id, hunt);
}

/// Watches the global Timestamp; once a hunt's deadline passes plus the GDD's
/// one-week neglect grace, fails the hunt as "neglected" so the Contract
/// enforcer can apply Deny the Contract.
pub fn process_hunt_deadlines(
    timestamp: Res<crate::core::Timestamp>,
    log: Res<QuestLog>,
    hunts: Res<HuntRegistry>,
    mut writer: MessageWriter<HuntFailedEvent>,
) {
    let now = timestamp.0;
    let neglect_grace = 7 * 24 * crate::constants::TIMESTAMP_TICKS_PER_HOUR;
    for (quest_id, hunt) in hunts.0.iter() {
        let Some(quest) = log.quests.get(quest_id) else {
            continue;
        };
        if quest.status != QuestStatus::Active {
            continue;
        }
        let auto_fail_at = hunt.deadline_timestamp.saturating_add(neglect_grace);
        if now >= auto_fail_at {
            writer.write(HuntFailedEvent {
                quest_id: *quest_id,
                assigned_to: hunt.assigned_to,
                neglected: true,
            });
        }
    }
}

/// Detect when a hunt target dies in battle and emit `HuntCompletedEvent`.
///
/// The implicit completion rule is "the active battle's enemy id matches a
/// `Hunt.target_enemy_id`". The `KillEnemy` entries in
/// `Hunt.conditions` are the multi-target extension (any one matching id
/// counts).
///
/// `NoFlag` and `DeadlineTicks` failure conditions are checked by their own
/// systems (`fail_active_hunt_on_no_flag`, `fail_active_hunt_on_deadline`).
pub fn detect_hunt_completion_on_enemy_death(
    mut deaths: MessageReader<crate::combat_plugin::DeathEvent>,
    participants_q: Query<
        &crate::battle::BattleSide,
        With<crate::battle::BattleParticipant>,
    >,
    battle_state: Res<crate::battle::BattleState>,
    hunts: Res<HuntRegistry>,
    log: Res<QuestLog>,
    timestamp: Res<crate::core::Timestamp>,
    mut writer: MessageWriter<HuntCompletedEvent>,
) {
    let Some(active_id) = battle_state.enemy_id else {
        return;
    };
    for ev in deaths.read() {
        let Ok(side) = participants_q.get(ev.entity) else {
            continue;
        };
        if !matches!(side, crate::battle::BattleSide::Enemy) {
            continue;
        }
        for hunt in hunts.0.values() {
            let Some(quest) = log.quests.get(&hunt.quest_id) else {
                continue;
            };
            if !matches!(quest.status, QuestStatus::Active) {
                continue;
            }
            let primary_match = hunt.target_enemy_id == active_id;
            let extra_match = hunt.conditions.iter().any(|c| {
                matches!(
                    c,
                    HuntCondition::KillEnemy { encounter_id } if *encounter_id == active_id
                )
            });
            if !(primary_match || extra_match) {
                continue;
            }
            writer.write(HuntCompletedEvent {
                quest_id: hunt.quest_id,
                completer: ev.killer,
                coin_reward: hunt.coin_reward,
                completed_late: timestamp.0 > hunt.deadline_timestamp,
            });
        }
    }
}

/// `NoFlag` condition: hunt fails if a forbidden flag becomes set during
/// the hunt window.
pub fn fail_active_hunt_on_no_flag(
    mut events: MessageReader<crate::story_flags::FlagChangedEvent>,
    hunts: Res<HuntRegistry>,
    log: Res<QuestLog>,
    mut writer: MessageWriter<HuntFailedEvent>,
) {
    for ev in events.read() {
        if !ev.set {
            continue;
        }
        for hunt in hunts.0.values() {
            let Some(quest) = log.quests.get(&hunt.quest_id) else {
                continue;
            };
            if !matches!(quest.status, QuestStatus::Active) {
                continue;
            }
            for cond in &hunt.conditions {
                if let HuntCondition::NoFlag(name) = cond {
                    if name == &ev.name {
                        writer.write(HuntFailedEvent {
                            quest_id: hunt.quest_id,
                            assigned_to: hunt.assigned_to,
                            neglected: false,
                        });
                    }
                }
            }
        }
    }
}

/// `DeadlineTicks` condition: hunt fails when `Timestamp` passes the
/// per-condition deadline (independent of the soft neglect deadline).
pub fn fail_active_hunt_on_deadline(
    timestamp: Res<crate::core::Timestamp>,
    hunts: Res<HuntRegistry>,
    log: Res<QuestLog>,
    mut writer: MessageWriter<HuntFailedEvent>,
) {
    let now = timestamp.0;
    for hunt in hunts.0.values() {
        let Some(quest) = log.quests.get(&hunt.quest_id) else {
            continue;
        };
        if !matches!(quest.status, QuestStatus::Active) {
            continue;
        }
        for cond in &hunt.conditions {
            if let HuntCondition::DeadlineTicks(deadline) = cond {
                if now >= *deadline {
                    writer.write(HuntFailedEvent {
                        quest_id: hunt.quest_id,
                        assigned_to: hunt.assigned_to,
                        neglected: false,
                    });
                }
            }
        }
    }
}

/// On hunt completion, award coins and update the assignee's resurrection
/// standing.
pub fn handle_hunt_completion_system(
    mut reader: MessageReader<HuntCompletedEvent>,
    mut coins: ResMut<MerchantCoins>,
    mut q: Query<&mut crate::combat_plugin::ResurrectionStanding>,
) {
    for ev in reader.read() {
        coins.0 = coins.0.saturating_add(ev.coin_reward);
        let Some(who) = ev.completer else {
            continue;
        };
        if let Ok(mut standing) = q.get_mut(who) {
            standing.hunts_completed = standing.hunts_completed.saturating_add(1);
            standing.score = if ev.completed_late {
                standing.score.saturating_add(15)
            } else {
                standing.score.saturating_add(30)
            };
        }
    }
}

/// On hunt failure, penalize the assignee's standing.
pub fn handle_hunt_failure_system(
    mut reader: MessageReader<HuntFailedEvent>,
    mut q: Query<&mut crate::combat_plugin::ResurrectionStanding>,
) {
    for ev in reader.read() {
        let Some(who) = ev.assigned_to else {
            continue;
        };
        if let Ok(mut standing) = q.get_mut(who) {
            standing.hunts_failed = standing.hunts_failed.saturating_add(1);
            standing.score = standing
                .score
                .saturating_sub(if ev.neglected { 60 } else { 25 });
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct QuestPlugin;

impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestRegistry>()
            .init_resource::<QuestLog>()
            .init_resource::<QuestFlags>()
            .init_resource::<HuntRegistry>()
            .init_resource::<MerchantCoins>()
            .add_message::<AddQuestEvent>()
            .add_message::<AdvanceObjectiveEvent>()
            .add_message::<UpdateObjectiveEvent>()
            .add_message::<QuestStatusChangedEvent>()
            .add_message::<ItemPickupEvent>()
            .add_message::<DialogueCompletedEvent>()
            .add_message::<DialogueChoicePickedEvent>()
            .add_message::<ManualFlagEvent>()
            .add_message::<QuestRewardGrantedEvent>()
            .add_message::<HuntAssignedEvent>()
            .add_message::<HuntCompletedEvent>()
            .add_message::<HuntFailedEvent>()
            .add_systems(Startup, load_quest_registry)
            .add_systems(
                Update,
                (
                    auto_offer_eligible_quests,
                    ingest_add_quest_events,
                    ingest_advance_events,
                    ingest_update_events,
                    dispatch_kill_progress,
                    dispatch_area_progress,
                    dispatch_dialogue_progress,
                    dispatch_reputation_progress,
                    dispatch_manual_flag_progress,
                    trigger_on_item_pickup,
                    trigger_on_death,
                    trigger_on_reach,
                    watch_dialogue_completion,
                    fulfill_quest_rewards,
                    process_hunt_deadlines,
                    detect_hunt_completion_on_enemy_death,
                    fail_active_hunt_on_no_flag,
                    fail_active_hunt_on_deadline,
                    handle_hunt_completion_system,
                    handle_hunt_failure_system,
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_quests_parse() {
        let text = std::fs::read_to_string(QUEST_REGISTRY_PATH)
            .expect("quests.ron exists at the documented path");
        let catalog: QuestCatalog =
            ron::de::from_str(&text).expect("quests.ron deserialises into QuestCatalog");
        assert!(!catalog.quests.is_empty(), "expected at least one quest");
        for quest in &catalog.quests {
            assert!(!quest.objectives.is_empty(), "quest {} has no objectives", quest.id);
        }
    }
}
