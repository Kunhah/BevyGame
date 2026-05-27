use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::prelude::Messages;

use crate::city_data::CityCatalog;
use crate::combat_plugin::Bound;
use crate::core::Player;
use crate::economy::{InventoryStack, Merchants, PlayerInventory, PlayerWallet};
use crate::governance::{ReputationChangeEvent, ReputationLedger, ReputationTarget};
use crate::map::CurrentArea;
use crate::quests::{
    AddQuestEvent, AdvanceObjectiveEvent, QuestLog, QuestStatus,
};
use crate::story_flags::{FlagChangedEvent, StoryFlags};

use super::schema::{
    Condition, DialogueNode, DialogueScene, Effect, NodeId, QuestStatusFilter, ReputationTargetRef,
    SceneId,
};
use super::scene_player::ScenePlayback;
use super::ui::Interactable;

#[derive(Resource, Default)]
pub struct DialogueCatalog {
    pub scenes: HashMap<SceneId, DialogueScene>,
    /// First-match index from node id back to its owning scene id. Lets the
    /// runtime resolve `Interactable.dialogue_id` strings that came from the
    /// pre-schema flat catalog (where they were node ids, not scene ids).
    pub node_index: HashMap<NodeId, SceneId>,
}

impl DialogueCatalog {
    pub fn rebuild_node_index(&mut self) {
        self.node_index.clear();
        for (scene_id, scene) in &self.scenes {
            for node_id in scene.nodes.keys() {
                self.node_index
                    .entry(node_id.clone())
                    .or_insert_with(|| scene_id.clone());
            }
        }
    }
}

/// Runtime state for the active dialogue. Replaces the legacy `Dialogue_State`.
#[derive(Resource, Default)]
pub struct DialogueRuntime {
    pub active: bool,
    pub just_spawned: bool,
    pub current_scene: Option<SceneId>,
    pub current_node: Option<NodeId>,
}

impl DialogueRuntime {
    pub fn current_scene<'a>(&self, catalog: &'a DialogueCatalog) -> Option<&'a DialogueScene> {
        catalog.scenes.get(self.current_scene.as_ref()?)
    }

    pub fn current_node<'a>(&self, catalog: &'a DialogueCatalog) -> Option<&'a DialogueNode> {
        let scene = self.current_scene(catalog)?;
        scene.nodes.get(self.current_node.as_ref()?)
    }

    /// Start a dialogue from a `target` that may be either a scene id (start
    /// at the scene's declared `start` node) or a node id (start at that node
    /// inside whatever scene contains it). The dual lookup keeps the
    /// `Interactable` data — which references node ids in the pre-schema
    /// flat catalog — working unchanged.
    pub fn start(&mut self, target: String, catalog: &DialogueCatalog) -> bool {
        if let Some(scene) = catalog.scenes.get(&target) {
            self.current_node = Some(scene.start.clone());
            self.current_scene = Some(target);
        } else if let Some(scene_id) = catalog.node_index.get(&target).cloned() {
            self.current_node = Some(target);
            self.current_scene = Some(scene_id);
        } else {
            warn!("DialogueCatalog has no scene or node id '{target}'");
            return false;
        }
        self.active = true;
        self.just_spawned = true;
        true
    }

    pub fn end(&mut self) {
        self.active = false;
        self.just_spawned = false;
        self.current_scene = None;
        self.current_node = None;
    }

    pub fn goto(&mut self, node: Option<NodeId>) {
        if let Some(id) = node {
            self.current_node = Some(id);
        } else {
            self.end();
        }
    }
}

/// Player-selected option index for the current `Choice` node, if any.
#[derive(Resource, Default)]
pub struct DialogueSelectedIndex(pub Option<usize>);

/// Side channel from the effect dispatcher to the advance step. The dispatcher
/// can't mutate `DialogueRuntime` itself because the caller already holds a
/// mutable borrow, so a `ChangeScene` effect parks its target here and
/// `advance_dialogue` consumes it after `dispatch_all` returns.
#[derive(Resource, Default)]
pub struct PendingSceneChange(pub Option<SceneId>);

/// Tracks the entity playing looping music so subsequent `PlayMusic` effects
/// can stop the previous track before starting a new one.
#[derive(Resource, Default)]
pub struct CurrentMusic(pub Option<Entity>);

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

#[derive(SystemParam)]
pub struct ConditionContext<'w> {
    pub flags: Res<'w, StoryFlags>,
    pub inventory: Res<'w, PlayerInventory>,
    pub quest_log: Res<'w, QuestLog>,
    pub reputation: Res<'w, ReputationLedger>,
    pub current_area: Res<'w, CurrentArea>,
    pub cities: Res<'w, CityCatalog>,
    pub merchants: Res<'w, Merchants>,
}

pub fn evaluate_condition(cond: &Condition, ctx: &ConditionContext) -> bool {
    match cond {
        Condition::Flag(name) => ctx.flags.is_set(name),
        Condition::NotFlag(name) => !ctx.flags.is_set(name),
        Condition::All(parts) => parts.iter().all(|c| evaluate_condition(c, ctx)),
        Condition::Any(parts) => parts.iter().any(|c| evaluate_condition(c, ctx)),
        Condition::Not(inner) => !evaluate_condition(inner, ctx),
        Condition::HasItem { item, qty } => {
            let needed = *qty as u32;
            let have: u32 = ctx
                .inventory
                .0
                .iter()
                .filter(|s| s.item_id as u32 == *item)
                .map(|s| s.quantity as u32)
                .sum();
            have >= needed
        }
        Condition::QuestStatus { quest, status } => {
            let observed = ctx
                .quest_log
                .quests
                .get(quest)
                .map(|q| q.status)
                .map(QuestStatusFilter::from);
            match (observed, status) {
                (None, QuestStatusFilter::Inactive) => true,
                (Some(actual), expected) => actual == *expected,
                _ => false,
            }
        }
        Condition::ReputationAtLeast { target, min } => {
            let Some(resolved) = resolve_reputation_target(
                target,
                ctx.current_area.0,
                &ctx.cities,
                &ctx.merchants,
            ) else {
                return false;
            };
            let value = match resolved {
                ReputationTarget::Governor { city_id } => ctx.reputation.get_governor(city_id),
                ReputationTarget::Merchant { merchant_id } => {
                    ctx.reputation.get_merchant(merchant_id)
                }
                ReputationTarget::Clan { clan_name } => ctx.reputation.get_clan(&clan_name),
            };
            value >= *min
        }
    }
}

impl From<QuestStatus> for QuestStatusFilter {
    fn from(s: QuestStatus) -> Self {
        match s {
            QuestStatus::Active => QuestStatusFilter::Active,
            QuestStatus::Completed => QuestStatusFilter::Completed,
            QuestStatus::Failed => QuestStatusFilter::Failed,
        }
    }
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

#[derive(SystemParam)]
pub struct EffectDispatcher<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub asset_server: Res<'w, AssetServer>,
    pub flags: ResMut<'w, StoryFlags>,
    pub inventory: ResMut<'w, PlayerInventory>,
    pub wallet: ResMut<'w, PlayerWallet>,
    pub current_area: Res<'w, CurrentArea>,
    pub cities: Res<'w, CityCatalog>,
    pub merchants: Res<'w, Merchants>,
    pub reputation_events: ResMut<'w, Messages<ReputationChangeEvent>>,
    pub flag_changed_events: ResMut<'w, Messages<FlagChangedEvent>>,
    pub add_quest_events: ResMut<'w, Messages<AddQuestEvent>>,
    pub advance_obj_events: ResMut<'w, Messages<AdvanceObjectiveEvent>>,
    pub current_music: ResMut<'w, CurrentMusic>,
    pub pending_scene_change: ResMut<'w, PendingSceneChange>,
    pub player_q: Query<'w, 's, Entity, With<Player>>,
    pub interactables_q: Query<'w, 's, (Entity, &'static Interactable)>,
}

impl<'w, 's> EffectDispatcher<'w, 's> {
    pub fn dispatch_all(&mut self, effects: &[Effect]) {
        for effect in effects {
            self.dispatch(effect);
        }
    }

    pub fn dispatch(&mut self, effect: &Effect) {
        match effect {
            Effect::SetFlag(name) => {
                let inserted = self.flags.set(name.clone());
                if inserted {
                    self.flag_changed_events.write(FlagChangedEvent {
                        name: name.clone(),
                        set: true,
                    });
                }
            }
            Effect::ClearFlag(name) => {
                let removed = self.flags.clear(name);
                if removed {
                    self.flag_changed_events.write(FlagChangedEvent {
                        name: name.clone(),
                        set: false,
                    });
                }
            }
            Effect::Reputation { target, delta, reason } => {
                if let Some(resolved) = resolve_reputation_target(
                    target,
                    self.current_area.0,
                    &self.cities,
                    &self.merchants,
                ) {
                    self.reputation_events.write(ReputationChangeEvent {
                        target: resolved,
                        delta: *delta,
                        reason: reason.clone(),
                    });
                }
            }
            Effect::GiveItem { item, qty } => give_item(&mut self.inventory, *item, *qty),
            Effect::TakeItem { item, qty } => take_item(&mut self.inventory, *item, *qty),
            Effect::GiveCoin(amount) => {
                self.wallet.coins = self.wallet.coins.saturating_add(*amount);
            }
            Effect::TakeCoin(amount) => {
                self.wallet.coins = self.wallet.coins.saturating_sub(*amount);
            }
            Effect::StartQuest(quest_id) => {
                self.add_quest_events
                    .write(AddQuestEvent { quest_id: *quest_id });
            }
            Effect::AdvanceObjective { quest, objective } => {
                self.advance_obj_events.write(AdvanceObjectiveEvent {
                    quest_id: *quest,
                    objective_id: *objective,
                    delta: 1,
                });
            }
            Effect::AcceptContract => {
                if let Ok(player) = self.player_q.single() {
                    self.commands.entity(player).insert(Bound);
                } else {
                    warn!("AcceptContract: no player entity");
                }
            }
            Effect::PlaySfx(name) => {
                let path = format!("audio/sfx/{name}");
                self.commands.spawn((
                    AudioPlayer::new(self.asset_server.load(path)),
                    PlaybackSettings::DESPAWN,
                ));
            }
            Effect::PlayMusic(track) => {
                if let Some(prev) = self.current_music.0.take() {
                    self.commands.entity(prev).despawn();
                }
                if let Some(name) = track {
                    let path = format!("audio/music/{name}");
                    let entity = self
                        .commands
                        .spawn((
                            AudioPlayer::new(self.asset_server.load(path)),
                            PlaybackSettings::LOOP,
                        ))
                        .id();
                    self.current_music.0 = Some(entity);
                }
            }
            Effect::SpawnInteractable { kind, .. } => {
                // Spawn templates need an authored registry that doesn't exist
                // yet. Logging here so authored scenes referencing future kinds
                // are visible without crashing.
                warn!("SpawnInteractable: unknown kind '{kind}' (no template registry)");
            }
            Effect::DespawnInteractable { name } => {
                let mut despawned = false;
                for (entity, interactable) in self.interactables_q.iter() {
                    if interactable.name == *name {
                        self.commands.entity(entity).despawn();
                        despawned = true;
                    }
                }
                if !despawned {
                    debug!("DespawnInteractable: no Interactable named '{name}'");
                }
            }
            Effect::ChangeScene(scene_id) => {
                self.pending_scene_change.0 = Some(scene_id.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn give_item(inventory: &mut PlayerInventory, item: u32, qty: u32) {
    let Some(item_id) = u32_to_u16(item, "GiveItem.item") else {
        return;
    };
    let Some(qty_u16) = u32_to_u16(qty, "GiveItem.qty") else {
        return;
    };
    if let Some(stack) = inventory.0.iter_mut().find(|s| s.item_id == item_id) {
        stack.quantity = stack.quantity.saturating_add(qty_u16);
    } else {
        inventory.0.push(InventoryStack {
            item_id,
            quantity: qty_u16,
        });
    }
}

fn take_item(inventory: &mut PlayerInventory, item: u32, qty: u32) {
    let Some(item_id) = u32_to_u16(item, "TakeItem.item") else {
        return;
    };
    let Some(qty_u16) = u32_to_u16(qty, "TakeItem.qty") else {
        return;
    };
    let mut remaining = qty_u16;
    inventory.0.retain_mut(|stack| {
        if remaining == 0 || stack.item_id != item_id {
            return true;
        }
        if stack.quantity > remaining {
            stack.quantity -= remaining;
            remaining = 0;
            true
        } else {
            remaining -= stack.quantity;
            false
        }
    });
}

fn u32_to_u16(value: u32, ctx: &'static str) -> Option<u16> {
    if value > u16::MAX as u32 {
        warn!("{ctx}: value {value} exceeds u16::MAX; clamping");
        Some(u16::MAX)
    } else {
        Some(value as u16)
    }
}

// ---------------------------------------------------------------------------
// on_enter dispatch: fire a Line node's `on_enter` effects exactly once when
// the runtime transitions onto it. Runs as a Bevy system every frame and
// keys off the last-seen node id stored in a `Local`.
// ---------------------------------------------------------------------------

pub fn dispatch_on_enter(
    mut last: Local<Option<NodeId>>,
    mut runtime: ResMut<DialogueRuntime>,
    catalog: Res<DialogueCatalog>,
    playback: Res<ScenePlayback>,
    mut effects: EffectDispatcher,
) {
    let current = runtime.current_node.clone();
    if current == *last {
        return;
    }
    *last = current.clone();

    if !runtime.active || playback.active {
        return;
    }

    let Some(node) = runtime.current_node(&catalog) else {
        return;
    };
    let DialogueNode::Line(line) = node else {
        return;
    };
    if line.on_enter.is_empty() {
        return;
    }
    let to_dispatch: Vec<Effect> = line.on_enter.clone();
    effects.dispatch_all(&to_dispatch);
    if let Some(target) = effects.pending_scene_change.0.take() {
        runtime.start(target, &catalog);
    }
}

fn resolve_reputation_target(
    target: &ReputationTargetRef,
    current_region: u16,
    cities: &CityCatalog,
    merchants: &Merchants,
) -> Option<ReputationTarget> {
    match target {
        ReputationTargetRef::LocalGovernor => cities
            .0
            .values()
            .find(|c| c.region_ids.contains(&current_region))
            .map(|c| ReputationTarget::Governor { city_id: c.id }),
        ReputationTargetRef::LocalMerchant => merchants
            .0
            .iter()
            .find(|(_, m)| m.region_id == current_region)
            .map(|(id, _)| ReputationTarget::Merchant { merchant_id: *id }),
        ReputationTargetRef::LocalClan => cities
            .0
            .values()
            .find(|c| c.region_ids.contains(&current_region))
            .map(|c| ReputationTarget::Clan {
                clan_name: c.clan_name.clone(),
            }),
        ReputationTargetRef::Governor { city_id } => {
            Some(ReputationTarget::Governor { city_id: *city_id })
        }
        ReputationTargetRef::Merchant { merchant_id } => Some(ReputationTarget::Merchant {
            merchant_id: *merchant_id,
        }),
        ReputationTargetRef::Clan { name } => Some(ReputationTarget::Clan {
            clan_name: name.clone(),
        }),
    }
}
