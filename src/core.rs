use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game_State {
    MainMenu,
    /// Pre-game roster screen: pick the four-character party for this run.
    PartySelection,
    Exploring,
    Interacting,
    Shopping,
    Battle,
    MapOpen,
    /// The world-map overlay listing named areas as a node graph; pick a
    /// destination to begin overland travel. Opened with `M`.
    WorldMapOpen,
    Traveling,
    Paused,
    /// The character / skill-tree overlay, opened with `K` while exploring.
    /// Lets the player browse and spend skill points on the party leader.
    SkillTree,
}

#[derive(Resource)]
pub struct GameState(pub Game_State);

impl Default for GameState {
    fn default() -> Self {
        GameState(Game_State::Exploring)
    }
}

#[derive(Resource, Default)]
pub struct Global_Variables(pub GlobalVariables);

#[derive(Resource, Default)]
pub struct PlayerMapPosition(pub Position);

#[derive(Resource)]
pub struct Timestamp(pub u32);

pub struct GlobalVariables {
    pub moving: bool,
    pub camera_locked: bool,
}

impl Default for GlobalVariables {
    fn default() -> Self {
        GlobalVariables {
            moving: false,
            camera_locked: true,
        }
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct MainCamera;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Component, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

impl Default for Position {
    fn default() -> Self {
        Position { x: 0, y: 0 }
    }
}
