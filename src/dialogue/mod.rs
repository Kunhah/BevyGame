use bevy::prelude::*;
use bevy::prelude::Messages;

mod loader;
mod runtime;
mod scene_player;
mod schema;
mod stage;
mod ui;

use runtime::dispatch_on_enter;
use scene_player::{tick_scene_playback, ScenePlayback};
use stage::{
    auto_place_line_speaker, despawn_stage_when_inactive, refresh_stage_visuals, StageState,
};
use ui::{
    create_first_dialogue, gui_selection, interact, redraw_when_runtime_changes,
    spawn_dialogue_box, DialogueSet, DialogueTriggerEvent,
};

// Schema and runtime types are surfaced for future steps (editor schema dump,
// downstream consumers). Only a subset is currently consumed by the rest of
// the crate.
#[allow(unused_imports)]
pub use loader::build_dialogue_catalog;
#[allow(unused_imports)]
pub use runtime::{
    evaluate_condition, ConditionContext, CurrentMusic, DialogueCatalog, DialogueRuntime,
    DialogueSelectedIndex, EffectDispatcher, PendingSceneChange,
};
#[allow(unused_imports)]
pub use schema::{
    ChoiceNode, ChoiceOption, Condition, DialogueNode, DialogueScene, Effect, LineNode, NodeId,
    QuestStatusFilter, ReputationTargetRef, SceneAction, SceneId, SceneNode, Speaker, SpeakerSlot,
};
pub use ui::{CachedInteractables, DialogueBoxTriggerEvent, Interactable};

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(build_dialogue_catalog())
            .init_resource::<DialogueRuntime>()
            .init_resource::<DialogueSelectedIndex>()
            .init_resource::<CurrentMusic>()
            .init_resource::<PendingSceneChange>()
            .init_resource::<ScenePlayback>()
            .init_resource::<StageState>()
            .insert_resource(CachedInteractables(Vec::new()))
            .insert_resource(Messages::<DialogueBoxTriggerEvent>::default())
            .insert_resource(Messages::<DialogueTriggerEvent>::default())
            .add_systems(Update, spawn_dialogue_box.in_set(DialogueSet::Spawn))
            .add_systems(
                Update,
                interact
                    .in_set(DialogueSet::Interact)
                    .after(DialogueSet::Spawn),
            )
            .add_systems(Update, create_first_dialogue)
            .add_systems(Update, gui_selection)
            // Scene-action timeline runs after input so a fresh advance can
            // start playback this same frame.
            .add_systems(Update, tick_scene_playback.after(DialogueSet::Interact))
            // Detect node transitions (e.g. scene player completing, choice
            // advance) and fire on_enter effects once per entry.
            .add_systems(Update, dispatch_on_enter.after(tick_scene_playback))
            // Stage updates after node transitions and effects are applied.
            .add_systems(
                Update,
                auto_place_line_speaker.after(dispatch_on_enter),
            )
            .add_systems(
                Update,
                refresh_stage_visuals.after(auto_place_line_speaker),
            )
            .add_systems(
                Update,
                redraw_when_runtime_changes
                    .after(dispatch_on_enter)
                    .before(refresh_stage_visuals),
            )
            .add_systems(Update, despawn_stage_when_inactive);
    }
}
