use std::collections::HashMap;

use bevy::prelude::{Messages, *};
use bevy::render::{
    settings::{Backends, WgpuSettings},
    RenderPlugin,
};
use bevy::window::{Window, WindowPlugin};
use bevy::log::{Level, LogPlugin};

mod activities;
mod ai_decision;
mod battle;
mod city_data;
mod combat_ability;
mod combat_hud;
mod combat_plugin;
mod constants;
mod contract;
mod core;
mod debug_console;
mod dialogue;
mod economy;
mod governance;
mod hud;
mod light_plugin;
mod menu;
mod map;
mod movement;
mod pathfinding;
mod quadtree;
mod quests;
mod save;
mod services;
mod settings;
mod skill_tree;
mod status_effects;
mod ui_style;
mod world;

use battle::{
    battle_trigger_system, combat_end_turn_input, end_battle_on_death,
    setup_player_turns, sync_combat_move_points_from_world, test_log_button, transform_npc_to_enemy, BattleState,
};
use combat_hud::CombatHudPlugin;
use combat_plugin::{
    AfterRestEvent, AttackIntentEvent, AwardXpEvent, BeforeRestEvent, CombatPlugin, DamageQueue,
    DeathEvent, RestEvent,
};
use contract::ContractPlugin;
use constants::*;
use core::{GameState, Game_State, GlobalVariables, Global_Variables, PlayerMapPosition, Position, Timestamp};
use debug_console::DebugConsolePlugin;
use dialogue::{
    create_first_dialogue, gui_selection, interact, spawn_dialogue_box, CachedInteractables, Choice,
    Conditionals, DialogueReputationRules, DialogueSet, DialogueState, DialogueBoxTriggerEvent, DialogueTriggerEvent,
    Dialogue_State, Next_Id, Selected_Choice, Selected_Choice_Index,
};
use economy::EconomyPlugin;
use governance::GovernancePlugin;
use hud::HudPlugin;
use light_plugin::LightPlugin;
use menu::MenuPlugin;
use movement::{
    ally_follow_player_system, follow_path_system, mouse_click, player_movement,
    toggle_camera_lock,
};
use map::{
    clear_completed_tile_events, confirm_travel, generate_map_tiles, handle_tile_entry,
    handle_local_map_boundary_crossing,
    navigate_map_selection_keyboard, navigate_map_selection_mouse, toggle_map_mode,
    update_active_tile_background, update_path_preview, demo_tile_event_handler,
    ActiveMapBackgrounds, ActiveTileEvent, AfterTileEnterEvent, AreaChanged, AreaTransitionLog,
    BeforeTileEnterEvent, CurrentArea, LastEnteredTile, MapOverlay, MapPathPreview, MapSelection,
    MapTravelUi, MapTravelPathCache, TerrainSlowEffectIndex, TerrainSlowEffectList,
    TileContentCache, TileEventCompleted, TileEventTriggered, handle_area_changed,
    rebuild_terrain_slow_effect_index, update_travel_ui,
};
use quests::QuestPlugin;
use save::{
    autosave_tick, handle_save_requests, save_game_hotkeys, AutoSaveSettings, SaveRequest,
};
use services::ServicesPlugin;
use settings::SettingsPlugin;
use skill_tree::SkillTreePlugin;
use status_effects::StatusEffectsPlugin;
use ui_style::UiStylePlugin;
use ai_decision::AiDecisionPlugin;
use quadtree::CachedColliders;
use world::{apply_y_sort, setup, update_cache, update_visual_occluders};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: Some(Backends::VULKAN),
                        ..default()
                    }
                    .into(),
                    ..default()
                })
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
        .add_plugins(UiStylePlugin)
        .add_plugins(HudPlugin)
        .add_plugins(LightPlugin)
        .add_plugins(CombatPlugin)
        .add_plugins(StatusEffectsPlugin)
        .add_plugins(ContractPlugin)
        .add_plugins(GovernancePlugin)
        .add_plugins(EconomyPlugin)
        .add_plugins(ServicesPlugin)
        .add_plugins(QuestPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(SettingsPlugin)
        .add_plugins(SkillTreePlugin)
        .add_plugins(CombatHudPlugin)
        .add_plugins(AiDecisionPlugin)
        .add_plugins(activities::ActivitiesPlugin)
        .add_plugins(DebugConsolePlugin)
        .insert_resource(PlayerMapPosition(Position::default()))
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.1)))
        .insert_resource(CachedInteractables(Vec::new()))
        .insert_resource(CachedColliders(Vec::new()))
        .insert_resource(GameState(Game_State::MainMenu))
        .insert_resource(BattleState::default())
        .insert_resource(Global_Variables(GlobalVariables::default()))
        .insert_resource(Timestamp(0))
        .insert_resource(Dialogue_State(DialogueState::default()))
        .insert_resource(Selected_Choice(Choice::default()))
        .insert_resource(Selected_Choice_Index(None))
        .insert_resource(Next_Id(HashMap::new()))
        .init_resource::<DialogueReputationRules>()
        .insert_resource(Conditionals(Flags::empty()))
        .insert_resource(Messages::<DialogueBoxTriggerEvent>::default())
        .insert_resource(Messages::<DialogueTriggerEvent>::default())
        .insert_resource(Messages::<DeathEvent>::default())
        .insert_resource(Messages::<RestEvent>::default())
        .insert_resource(Messages::<BeforeRestEvent>::default())
        .insert_resource(Messages::<AfterRestEvent>::default())
        .insert_resource(Messages::<AwardXpEvent>::default())
        .insert_resource(Messages::<AttackIntentEvent>::default())
        .init_resource::<movement::TravelTimeAccumulator>()
        .insert_resource(DamageQueue::default())
        .insert_resource(generate_map_tiles())
        .insert_resource(MapSelection(Position::default()))
        .insert_resource(CurrentArea::default())
        .insert_resource(ActiveMapBackgrounds::default())
        .insert_resource(TileContentCache::default())
        .insert_resource(MapOverlay::default())
        .insert_resource(MapTravelUi::default())
        .insert_resource(LastEnteredTile::default())
        .insert_resource(AreaTransitionLog::default())
        .insert_resource(ActiveTileEvent::default())
        .insert_resource(MapPathPreview::default())
        .insert_resource(MapTravelPathCache::default())
        .insert_resource(TerrainSlowEffectList::default())
        .insert_resource(TerrainSlowEffectIndex::default())
        .insert_resource(Messages::<TileEventTriggered>::default())
        .insert_resource(Messages::<TileEventCompleted>::default())
        .insert_resource(Messages::<AreaChanged>::default())
        .insert_resource(Messages::<BeforeTileEnterEvent>::default())
        .insert_resource(Messages::<AfterTileEnterEvent>::default())
        .insert_resource(Messages::<SaveRequest>::default())
        .insert_resource(AutoSaveSettings::default())
        .add_systems(Startup, setup)
        .add_systems(Update, player_movement)
        .add_systems(Update, toggle_camera_lock)
        .add_systems(Update, update_cache)
        .add_systems(Update, rebuild_terrain_slow_effect_index)
        .add_systems(Update, apply_y_sort.after(player_movement))
        .add_systems(
            Update,
            update_visual_occluders
                .after(player_movement)
                .run_if(graphics_setting_visual_occluder_fade),
        )
        .add_systems(Update, mouse_click)
        .add_systems(Update, battle_trigger_system)
        .add_systems(Update, setup_player_turns)
        .add_systems(Update, sync_combat_move_points_from_world.after(setup_player_turns))
        .add_systems(Update, combat_end_turn_input)
        .add_systems(Update, transform_npc_to_enemy)
        .add_systems(Update, test_log_button)
        .add_systems(Update, end_battle_on_death)
        .add_systems(Update, follow_path_system)
        .add_systems(Update, ally_follow_player_system.after(player_movement))
        // map travel mode
        .add_systems(Update, toggle_map_mode)
        .add_systems(Update, navigate_map_selection_keyboard)
        .add_systems(Update, navigate_map_selection_mouse)
        .add_systems(Update, confirm_travel)
        .add_systems(Update, update_active_tile_background)
        .add_systems(Update, handle_local_map_boundary_crossing.after(player_movement))
        .add_systems(Update, handle_tile_entry)
        .add_systems(Update, demo_tile_event_handler)
        .add_systems(Update, clear_completed_tile_events)
        .add_systems(Update, update_path_preview)
        .add_systems(Update, update_travel_ui)
        .add_systems(Update, handle_area_changed)
        .add_systems(Update, save_game_hotkeys)
        .add_systems(Update, handle_save_requests)
        .add_systems(Update, autosave_tick)
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

fn graphics_setting_visual_occluder_fade(graphics: Res<settings::GraphicsSettings>) -> bool {
    graphics.visual_occluder_fade
}
