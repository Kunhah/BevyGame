use std::fs;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::city_data::{CityCatalog, ClanCatalog};
use crate::core::{GameState, Game_State, Player, PlayerMapPosition, Position, Timestamp};
use crate::economy::{ActiveCaravans, CaravanClock};
use crate::governance::{
    CastleAssaultClock, CoupChainState, CoupPreparationProgress, GlobalPunishmentState,
    GovernanceClock, GovernorPolicyClock, PlayerCrimeStatus, ReputationLedger,
};
use crate::map::{CurrentArea, MapSelection, MapTiles};

const SAVE_DIR: &str = "saves";

#[derive(Clone, Copy, Debug)]
pub enum SaveSlot {
    Auto,
    Slot1,
    Slot2,
    Slot3,
}

impl SaveSlot {
    fn file_name(self) -> &'static str {
        match self {
            SaveSlot::Auto => "auto.ron",
            SaveSlot::Slot1 => "slot_1.ron",
            SaveSlot::Slot2 => "slot_2.ron",
            SaveSlot::Slot3 => "slot_3.ron",
        }
    }

    fn path(self) -> String {
        format!("{}/{}", SAVE_DIR, self.file_name())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SaveAction {
    Save,
    Load,
}

#[derive(Clone, Copy, Debug, Message)]
pub struct SaveRequest {
    pub action: SaveAction,
    pub slot: SaveSlot,
}

#[derive(Resource)]
pub struct AutoSaveSettings {
    pub enabled: bool,
    pub interval_seconds: f32,
    pub timer: Timer,
}

impl Default for AutoSaveSettings {
    fn default() -> Self {
        let interval_seconds = 180.0;
        Self {
            enabled: true,
            interval_seconds,
            timer: Timer::from_seconds(interval_seconds, TimerMode::Repeating),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SaveVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<Vec3> for SaveVec3 {
    fn from(v: Vec3) -> Self {
        Self {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}

impl From<SaveVec3> for Vec3 {
    fn from(v: SaveVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveData {
    pub player_world: SaveVec3,
    pub player_tile: Position,
    pub map_selection: Position,
    pub current_area: u16,
    pub timestamp: u32,
    pub map_tiles: MapTiles,
    #[serde(default)]
    pub city_catalog: CityCatalog,
    #[serde(default)]
    pub clan_catalog: ClanCatalog,
    #[serde(default)]
    pub active_caravans: ActiveCaravans,
    #[serde(default)]
    pub caravan_clock: CaravanClock,
    #[serde(default)]
    pub reputation_ledger: ReputationLedger,
    #[serde(default)]
    pub player_crime_status: PlayerCrimeStatus,
    #[serde(default)]
    pub global_punishment_state: GlobalPunishmentState,
    #[serde(default)]
    pub coup_chain_state: CoupChainState,
    #[serde(default)]
    pub governance_clock: GovernanceClock,
    #[serde(default)]
    pub castle_assault_clock: CastleAssaultClock,
    #[serde(default)]
    pub governor_policy_clock: GovernorPolicyClock,
    #[serde(default)]
    pub coup_preparation_progress: CoupPreparationProgress,
    // TODO(save): include inventory, quests, flags, skills, stats, active events, party, and anything else that must persist.
}

pub fn save_game_hotkeys(
    input: Res<ButtonInput<KeyCode>>,
    mut requests: ResMut<Messages<SaveRequest>>,
) {
    if input.just_pressed(KeyCode::F5) {
        requests.write(SaveRequest {
            action: SaveAction::Save,
            slot: SaveSlot::Slot1,
        });
    }
    if input.just_pressed(KeyCode::F9) {
        requests.write(SaveRequest {
            action: SaveAction::Load,
            slot: SaveSlot::Slot1,
        });
    }
}

pub fn handle_save_requests(
    mut requests: ResMut<Messages<SaveRequest>>,
    mut game_state: ResMut<GameState>,
    mut map: ResMut<MapTiles>,
    mut selection: ResMut<MapSelection>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<Timestamp>,
    mut city_catalog: ResMut<CityCatalog>,
    mut clan_catalog: ResMut<ClanCatalog>,
    mut active_caravans: ResMut<ActiveCaravans>,
    mut caravan_clock: ResMut<CaravanClock>,
    mut reputation_ledger: ResMut<ReputationLedger>,
    mut governance_state: ParamSet<(
        ResMut<PlayerCrimeStatus>,
        ResMut<GlobalPunishmentState>,
        ResMut<CoupChainState>,
        ResMut<GovernanceClock>,
        ResMut<CastleAssaultClock>,
        ResMut<GovernorPolicyClock>,
        ResMut<CoupPreparationProgress>,
    )>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_q: Query<&mut Transform, (With<crate::core::MainCamera>, Without<Player>)>,
) {
    for req in requests.drain() {
        match req.action {
            SaveAction::Save => {
                if game_state.0 != Game_State::Exploring && game_state.0 != Game_State::MapOpen {
                    continue;
                }
                let Ok(player_tf) = player_q.single_mut() else {
                    warn!("save_game: player transform not found");
                    continue;
                };
                let player_crime_status = (*governance_state.p0()).clone();
                let global_punishment_state = (*governance_state.p1()).clone();
                let coup_chain_state = (*governance_state.p2()).clone();
                let governance_clock = (*governance_state.p3()).clone();
                let castle_assault_clock = (*governance_state.p4()).clone();
                let governor_policy_clock = (*governance_state.p5()).clone();
                let coup_preparation_progress = (*governance_state.p6()).clone();
                let data = SaveData {
                    player_world: SaveVec3::from(player_tf.translation),
                    player_tile: map_position.0,
                    map_selection: selection.0,
                    current_area: current_area.0,
                    timestamp: timestamp.0,
                    map_tiles: map.clone(),
                    city_catalog: city_catalog.clone(),
                    clan_catalog: clan_catalog.clone(),
                    active_caravans: active_caravans.clone(),
                    caravan_clock: caravan_clock.clone(),
                    reputation_ledger: reputation_ledger.clone(),
                    player_crime_status,
                    global_punishment_state,
                    coup_chain_state,
                    governance_clock,
                    castle_assault_clock,
                    governor_policy_clock,
                    coup_preparation_progress,
                };
                if let Err(e) = write_save(req.slot, &data) {
                    warn!("save_game: {}", e);
                } else {
                    info!("Saved game to {}", req.slot.path());
                }
            }
            SaveAction::Load => {
                let Ok(data) = read_save(req.slot) else {
                    warn!("load_game: save file not found at {}", req.slot.path());
                    continue;
                };
                map.tiles = data.map_tiles.tiles;
                normalize_legacy_tile_image_paths(&mut map);
                selection.0 = data.map_selection;
                map_position.0 = data.player_tile;
                current_area.0 = data.current_area;
                timestamp.0 = data.timestamp;
                *city_catalog = data.city_catalog;
                *clan_catalog = data.clan_catalog;
                *active_caravans = data.active_caravans;
                *caravan_clock = data.caravan_clock;
                *reputation_ledger = data.reputation_ledger;
                *governance_state.p0() = data.player_crime_status;
                *governance_state.p1() = data.global_punishment_state;
                *governance_state.p2() = data.coup_chain_state;
                *governance_state.p3() = data.governance_clock;
                *governance_state.p4() = data.castle_assault_clock;
                *governance_state.p5() = data.governor_policy_clock;
                *governance_state.p6() = data.coup_preparation_progress;

                if let Ok(mut player_tf) = player_q.single_mut() {
                    player_tf.translation = Vec3::from(data.player_world);
                }
                if let Ok(mut cam_tf) = camera_q.single_mut() {
                    let loaded = Vec3::from(data.player_world);
                    cam_tf.translation =
                        Vec3::new(loaded.x, loaded.y, 0.0) + crate::render3d::iso_camera_offset();
                }

                game_state.0 = Game_State::Exploring;
                info!("Loaded game from {}", req.slot.path());
            }
        }
    }
}

pub fn autosave_tick(
    time: Res<Time>,
    mut settings: ResMut<AutoSaveSettings>,
    game_state: Res<GameState>,
    mut requests: ResMut<Messages<SaveRequest>>,
) {
    if !settings.enabled {
        return;
    }
    if game_state.0 != Game_State::Exploring {
        return;
    }

    settings.timer.tick(time.delta());
    if settings.timer.just_finished() {
        requests.write(SaveRequest {
            action: SaveAction::Save,
            slot: SaveSlot::Auto,
        });
    }
}

fn write_save(slot: SaveSlot, data: &SaveData) -> Result<(), String> {
    if let Err(e) = fs::create_dir_all(SAVE_DIR) {
        return Err(format!("failed to create save directory: {}", e));
    }
    let path = slot.path();
    // Compact RON: smaller files, faster I/O. Saves are large (10s of MB of map tiles)
    // so we skip pretty-printing — savings are significant on disk and parse time.
    let serialized = ron::ser::to_string(data).map_err(|e| e.to_string())?;
    fs::write(&path, serialized).map_err(|e| format!("failed to write save file: {}", e))?;
    Ok(())
}

fn read_save(slot: SaveSlot) -> Result<SaveData, String> {
    let path = slot.path();
    let contents = fs::read_to_string(&path).map_err(|_| "save file not found".to_string())?;
    ron::de::from_str::<SaveData>(&contents).map_err(|e| format!("failed to parse save: {}", e))
}

fn normalize_legacy_tile_image_paths(map: &mut MapTiles) {
    for row in &mut map.tiles {
        for tile in row {
            if tile.image_path == "dot.webp" {
                tile.image_path = "dot.png".to_string();
            }
        }
    }
}
