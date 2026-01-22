use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::constants::{
    GRID_HEIGHT, GRID_WIDTH, PATH_DRAW_MARGIN, PATH_MOVEMENT_SPEED, PLAYER_SPEED, WALKING_LIMIT,
};
use crate::core::{GameState, Game_State, Global_Variables, MainCamera, Player, Position, Timestamp};
use crate::map::{time_cost_for_tile, MapTiles, TILE_WORLD_SIZE};
use crate::pathfinding::{is_walkable_move, pathfinding};
use crate::quadtree::QuadTree;

#[derive(Component)]
pub struct FadeOutTimer(pub Timer);

#[derive(Component)]
pub struct AnimationIndices {
    pub first: usize,
    pub last: usize,
}

#[derive(Component, Deref, DerefMut)]
pub struct AnimationTimer(pub Timer);

#[derive(Component)]
pub struct MoveAlongPath {
    pub path: Vec<IVec2>,
    pub current_index: usize,
    pub timer: Timer,
}

#[derive(Resource, Default)]
pub struct TravelTimeAccumulator {
    pub last_tile: Option<IVec2>,
}

pub fn fade_out_system(mut commands: Commands, time: Res<Time>, mut query: Query<(Entity, &mut FadeOutTimer, &mut Sprite)>) {
    for (entity, mut timer, mut sprite) in query.iter_mut() {
        timer.0.tick(time.delta());
        if timer.0.just_finished() {
            commands.entity(entity).despawn();
        } else {
            let r = sprite.color.to_srgba().red;
            let g = sprite.color.to_srgba().green;
            let b = sprite.color.to_srgba().blue;
            let a = sprite.color.alpha();
            let new_alpha = (a - 0.01).max(0.0);
            sprite.color = Color::srgba(r, g, b, new_alpha);
        }
    }
}

pub fn animate_sprite(
    time: Res<Time>,
    mut query: Query<(&AnimationIndices, &mut AnimationTimer, &mut Sprite)>,
) {
    for (indices, mut timer, mut sprite) in &mut query {
        timer.tick(time.delta());

        if timer.just_finished() {
            if let Some(atlas) = &mut sprite.texture_atlas {
                atlas.index = if atlas.index == indices.last {
                    indices.first
                } else {
                    atlas.index + 1
                };
            }
        }
    }
}

pub fn player_movement(
    mut param_set: ParamSet<(
        Query<&mut Transform, With<Player>>,
        Query<&mut Transform, With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    game_state: Res<GameState>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    // Ensure we only move while actually exploring (ignore other game modes).
    if game_state.0 != Game_State::Exploring {
        return;
    }

    let mut direction = Vec2::ZERO;

    if input.pressed(KeyCode::KeyW) {
        direction.y += 1.0;
    }
    if input.pressed(KeyCode::KeyS) {
        direction.y -= 1.0;
    }
    if input.pressed(KeyCode::KeyD) {
        direction.x += 1.0;
    }
    if input.pressed(KeyCode::KeyA) {
        direction.x -= 1.0;
    }

    let movement_speed = PLAYER_SPEED * time.delta_secs();

    let camera_locked = param_set.p2().0.camera_locked;

    if direction.length() > 0.0 {
        info!(
            "player_movement direction {:?}, speed {} (camera_locked={})",
            direction, movement_speed, camera_locked
        );
        let mut new_x_out: Option<f32> = None;
        let mut new_y_out: Option<f32> = None;

        if direction.x != 0.0 && direction.y != 0.0 {
            let diagonal_speed = movement_speed / (2.0_f32.sqrt());

            let mut p0 = param_set.p0();

            for mut transform in p0.iter_mut() {
                let new_x = transform.translation.x + direction.x * diagonal_speed;
                let new_y = transform.translation.y + direction.y * diagonal_speed;

                new_x_out = Some(new_x);
                new_y_out = Some(new_y);

                transform.rotation = Quat::from_rotation_z(rotate_to_direction(
                    transform.translation.x,
                    transform.translation.y,
                    new_x,
                    new_y,
                ));

                if ((new_x.abs() as u32) < GRID_WIDTH)
                    && ((new_y.abs() as u32) < GRID_HEIGHT)
                {
                    let new_pos = Position {
                        x: new_x as i32,
                        y: new_y as i32,
                    };

                    if is_walkable_move(new_pos, &quad_tree) {
                        transform.translation.x = new_x;
                        transform.translation.y = new_y;
                    }
                }
            }
        } else {
            for mut transform in param_set.p0().iter_mut() {
                let new_x = transform.translation.x + direction.x * movement_speed;
                let new_y = transform.translation.y + direction.y * movement_speed;

                new_x_out = Some(new_x);
                new_y_out = Some(new_y);

                transform.rotation = Quat::from_rotation_z(rotate_to_direction(
                    transform.translation.x,
                    transform.translation.y,
                    new_x,
                    new_y,
                ));

                if ((new_x.abs() as u32) < GRID_WIDTH)
                    && ((new_y.abs() as u32) < GRID_HEIGHT)
                {
                    let new_pos = Position {
                        x: new_x as i32,
                        y: new_y as i32,
                    };

                    if is_walkable_move(new_pos, &quad_tree) {
                        transform.translation.x = new_x;
                        transform.translation.y = new_y;
                    }
                }
            }
        }
        if camera_locked {
            if let (Some(new_x), Some(new_y)) = (new_x_out, new_y_out) {
                for mut transform_c in param_set.p1().iter_mut() {
                    transform_c.translation.x = new_x;
                    transform_c.translation.y = new_y;
                }
            } else {
                warn!("Camera lock update skipped due to missing coordinates");
            }
        }
    }
}

pub fn follow_path_system(
    mut commands: Commands,
    mut query: Query<(&mut Transform, &mut MoveAlongPath, Entity), Without<MainCamera>>,
    time: Res<Time>,
    mut global_variables: ResMut<Global_Variables>,
    game_state: Res<GameState>,
) {
    if !(matches!(game_state.0, Game_State::Exploring)) {
        return;
    }

    global_variables.0.moving = true;
    for (mut transform, mut movement, entity) in query.iter_mut() {
        if movement
            .timer
            .tick(time.delta() * PATH_MOVEMENT_SPEED)
            .just_finished()
        {
            if movement.current_index < movement.path.len() {
                let next_tile = movement.path[movement.current_index];
                let target_x = next_tile.x as f32;
                let target_y = next_tile.y as f32;

                transform.rotation = Quat::from_rotation_z(rotate_to_direction(
                    transform.translation.x,
                    transform.translation.y,
                    target_x,
                    target_y,
                ));
                transform.translation.x = target_x;
                transform.translation.y = target_y;

                movement.current_index += 1;
            } else {
                commands.entity(entity).remove::<MoveAlongPath>();
            }
        }
    }
    global_variables.0.moving = false;
}

/// Advance in-world time when the player manually walks (not along an auto path).
pub fn accumulate_manual_travel_time(
    mut tracker: ResMut<TravelTimeAccumulator>,
    mut timestamp: ResMut<Timestamp>,
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    player_q: Query<(&Transform, Option<&MoveAlongPath>), With<Player>>,
) {
    if game_state.0 != Game_State::Exploring {
        tracker.last_tile = None;
        return;
    }

    let Ok((transform, move_path)) = player_q.single() else {
        return;
    };
    let current = transform.translation.truncate();

    // Do not accrue time while on an auto path.
    if move_path.is_some() {
        tracker.last_tile = Some(IVec2::new(
            (current.x / TILE_WORLD_SIZE).floor() as i32,
            (current.y / TILE_WORLD_SIZE).floor() as i32,
        ));
        return;
    }

    let tile_coords = IVec2::new(
        (current.x / TILE_WORLD_SIZE).floor() as i32,
        (current.y / TILE_WORLD_SIZE).floor() as i32,
    );

    // Only increment time when entering a new tile.
    if tracker.last_tile.map_or(true, |prev| prev != tile_coords) {
        let height = map.tiles.len() as i32;
        let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;
        if tile_coords.x >= 0
            && tile_coords.y >= 0
            && tile_coords.x < width
            && tile_coords.y < height
        {
                if let Some(row) = map.tiles.get(tile_coords.y as usize) {
                if let Some(tile) = row.get(tile_coords.x as usize) {
                    timestamp.0 = timestamp.0.saturating_add(time_cost_for_tile(tile));
                }
            }
        }
    }

    tracker.last_tile = Some(tile_coords);
}

pub fn toggle_camera_lock(
    mut param_set: ParamSet<(
        Query<&mut Transform, With<Player>>,
        Query<&mut Transform, With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.just_pressed(KeyCode::KeyL) {
        if !param_set.p2().0.camera_locked {
            param_set.p2().0.camera_locked = true;

            let mut transform_x: f32 = 0.0;
            let mut transform_y: f32 = 0.0;

            for player_transform in param_set.p0().iter_mut() {
                transform_x = player_transform.translation.x;
                transform_y = player_transform.translation.y;
            }

            for mut camera_transform in param_set.p1().iter_mut() {
                camera_transform.translation.x = transform_x;
                camera_transform.translation.y = transform_y;
            }
        } else {
            param_set.p2().0.camera_locked = false;
        }
    }
}

pub fn mouse_click(
    mut param_set: ParamSet<(Query<(Entity, &Transform), With<Player>>, )>,
    game_state: Res<GameState>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
) {

    if !(matches!(game_state.0, Game_State::Exploring)) {
        return;
    }
    
    if input.just_pressed(MouseButton::Left) {
        let mut p0 = param_set.p0();
        let Some((entity, transform)) = p0.iter_mut().next() else {
            warn!("mouse_click: left click but no player entity found");
            return;
        };

        let player_entity = entity;
        let current_position = Position {
            x: transform.translation.x as i32,
            y: transform.translation.y as i32,
        };

        let path_ops = find_path(
            current_position,
            game_state.0,
            quad_tree,
            camera_query,
            windows,
            PATH_DRAW_MARGIN,
        );
        if path_ops.is_none() {
            info!("mouse_click: left click produced no path");
            return;
        }
        let path = match path_ops {
            Some(p) => p,
            None => {
                warn!("mouse_click: path calculation unexpectedly missing after Some check");
                return;
            }
        };
        if path.is_empty() {
            info!("mouse_click: left click produced empty path");
            return;
        }
        let path_len = path.len();
        if path_len > WALKING_LIMIT {
            info!(
                "mouse_click: left click path too long ({} > limit {})",
                path_len, WALKING_LIMIT
            );
            return;
        }

        if path_len > 1 {
            let path_iv2: Vec<IVec2> = path.iter().map(|p| IVec2::new(p.x, p.y)).collect();
            info!("mouse_click: moving along path with {} steps", path_iv2.len());
            commands.entity(player_entity).insert(MoveAlongPath {
                path: path_iv2,
                current_index: 1,
                timer: Timer::from_seconds(0.3, TimerMode::Repeating),
            });
        }
    } else if input.just_pressed(MouseButton::Right) {
        let mut p0 = param_set.p0();
        let Some((_entity, transform)) = p0.iter_mut().next() else {
            warn!("mouse_click: right click but no player entity found");
            return;
        };
        let current_position = Position {
            x: transform.translation.x as i32,
            y: transform.translation.y as i32,
        };

        let path_ops = find_path(
            current_position,
            game_state.0,
            quad_tree,
            camera_query,
            windows,
            PATH_DRAW_MARGIN,
        );
        if path_ops.is_none() {
            info!("mouse_click: right click produced no path");
            return;
        }
        let path = match path_ops {
            Some(p) => p,
            None => {
                warn!("mouse_click: path calculation unexpectedly missing after Some check");
                return;
            }
        };
        if path.is_empty() {
            info!("mouse_click: right click produced empty path");
            return;
        }
        let path_len = path.len();
        if path_len > WALKING_LIMIT {
            info!(
                "mouse_click: right click path too long ({} > limit {})",
                path_len, WALKING_LIMIT
            );
            return;
        }

        if path_len > 1 {
            for i in 1..path_len {
                let next_tile = path[i];
                let target_x = next_tile.x as f32;
                let target_y = next_tile.y as f32;

                commands
                    .spawn((
                        Sprite {
                            image: asset_server.load("dot.png"),
                            custom_size: Some(Vec2::new(10.0, 10.0)),
                            ..default()
                        },
                        Transform::from_xyz(target_x, target_y, 0.0),
                    ))
                    .insert(FadeOutTimer(Timer::from_seconds(1.0, TimerMode::Once)));
            }
        }
    }
}

fn find_path(
    position: Position,
    game_state: Game_State,
    quad_tree: Res<QuadTree>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    windows: Query<&Window>,
    margin: i32,
) -> Option<Vec<Position>> {
    match game_state {
        Game_State::Exploring => {
            let (camera, camera_transform) = camera_query.single().expect("Failed to get camera");
            let window = windows.single().expect("Failed to get window");

            if let Some(screen_pos) = window.cursor_position() {
                let current_position = Position {
                    x: position.x,
                    y: position.y,
                };

                let target_position = match camera.viewport_to_world_2d(camera_transform, screen_pos) {
                    Ok(target_position) => target_position,
                    Err(_) => return None,
                };

                let target_position: Position = Position {
                    x: target_position.x as i32,
                    y: target_position.y as i32,
                };

                let path = pathfinding(&quad_tree, current_position, target_position, margin);
                if path.is_empty() {
                    return None;
                }

                return Some(path);
            } else {
                return None;
            }
        }
        Game_State::Interacting => None,
        Game_State::Battle => None,
        _ => None,
    }
}

fn rotate_to_direction(start_x: f32, start_y: f32, destination_x: f32, destination_y: f32) -> f32 {
    let dx = destination_x - start_x;
    let dy = destination_y - start_y;
    let angle = (dy as f32).atan2(dx as f32);
    angle
}
