use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::combat_plugin::TurnManager;
use crate::core::{GameState, Game_State, Position, Timestamp};

pub fn enter_battle(
    mut game_state: ResMut<GameState>,
    _input: Res<ButtonInput<KeyCode>>,
    _turn_manager: Res<TurnManager>,
) {
    game_state.0 = Game_State::Battle;
}

pub fn pass_turn(
    _game_state: ResMut<GameState>,
    _input: Res<ButtonInput<KeyCode>>,
    _turn_manager: Res<TurnManager>,
    _timestamp: Res<Timestamp>,
) {
}

pub fn end_battle(
    mut game_state: ResMut<GameState>,
    _input: Res<ButtonInput<KeyCode>>,
    _turn_manager: Res<TurnManager>,
) {
    game_state.0 = Game_State::Exploring;
}

pub fn get_travel(_map_position: Res<crate::core::PlayerMapPosition>, _target_position: Position) -> Option<u32> {
    None
}

pub fn confirm_travel(
    _game_state: ResMut<GameState>,
    _input: Res<ButtonInput<KeyCode>>,
    _map_position: Res<crate::core::PlayerMapPosition>,
    _target_position: Position,
    _timestamp: Res<Timestamp>,
) {
}

pub fn walk_to_tile(
    _game_state: ResMut<GameState>,
    _input: Res<ButtonInput<KeyCode>>,
    _map_position: Res<crate::core::PlayerMapPosition>,
    _target_position: Position,
    _timestamp: Res<Timestamp>,
) {
}

pub fn trigger_event(_event_id: u32) {}
