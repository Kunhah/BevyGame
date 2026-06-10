use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::constants::{
    GRID_HEIGHT, GRID_WIDTH, PATH_DRAW_MARGIN, PATH_MOVEMENT_SPEED, PLAYER_SPEED, WALKING_LIMIT,
};
use crate::battle::{CombatMovePoints, CombatMoveTarget, WorldAlly};
use crate::core::{GameState, Game_State, Global_Variables, MainCamera, Player, Position};
use crate::map::{
    movement_speed_multiplier_at_world, movement_speed_multiplier_with_effects_at_world,
    MapTiles, TerrainSlowEffectIndex, TILE_WORLD_SIZE,
};
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

pub fn fade_out_system(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut FadeOutTimer)>,
) {
    // Path-preview markers are 3D meshes now (no sprite alpha to fade), so this
    // just despawns them when their timer elapses.
    for (entity, mut timer) in query.iter_mut() {
        if timer.0.tick(time.delta()).just_finished() {
            commands.entity(entity).despawn();
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
        Query<
            (
                Entity,
                &mut Transform,
                Option<&mut CombatMovePoints>,
                Option<&CombatMoveTarget>,
            ),
            With<Player>,
        >,
        Query<&mut Transform, With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    game_state: Res<GameState>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    map_tiles: Option<Res<MapTiles>>,
    slow_effects: Option<Res<TerrainSlowEffectIndex>>,
    mut commands: Commands,
) {
    // Allow exploration and battle movement; other modes are blocked.
    if game_state.0 != Game_State::Exploring && game_state.0 != Game_State::Battle {
        return;
    }

    // WSAD now drives the camera (see render3d::drive_camera); the player moves
    // by click-to-move pathfinding (exploration: MoveAlongPath; battle:
    // CombatMoveTarget). `direction` is only populated from a battle move target
    // below.
    let mut direction = Vec2::ZERO;
    let _ = &input;

    let base_movement_speed = PLAYER_SPEED * time.delta_secs();

    let battle_move = game_state.0 == Game_State::Battle;

    if direction.length() == 0.0 && battle_move {
        let mut p0 = param_set.p0();
        if let Some((entity, transform, _mp_opt, target_opt)) = p0.iter_mut().next() {
            if let Some(target) = target_opt {
                let to_target = target.target - transform.translation.truncate();
                if to_target.length_squared() <= 0.25 {
                    commands.entity(entity).remove::<CombatMoveTarget>();
                } else {
                    direction = to_target.normalize_or_zero();
                }
            }
        }
    }

    let bounds = map_tiles.as_ref().and_then(|map| {
        let height = map.tiles.len() as f32;
        let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as f32;
        if width == 0.0 || height == 0.0 {
            None
        } else {
            Some((
                0.0,
                0.0,
                width * TILE_WORLD_SIZE,
                height * TILE_WORLD_SIZE,
            ))
        }
    });
    let within_bounds = |x: f32, y: f32| {
        if let Some((min_x, min_y, max_x, max_y)) = bounds {
            x >= min_x && x <= max_x && y >= min_y && y <= max_y
        } else {
            (x.abs() as u32) < GRID_WIDTH && (y.abs() as u32) < GRID_HEIGHT
        }
    };

    if direction.length() > 0.0 {
        // info!(
        //     "player_movement direction {:?}, speed {} (camera_locked={})",
        //     direction, movement_speed, camera_locked
        // );
        let mut new_x_out: Option<f32> = None;
        let mut new_y_out: Option<f32> = None;

        if direction.x != 0.0 && direction.y != 0.0 {
            let mut p0 = param_set.p0();

            for (entity, mut transform, mut mp_opt, target_opt) in p0.iter_mut() {
                let mut remaining = mp_opt.as_ref().map(|mp| mp.remaining).unwrap_or(0.0);
                if game_state.0 == Game_State::Battle {
                    if mp_opt.is_none() || remaining <= 0.0 {
                        info!(
                            "Battle move blocked (diagonal): has_points={}, remaining={:.2}",
                            mp_opt.is_some(),
                            remaining
                        );
                        continue;
                    }
                }
                let terrain_factor = if battle_move {
                    1.0
                } else {
                    match (map_tiles.as_ref(), slow_effects.as_ref()) {
                        (Some(map), Some(effects)) => movement_speed_multiplier_with_effects_at_world(
                            map,
                            effects,
                            transform.translation.truncate(),
                        ),
                        (Some(map), None) => {
                            movement_speed_multiplier_at_world(map, transform.translation.truncate())
                        }
                        (None, _) => 1.0,
                    }
                };
                let movement_speed = base_movement_speed * terrain_factor;
                let diagonal_speed = movement_speed / (2.0_f32.sqrt());
                let new_x = transform.translation.x + direction.x * diagonal_speed;
                let new_y = transform.translation.y + direction.y * diagonal_speed;

                transform.rotation = Quat::from_rotation_z(rotate_to_direction(
                    transform.translation.x,
                    transform.translation.y,
                    new_x,
                    new_y,
                ));

                if within_bounds(new_x, new_y) {
                    let new_pos = Position {
                        x: new_x as i32,
                        y: new_y as i32,
                    };

                    if is_walkable_move(new_pos, &quad_tree) {
                        let mut step = diagonal_speed;
                        if battle_move {
                            if step > remaining {
                                step = remaining;
                            }
                            remaining -= step;
                        }
                        transform.translation.x = transform.translation.x + direction.x * step;
                        transform.translation.y = transform.translation.y + direction.y * step;
                        new_x_out = Some(transform.translation.x);
                        new_y_out = Some(transform.translation.y);
                        if battle_move {
                            if let Some(ref mut mp) = mp_opt {
                                mp.remaining = remaining;
                                info!(
                                    "Battle move (diagonal) ok: remaining={:.2}",
                                    mp.remaining
                                );
                            }
                            if let Some(target) = target_opt {
                                if transform.translation.truncate().distance(target.target) <= 0.5 {
                                    commands.entity(entity).remove::<CombatMoveTarget>();
                                }
                            }
                        }
                    } else if battle_move {
                        info!("Battle move blocked (diagonal): not walkable");
                    }
                } else if battle_move {
                    info!(
                        "Battle move blocked (diagonal): out of bounds new=({:.2},{:.2})",
                        new_x, new_y
                    );
                }
            }
        } else {
            for (entity, mut transform, mut mp_opt, target_opt) in param_set.p0().iter_mut() {
                let mut remaining = mp_opt.as_ref().map(|mp| mp.remaining).unwrap_or(0.0);
                if game_state.0 == Game_State::Battle {
                    if mp_opt.is_none() || remaining <= 0.0 {
                        info!(
                            "Battle move blocked: has_points={}, remaining={:.2}",
                            mp_opt.is_some(),
                            remaining
                        );
                        continue;
                    }
                }
                let terrain_factor = if battle_move {
                    1.0
                } else {
                    match (map_tiles.as_ref(), slow_effects.as_ref()) {
                        (Some(map), Some(effects)) => movement_speed_multiplier_with_effects_at_world(
                            map,
                            effects,
                            transform.translation.truncate(),
                        ),
                        (Some(map), None) => {
                            movement_speed_multiplier_at_world(map, transform.translation.truncate())
                        }
                        (None, _) => 1.0,
                    }
                };
                let movement_speed = base_movement_speed * terrain_factor;
                let new_x = transform.translation.x + direction.x * movement_speed;
                let new_y = transform.translation.y + direction.y * movement_speed;

                transform.rotation = Quat::from_rotation_z(rotate_to_direction(
                    transform.translation.x,
                    transform.translation.y,
                    new_x,
                    new_y,
                ));

                if within_bounds(new_x, new_y) {
                    let new_pos = Position {
                        x: new_x as i32,
                        y: new_y as i32,
                    };

                    if is_walkable_move(new_pos, &quad_tree) {
                        let mut step = movement_speed;
                        if battle_move {
                            if step > remaining {
                                step = remaining;
                            }
                            remaining -= step;
                        }
                        transform.translation.x = transform.translation.x + direction.x * step;
                        transform.translation.y = transform.translation.y + direction.y * step;
                        new_x_out = Some(transform.translation.x);
                        new_y_out = Some(transform.translation.y);
                        if battle_move {
                            if let Some(ref mut mp) = mp_opt {
                                mp.remaining = remaining;
                                info!("Battle move ok: remaining={:.2}", mp.remaining);
                            }
                            if let Some(target) = target_opt {
                                if transform.translation.truncate().distance(target.target) <= 0.5 {
                                    commands.entity(entity).remove::<CombatMoveTarget>();
                                }
                            }
                        }
                    } else if battle_move {
                        info!("Battle move blocked: not walkable");
                    }
                } else if battle_move {
                    info!(
                        "Battle move blocked: out of bounds new=({:.2},{:.2})",
                        new_x, new_y
                    );
                }
            }
        }
        // Camera following is owned solely by `camera_follow_player` (it applies
        // the fixed isometric offset). The old 2D code snapped the camera onto
        // the player's x/y here, which fought the iso follow and caused jitter.
        let _ = (new_x_out, new_y_out);
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
                let target = Vec3::new(
                    next_tile.x as f32,
                    next_tile.y as f32,
                    transform.translation.z,
                );

                transform.rotation = Quat::from_rotation_z(rotate_to_direction(
                    transform.translation.x,
                    transform.translation.y,
                    target.x,
                    target.y,
                ));
                transform.translation = target;

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
    game_state: Res<GameState>,
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

    tracker.last_tile = Some(IVec2::new(
        (current.x / TILE_WORLD_SIZE).floor() as i32,
        (current.y / TILE_WORLD_SIZE).floor() as i32,
    ));
}

// `camera_follow_player` was replaced by `render3d::drive_camera`, which owns
// the camera (follow + WSAD pan nudge + Q/E spin + R/F tilt + wheel zoom).

/// `L` toggles camera follow-lock. Locked: the camera follows the player (WSAD
/// nudges); unlocked: WSAD roams the camera freely (see `render3d::drive_camera`,
/// which owns the camera transform — this only flips the flag).
pub fn toggle_camera_lock(
    mut globals: ResMut<Global_Variables>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.just_pressed(KeyCode::KeyL) {
        globals.0.camera_locked = !globals.0.camera_locked;
        info!(
            "camera {}",
            if globals.0.camera_locked { "locked (follow)" } else { "unlocked (free roam)" }
        );
    }
}

pub fn mouse_click(
    mut param_set: ParamSet<(Query<(Entity, &Transform, Option<&CombatMovePoints>), With<Player>>, )>,
    game_state: Res<GameState>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
) {

    if !(matches!(game_state.0, Game_State::Exploring | Game_State::Battle)) {
        return;
    }
    
    if input.just_pressed(MouseButton::Left) {
        let mut p0 = param_set.p0();
        let Some((entity, transform, mp_opt)) = p0.iter_mut().next() else {
            warn!("mouse_click: left click but no player entity found");
            return;
        };

        if game_state.0 == Game_State::Battle {
            let Some((camera, camera_transform)) = camera_query.iter().next() else {
                return;
            };
            let Some(window) = windows.iter().next() else {
                return;
            };
            let Some(screen_pos) = window.cursor_position() else {
                return;
            };
            let Some(target_world) =
                crate::render3d::cursor_to_ground(camera, camera_transform, screen_pos)
            else {
                return;
            };
            let remaining = mp_opt.as_ref().map(|mp| mp.remaining).unwrap_or(0.0);
            if mp_opt.is_none() {
                info!("mouse_click (battle): no move points on player");
                return;
            }
            if remaining <= 0.0 {
                info!("mouse_click (battle): no move points left this turn");
                return;
            }
            // Clamp the destination to how far the remaining move points reach,
            // so a click always walks the unit *toward* the spot (up to its
            // range) rather than refusing outright when it's too far.
            let here = transform.translation.truncate();
            let to_target = target_world - here;
            let dist = to_target.length();
            let dest = if dist <= remaining {
                target_world
            } else {
                here + to_target.normalize_or_zero() * remaining
            };
            commands
                .entity(entity)
                .insert(CombatMoveTarget { target: dest });
            info!(
                "mouse_click (battle): move toward ({:.2}, {:.2}) dist {:.2} remaining {:.2}",
                dest.x, dest.y, dist, remaining
            );
            return;
        }

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
        let Some((_entity, transform, _mp_opt)) = p0.iter_mut().next() else {
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
                        crate::render3d::PlaceholderVisual::prop(
                            Color::srgb(0.9, 0.9, 0.3),
                            Vec2::splat(10.0),
                            10.0,
                        ),
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

                let Some(target_world) =
                    crate::render3d::cursor_to_ground(camera, camera_transform, screen_pos)
                else {
                    return None;
                };

                let target_position: Position = Position {
                    x: target_world.x as i32,
                    y: target_world.y as i32,
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

/// Per-frame leash that pulls every `WorldAlly` toward the player when the
/// gap exceeds `LEASH_DISTANCE`, holding off once they're within
/// `STOP_DISTANCE` so the party doesn't pile on the player's tile. Only runs
/// while exploring (battle has its own movement, and travel/menus shouldn't
/// drag allies around).
///
/// Movement is straight-line for now — pathfinding through obstacles is a
/// future polish pass; the existing followers occupy walkable space near the
/// player so this works in practice.
pub fn ally_follow_player_system(
    time: Res<Time>,
    game_state: Res<GameState>,
    player_q: Query<&Transform, With<Player>>,
    mut ally_q: Query<&mut Transform, (With<WorldAlly>, Without<Player>)>,
) {
    if game_state.0 != Game_State::Exploring {
        return;
    }
    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    // Tunables — kept as locals so they're easy to tweak without restructuring.
    const LEASH_DISTANCE: f32 = 96.0;
    const STOP_DISTANCE: f32 = 48.0;
    const FOLLOW_SPEED: f32 = PLAYER_SPEED;

    for mut ally_tf in ally_q.iter_mut() {
        let ally_pos = ally_tf.translation.truncate();
        let to_player = player_pos - ally_pos;
        let distance = to_player.length();
        if distance <= STOP_DISTANCE {
            continue;
        }
        let dir = to_player / distance.max(0.0001);
        // Slow down as the ally approaches `STOP_DISTANCE` so they don't
        // overshoot and oscillate around the player.
        let approach_factor = ((distance - STOP_DISTANCE) / LEASH_DISTANCE).clamp(0.0, 1.0);
        let step = FOLLOW_SPEED * time.delta_secs() * approach_factor.max(0.25);
        let move_vec = dir * step.min(distance - STOP_DISTANCE);
        ally_tf.translation.x += move_vec.x;
        ally_tf.translation.y += move_vec.y;
    }
}
