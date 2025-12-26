use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::constants::{
    GRID_HEIGHT, GRID_WIDTH, PATH_DRAW_MARGIN, PATH_MOVEMENT_SPEED, PLAYER_SPEED, WALKING_LIMIT,
};
use crate::core::{GameState, Game_State, Global_Variables, MainCamera, Player, Position};
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
        Query<(&mut Transform, &mut Position), With<Player>>,
        Query<(&mut Transform, &mut Position), With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    game_state: Res<GameState>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    // Ensure we only move while actually exploring (ignore other game modes).
    if game_state.0 != Game_State::Exploring {
        info!("player_movement skipped; current state {:?}", game_state.0);
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

            for (mut transform, mut position) in p0.iter_mut() {
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
                        position.x = new_x as i32;
                        position.y = new_y as i32;
                    }
                }
            }
        } else {
            for (mut transform, mut position) in param_set.p0().iter_mut() {
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
                        position.x = new_x as i32;
                        position.y = new_y as i32;
                    }
                }
            }
        }
        if camera_locked && (new_x_out.is_some() || new_y_out.is_some()) {
            let new_x = new_x_out.unwrap();
            let new_y = new_y_out.unwrap();
            for (mut transform_c, mut position_c) in param_set.p1().iter_mut() {
                transform_c.translation.x = new_x;
                transform_c.translation.y = new_y;
                position_c.x = new_x as i32;
                position_c.y = new_y as i32;
            }
        }
    }
}

pub fn follow_path_system(
    mut commands: Commands,
    mut query: Query<(&mut Transform, &mut Position, &mut MoveAlongPath, Entity), Without<MainCamera>>,
    time: Res<Time>,
    mut global_variables: ResMut<Global_Variables>,
) {
    global_variables.0.moving = true;
    for (mut transform, mut position, mut movement, entity) in query.iter_mut() {
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
                position.x = next_tile.x;
                position.y = next_tile.y;

                movement.current_index += 1;
            } else {
                commands.entity(entity).remove::<MoveAlongPath>();
            }
        }
    }
    global_variables.0.moving = false;
}

pub fn toggle_camera_lock(
    mut param_set: ParamSet<(
        Query<(&mut Transform, &mut Position), With<Player>>,
        Query<(&mut Transform, &mut Position), With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.just_pressed(KeyCode::KeyL) {
        if !param_set.p2().0.camera_locked {
            param_set.p2().0.camera_locked = true;

            let mut position_x: i32 = 0;
            let mut position_y: i32 = 0;
            let mut transform_x: f32 = 0.0;
            let mut transform_y: f32 = 0.0;

            for (player_transform, player_position) in param_set.p0().iter_mut() {
                position_x = player_position.x;
                position_y = player_position.y;
                transform_x = player_transform.translation.x;
                transform_y = player_transform.translation.y;
            }

            for (mut camera_transform, mut camera_position) in param_set.p1().iter_mut() {
                camera_position.x = position_x;
                camera_position.y = position_y;
                camera_transform.translation.x = transform_x;
                camera_transform.translation.y = transform_y;
            }
        } else {
            param_set.p2().0.camera_locked = false;
        }
    }
}

pub fn mouse_click(
    mut param_set: ParamSet<(Query<(Entity, &mut Transform, &mut Position), With<Player>>, )>,
    game_state: Res<GameState>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
) {
    if input.just_pressed(MouseButton::Left) {
        let mut p0 = param_set.p0();
        let Some((entity, mut _transform, mut position)) = p0.iter_mut().next() else {
            warn!("mouse_click: left click but no player entity found");
            return;
        };

        let player_entity = entity;

        let path_ops = find_path(
            *position,
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
        let path = path_ops.unwrap();
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
        let Some((_entity, _transform, position)) = p0.iter_mut().next() else {
            warn!("mouse_click: right click but no player entity found");
            return;
        };

        let path_ops = find_path(
            *position,
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
        let path = path_ops.unwrap();
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
