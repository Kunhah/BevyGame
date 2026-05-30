//! Seirei Kuni library entry point.
//!
//! `run_full_game()` boots the complete game (used by `src/main.rs`).
//! `run_slice_demo()` boots the curated vertical-slice demo (used by
//! `src/bin/slice.rs`). Other binaries (`editor_server`, `dialogue_editor`,
//! `ability_editor`) are independent and don't depend on this library
//! surface.

#![allow(non_snake_case)]

use bevy::prelude::{Messages, *};
use bevy::render::{
    settings::{Backends, WgpuSettings},
    RenderPlugin,
};
use bevy::window::{Window, WindowPlugin};
use bevy::log::{Level, LogPlugin};

pub mod activities;
pub mod ai_decision;
pub mod areas;
pub mod battle;
pub mod characters;
pub mod city_data;
pub mod combat_ability;
pub mod combat_hud;
pub mod combat_plugin;
pub mod constants;
pub mod contract;
pub mod core;
pub mod creatures;
pub mod debug_console;
pub mod dialogue;
pub mod economy;
pub mod effects;
pub mod governance;
pub mod hud;
pub mod kegare;
pub mod light_plugin;
pub mod menu;
pub mod map;
pub mod party_select;
pub mod movement;
pub mod pathfinding;
pub mod quadtree;
pub mod cutscene;
pub mod post_fx;
pub mod quests;
pub mod render3d;
pub mod save;
pub mod services;
pub mod settings;
pub mod skill_tree;
pub mod status_effects;
pub mod story_flags;
pub mod tuning;
pub mod ui_style;
pub mod world;
pub mod world_rules;

use battle::{
    battle_trigger_system, combat_end_turn_input, end_battle_on_death, resolve_summon_system,
    setup_player_turns, sync_combat_move_points_from_world, test_log_button,
    tick_summon_lifetime_system, transform_npc_to_enemy, BattleState,
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
use dialogue::DialoguePlugin;
use economy::EconomyPlugin;
use governance::GovernancePlugin;
use hud::HudPlugin;
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
use quadtree::CachedColliders;
use quests::QuestPlugin;
use save::{
    autosave_tick, handle_save_requests, save_game_hotkeys, AutoSaveSettings, SaveRequest,
};
use services::ServicesPlugin;
use settings::SettingsPlugin;
use skill_tree::SkillTreePlugin;
use status_effects::StatusEffectsPlugin;
use story_flags::StoryFlagsPlugin;
use ui_style::UiStylePlugin;
use ai_decision::AiDecisionPlugin;
use world::{setup, update_cache};
use world_rules::WorldRulesPlugin;

/// Boot the full game. Adds every plugin in the project.
pub fn run_full_game() {
    full_game_app().run();
}

fn base_default_plugins(window_title: &str) -> impl PluginGroup {
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
                title: window_title.to_string(),
                resolution: (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32).into(),
                ..default()
            }),
            ..default()
        })
}

fn full_game_app() -> App {
    let mut app = App::new();

    // Named areas drive both the world-map travel UI and the terrain/location
    // ids stamped onto the single continuous tilemap. Build the catalog first
    // so the generated map can be stamped to match it before insertion.
    let area_catalog = areas::AreaCatalog::default();
    let mut map_tiles = generate_map_tiles();
    areas::stamp_areas_onto_map(&mut map_tiles, &area_catalog);
    // Ring the map in impassable edge tiles *after* area stamping so the border
    // always wins; the player can neither walk nor fast-travel onto them.
    map::apply_impassable_border(&mut map_tiles);

    app.add_plugins(base_default_plugins("Seirei Kuni"))
        .add_plugins(bevy::pbr::MaterialPlugin::<render3d::ToonMaterial>::default())
        .add_plugins(bevy_mod_outline::OutlinePlugin)
        .add_plugins(bevy_mod_outline::AutoGenerateOutlineNormalsPlugin::default())
        .add_plugins(post_fx::PostFxPlugin)
        // Phase 5: Blender->Bevy glTF component hydration. Components flagged
        // in Blender (Skein addon) hydrate via Bevy reflection at scene load,
        // provided the component derives Reflect and is registered via
        // `app.register_type::<T>()` in its owning plugin.
        .add_plugins(bevy_skein::SkeinPlugin::default())
        // Phase 5: data-driven cutscene sequencer. Press F1 to play
        // assets/cutscenes/intro.cutscene.ron.
        .add_plugins(cutscene::CutscenePlugin)
        // Live tuning panel — press F2 to open sliders for toon/post/grading.
        .add_plugins(tuning::RenderTuningPlugin)
        // Per-entity shader effects (HitFlash, Dissolve) — attach as
        // components; F3 / F4 demo them on the test capsule.
        .add_plugins(effects::EffectsPlugin)
        .add_plugins(UiStylePlugin)
        .add_plugins(HudPlugin)
        .add_plugins(CombatPlugin)
        .add_plugins(StatusEffectsPlugin)
        .add_plugins(kegare::KegarePlugin)
        .add_plugins(ContractPlugin)
        .add_plugins(GovernancePlugin)
        .add_plugins(EconomyPlugin)
        .add_plugins(ServicesPlugin)
        .add_plugins(QuestPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(party_select::PartySelectPlugin)
        .add_plugins(SettingsPlugin)
        .add_plugins(SkillTreePlugin)
        .add_plugins(CombatHudPlugin)
        .add_plugins(AiDecisionPlugin)
        .add_plugins(creatures::CreaturesPlugin)
        .add_plugins(activities::ActivitiesPlugin)
        .add_plugins(DebugConsolePlugin)
        .add_plugins(StoryFlagsPlugin)
        .add_plugins(DialoguePlugin)
        .add_plugins(WorldRulesPlugin)
        .add_plugins(areas::AreasPlugin)
        .insert_resource(PlayerMapPosition(map::PLAYER_SPAWN_TILE))
        .insert_resource(ClearColor(Color::srgb(0.1, 0.1, 0.1)))
        .insert_resource(CachedColliders(Vec::new()))
        .insert_resource(GameState(Game_State::MainMenu))
        .insert_resource(BattleState::default())
        .insert_resource(Global_Variables(GlobalVariables::default()))
        .insert_resource(Timestamp(0))
        .insert_resource(Messages::<DeathEvent>::default())
        .insert_resource(Messages::<RestEvent>::default())
        .insert_resource(Messages::<BeforeRestEvent>::default())
        .insert_resource(Messages::<AfterRestEvent>::default())
        .insert_resource(Messages::<AwardXpEvent>::default())
        .insert_resource(Messages::<AttackIntentEvent>::default())
        .init_resource::<movement::TravelTimeAccumulator>()
        .insert_resource(DamageQueue::default())
        .insert_resource(map_tiles)
        .insert_resource(area_catalog)
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
        .init_resource::<battle::PendingHuntBattle>()
        .init_resource::<render3d::CameraRig>()
        .init_resource::<characters::SelectedParty>()
        .add_systems(Startup, setup)
        .add_systems(Update, world::spawn_party)
        .add_systems(Update, player_movement)
        .add_systems(Update, toggle_camera_lock)
        .add_systems(Update, update_cache)
        .add_systems(Update, rebuild_terrain_slow_effect_index)
        .add_systems(Update, render3d::hydrate_placeholders)
        .add_systems(Update, render3d::scale_outline_width_by_distance)
        .add_systems(
            Update,
            render3d::debug_screenshot_once.run_if(|| std::env::var("ISO_SHOT").is_ok()),
        )
        .add_systems(Update, mouse_click)
        .add_systems(Update, render3d::drive_camera.after(player_movement))
        .add_systems(Update, battle_trigger_system)
        .add_systems(Update, battle::hunt_proximity_trigger)
        .add_systems(Update, battle::start_pending_hunt_battle)
        .add_systems(Update, setup_player_turns)
        .add_systems(Update, sync_combat_move_points_from_world.after(setup_player_turns))
        .add_systems(Update, battle::sync_player_combat_bound.after(setup_player_turns))
        .add_systems(Update, combat_end_turn_input)
        .add_systems(Update, transform_npc_to_enemy)
        .add_systems(Update, test_log_button)
        .add_systems(Update, end_battle_on_death)
        .add_systems(Update, resolve_summon_system)
        .add_systems(Update, tick_summon_lifetime_system)
        .add_systems(Update, battle::bridge_player_death_to_world)
        .add_systems(Update, follow_path_system)
        .add_systems(Update, ally_follow_player_system.after(player_movement))
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
        );
    app
}

pub fn graphics_setting_visual_occluder_fade(graphics: Res<settings::GraphicsSettings>) -> bool {
    graphics.visual_occluder_fade
}
