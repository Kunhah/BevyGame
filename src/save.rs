use std::fs;
use std::path::Path;

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::{GameState, Game_State, Player, PlayerMapPosition, Position, Timestamp};
use crate::map::{CurrentArea, MapSelection, MapTiles};

const SAVE_PATH: &str = "saves/savegame.json";

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
    pub current_area: u32,
    pub timestamp: u32,
    pub map_tiles: MapTiles,
    // TODO(save): include inventory, quests, flags, skills, stats, active events, party, and anything else that must persist.
}

pub fn save_game(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    selection: Res<MapSelection>,
    map_position: Res<PlayerMapPosition>,
    current_area: Res<CurrentArea>,
    timestamp: Res<Timestamp>,
    player_q: Query<&Transform, With<Player>>,
) {
    if !input.just_pressed(KeyCode::F5) {
        return;
    }

    if game_state.0 != Game_State::Exploring && game_state.0 != Game_State::MapOpen {
        return;
    }

    let Ok(player_tf) = player_q.get_single() else {
        warn!("save_game: player transform not found");
        return;
    };

    let data = SaveData {
        player_world: SaveVec3::from(player_tf.translation),
        player_tile: map_position.0,
        map_selection: selection.0,
        current_area: current_area.0,
        timestamp: timestamp.0,
        map_tiles: map.clone(),
    };

    if let Some(parent) = Path::new(SAVE_PATH).parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            warn!("save_game: failed to create save directory: {}", e);
            return;
        }
    }

    match serde_json::to_string_pretty(&data) {
        Ok(json) => {
            if let Err(e) = fs::write(SAVE_PATH, json) {
                warn!("save_game: failed to write save file: {}", e);
            } else {
                info!("Saved game to {}", SAVE_PATH);
            }
        }
        Err(e) => warn!("save_game: failed to serialize save data: {}", e),
    }
}

pub fn load_game(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    mut map: ResMut<MapTiles>,
    mut selection: ResMut<MapSelection>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<Timestamp>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_q: Query<&mut Transform, (With<crate::core::MainCamera>, Without<Player>)>,
) {
    if !input.just_pressed(KeyCode::F9) {
        return;
    }

    let Ok(contents) = fs::read_to_string(SAVE_PATH) else {
        warn!("load_game: save file not found at {}", SAVE_PATH);
        return;
    };

    let Ok(data) = serde_json::from_str::<SaveData>(&contents) else {
        warn!("load_game: failed to parse save file");
        return;
    };

    map.tiles = data.map_tiles.tiles;
    selection.0 = data.map_selection;
    map_position.0 = data.player_tile;
    current_area.0 = data.current_area;
    timestamp.0 = data.timestamp;

    if let Ok(mut player_tf) = player_q.get_single_mut() {
        player_tf.translation = Vec3::from(data.player_world);
    }
    if let Ok(mut cam_tf) = camera_q.get_single_mut() {
        cam_tf.translation = Vec3::from(data.player_world);
    }

    game_state.0 = Game_State::Exploring;
    info!("Loaded game from {}", SAVE_PATH);
}
