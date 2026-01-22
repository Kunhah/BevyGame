use std::collections::HashMap;

use bevy::prelude::{Messages, *};
use bevy::window::{Window, WindowPlugin};
use bevy::log::{Level, LogPlugin};

mod battle;
mod combat_ability;
mod combat_plugin;
mod constants;
mod core;
mod debug_console;
mod dialogue;
mod light_plugin;
mod menu;
mod map;
mod movement;
mod pathfinding;
mod quadtree;
mod quests;
mod world;

use battle::enter_battle;
use combat_plugin::{CombatPlugin, DamageQueue, HealthRegenEvent, MagicRegenEvent, StaminaRegenEvent, DeathEvent, AwardXpEvent, AttackIntentEvent};
use constants::*;
use core::{GameState, Game_State, GlobalVariables, Global_Variables, PlayerMapPosition, Position, Timestamp};
use debug_console::DebugConsolePlugin;
use dialogue::{
    create_first_dialogue, gui_selection, interact, spawn_dialogue_box, CachedInteractables, Choice,
    Conditionals, DialogueSet, DialogueState, DialogueBoxTriggerEvent, DialogueTriggerEvent,
    Dialogue_Data, Dialogue_State, Next_Id, Selected_Choice, Selected_Choice_Index,
};
use light_plugin::LightPlugin;
use menu::MenuPlugin;
use movement::{follow_path_system, mouse_click, player_movement, toggle_camera_lock};
use map::{
    confirm_travel, generate_map_tiles, navigate_map_selection, toggle_map_mode,
    update_active_tile_background, ActiveMapBackground, CurrentArea, MapSelection, MapTiles,
};
use quests::QuestPlugin;
use quadtree::CachedColliders;
use world::{setup, update_cache};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(LogPlugin {
                    level: Level::INFO,
                    filter: "wgpu=error,bevy_render=warn".to_string(),
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Seirei Kuni".to_string(),
                        resolution: (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(LightPlugin)
        .add_plugins(CombatPlugin)
        .add_plugins(QuestPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(DebugConsolePlugin)
        .insert_resource(PlayerMapPosition(Position::default()))
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.1)))
        .insert_resource(CachedInteractables(Vec::new()))
        .insert_resource(CachedColliders(Vec::new()))
        .insert_resource(GameState(Game_State::MainMenu))
        .insert_resource(Global_Variables(GlobalVariables::default()))
        .insert_resource(Timestamp(0))
        .insert_resource(Dialogue_State(DialogueState::default()))
        .insert_resource(Selected_Choice(Choice::default()))
        .insert_resource(Selected_Choice_Index(None))
        .insert_resource(Next_Id(HashMap::new()))
        .insert_resource(Conditionals(Flags::empty()))
        .insert_resource(Messages::<DialogueBoxTriggerEvent>::default())
        .insert_resource(Messages::<DialogueTriggerEvent>::default())
        .insert_resource(Messages::<DeathEvent>::default())
        .insert_resource(Messages::<HealthRegenEvent>::default())
        .insert_resource(Messages::<MagicRegenEvent>::default())
        .insert_resource(Messages::<StaminaRegenEvent>::default())
        .insert_resource(Messages::<AwardXpEvent>::default())
        .insert_resource(Messages::<AttackIntentEvent>::default())
        .init_resource::<movement::TravelTimeAccumulator>()
        .insert_resource(DamageQueue::default())
        .insert_resource(generate_map_tiles())
        .insert_resource(MapSelection(Position::default()))
        .insert_resource(CurrentArea::default())
        .insert_resource(ActiveMapBackground::default())
        .add_systems(Startup, setup)
        .add_systems(Update, player_movement)
        .add_systems(Update, toggle_camera_lock)
        .add_systems(Update, update_cache)
        .add_systems(Update, mouse_click)
        .add_systems(Update, follow_path_system)
        // map travel mode
        .add_systems(Update, toggle_map_mode)
        .add_systems(Update, navigate_map_selection)
        .add_systems(Update, confirm_travel)
        .add_systems(Update, update_active_tile_background)
        .add_systems(
            Update,
            movement::accumulate_manual_travel_time.after(player_movement),
        )
        .add_systems(Update, spawn_dialogue_box.in_set(DialogueSet::Spawn))
        .add_systems(
            Update,
            interact
                .in_set(DialogueSet::Interact)
                .after(DialogueSet::Spawn),
        )
        .add_systems(Update, create_first_dialogue)
        .add_systems(Update, gui_selection)
        .run();
}
