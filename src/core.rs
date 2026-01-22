use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game_State {
    MainMenu,
    Exploring,
    Interacting,
    Shopping,
    Battle,
    Traveling,
    Paused,
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
            camera_locked: false,
        }
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct MainCamera;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Component)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

impl Default for Position {
    fn default() -> Self {
        Position { x: 0, y: 0 }
    }
}
