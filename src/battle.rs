use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::combat_plugin::{
    Abilities, AccumulatedAgility, CombatStats, Experience, GrowthAttributes, Health, Level, Magic,
    PendingPlayerAction, PlayerAction, PlayerActionEvent, PlayerControlled, StatModifiers, Stamina,
    TurnManager, TurnOrder, TurnStartEvent,
};
use crate::constants::{GRID_HEIGHT, GRID_WIDTH, PLAYER_SPEED};
use crate::core::{GameState, Game_State, Global_Variables, MainCamera, Player, Position};
use crate::pathfinding::is_walkable_move;
use crate::quadtree::QuadTree;

#[derive(Component, Clone, Copy, Debug)]
pub struct EnemyEncounter {
    pub id: u32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct WorldNpc {
    pub id: u32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct WorldAlly;

#[derive(Component, Clone, Copy, Debug)]
pub enum BattleSide {
    Ally,
    Enemy,
}

#[derive(Component)]
pub struct BattleParticipant;

#[derive(Component, Clone, Copy, Debug)]
pub struct BattleWorldLink {
    pub world_entity: Entity,
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CombatMovePoints {
    pub remaining: f32,
    pub max: f32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct CombatMoveTarget {
    pub target: Vec2,
}

#[derive(Resource, Default)]
pub struct BattleState {
    pub active: bool,
    pub participants: Vec<Entity>,
    pub enemy_id: Option<u32>,
}

pub fn battle_trigger_system(
    mut commands: Commands,
    mut game_state: ResMut<GameState>,
    mut battle_state: ResMut<BattleState>,
    mut tm: ResMut<TurnManager>,
    mut turn_order: ResMut<TurnOrder>,
    input: Res<ButtonInput<KeyCode>>,
    player_q: Query<(Entity, &Transform), With<Player>>,
    enemy_q: Query<(Entity, &Transform, &EnemyEncounter)>,
    ally_q: Query<(Entity, &Transform), With<WorldAlly>>,
) {
    if game_state.0 != Game_State::Exploring || battle_state.active {
        return;
    }

    if !input.just_pressed(KeyCode::KeyE) {
        return;
    }

    let Ok((player_entity, player_tf)) = player_q.single() else {
        return;
    };

    let player_pos = player_tf.translation.truncate();
    for (enemy_entity, enemy_tf, encounter) in enemy_q.iter() {
        let enemy_pos = enemy_tf.translation.truncate();
        if player_pos.distance(enemy_pos) <= 32.0 {
            game_state.0 = Game_State::Battle;
            start_battle(
                &mut commands,
                &mut battle_state,
                &mut tm,
                &mut turn_order,
                encounter.id,
                enemy_entity,
                player_entity,
                player_tf.translation,
                enemy_tf.translation,
                ally_q.iter().map(|(e, t)| (e, t.clone())).collect(),
            );
            break;
        }
    }
}

fn start_battle(
    commands: &mut Commands,
    battle_state: &mut BattleState,
    tm: &mut TurnManager,
    turn_order: &mut TurnOrder,
    enemy_id: u32,
    enemy_world_entity: Entity,
    player_world_entity: Entity,
    player_world_pos: Vec3,
    enemy_world_pos: Vec3,
    allies_world: Vec<(Entity, Transform)>,
) {
    battle_state.active = true;
    battle_state.enemy_id = Some(enemy_id);

    let player = spawn_player_combat(commands, player_world_entity, player_world_pos);
    let mut participants = vec![player];
    for (ally_entity, ally_tf) in allies_world {
        let ally = spawn_ally_combat(commands, ally_entity, ally_tf.translation);
        participants.push(ally);
    }
    let enemy = spawn_enemy_combat(commands, enemy_id, enemy_world_pos);
    participants.push(enemy);

    battle_state.participants = participants;
    tm.participants = battle_state.participants.clone();
    turn_order.queue.clear();

    commands.entity(enemy_world_entity).despawn();
    info!("Battle started against enemy {}", enemy_id);
}

fn spawn_player_combat(commands: &mut Commands, world_entity: Entity, world_pos: Vec3) -> Entity {
    let mut e = commands.spawn_empty();
    e.insert(Name::new("PlayerCombat"));
    e.insert(BattleParticipant);
    e.insert(BattleSide::Ally);
    e.insert(PlayerControlled);
    e.insert(BattleWorldLink { world_entity });
    e.insert(Transform::from_translation(world_pos));
    e.insert(Health {
        current: 120,
        max: 120,
        regen: 2,
    });
    e.insert(Magic {
        current: 60,
        max: 60,
        regen: 1,
    });
    e.insert(Stamina {
        current: 90,
        max: 90,
        regen: 2,
    });
    e.insert(CombatStats {
        base_lethality: 14,
        base_hit: 80,
        base_armor: 10,
        base_agility: 10,
        base_mind: 8,
        base_morale: 90,
        movement: 5,
    });
    e.insert(GrowthAttributes {
        vitality: 12,
        endurance: 10,
        spirit: 10,
        power: 12,
        control: 10,
        agility: 10,
        insight: 10,
        resolve: 12,
    });
    e.insert(Abilities(vec![]));
    e.insert(Experience(0));
    e.insert(Level(1));
    e.insert(AccumulatedAgility(0));
    e.insert(StatModifiers(Vec::new()));
    e.insert(CombatMovePoints::default());
    e.id()
}

fn spawn_enemy_combat(commands: &mut Commands, enemy_id: u32, world_pos: Vec3) -> Entity {
    let (hp, lethality, hit, armor, agility) = match enemy_id {
        1 => (80, 10, 70, 6, 8),
        2 => (120, 14, 75, 10, 6),
        _ => (60, 8, 65, 4, 7),
    };

    let mut e = commands.spawn_empty();
    e.insert(Name::new(format!("EnemyCombat({})", enemy_id)));
    e.insert(BattleParticipant);
    e.insert(BattleSide::Enemy);
    e.insert(Transform::from_translation(world_pos));
    e.insert(Health {
        current: hp,
        max: hp,
        regen: 1,
    });
    e.insert(Magic {
        current: 20,
        max: 20,
        regen: 0,
    });
    e.insert(Stamina {
        current: 60,
        max: 60,
        regen: 1,
    });
    e.insert(CombatStats {
        base_lethality: lethality,
        base_hit: hit,
        base_armor: armor,
        base_agility: agility,
        base_mind: 6,
        base_morale: 70,
        movement: 4,
    });
    e.insert(GrowthAttributes {
        vitality: 8,
        endurance: 8,
        spirit: 6,
        power: 8,
        control: 8,
        agility: 8,
        insight: 6,
        resolve: 6,
    });
    e.insert(Abilities(vec![]));
    e.insert(Experience(0));
    e.insert(Level(1));
    e.insert(AccumulatedAgility(0));
    e.insert(StatModifiers(Vec::new()));
    e.insert(CombatMovePoints::default());
    e.id()
}

fn spawn_ally_combat(commands: &mut Commands, world_entity: Entity, world_pos: Vec3) -> Entity {
    let mut e = commands.spawn_empty();
    e.insert(Name::new("AllyCombat"));
    e.insert(BattleParticipant);
    e.insert(BattleSide::Ally);
    e.insert(PlayerControlled);
    e.insert(BattleWorldLink { world_entity });
    e.insert(Transform::from_translation(world_pos));
    e.insert(Health {
        current: 100,
        max: 100,
        regen: 2,
    });
    e.insert(Magic {
        current: 40,
        max: 40,
        regen: 1,
    });
    e.insert(Stamina {
        current: 80,
        max: 80,
        regen: 2,
    });
    e.insert(CombatStats {
        base_lethality: 12,
        base_hit: 75,
        base_armor: 8,
        base_agility: 9,
        base_mind: 8,
        base_morale: 85,
        movement: 5,
    });
    e.insert(GrowthAttributes {
        vitality: 10,
        endurance: 9,
        spirit: 8,
        power: 10,
        control: 9,
        agility: 9,
        insight: 8,
        resolve: 10,
    });
    e.insert(Abilities(vec![]));
    e.insert(Experience(0));
    e.insert(Level(1));
    e.insert(AccumulatedAgility(0));
    e.insert(StatModifiers(Vec::new()));
    e.insert(CombatMovePoints::default());
    e.id()
}

pub fn setup_player_turns(
    mut events: MessageReader<TurnStartEvent>,
    mut pending: ResMut<PendingPlayerAction>,
    mut commands: Commands,
    stats_q: Query<&CombatStats>,
    player_q: Query<(), With<PlayerControlled>>,
    link_q: Query<&BattleWorldLink>,
    mut world_mp_q: Query<&mut CombatMovePoints>,
) {
    for ev in events.read() {
        if player_q.get(ev.who).is_err() {
            continue;
        }
        if pending.entity.is_some() {
            continue;
        }
        if let Ok(stats) = stats_q.get(ev.who) {
            let movement = stats.movement.max(0) as f32;
            let max_distance = (movement * crate::constants::PLAYER_SPEED).min(250.0);
            // Always refresh points at turn start.
            commands.entity(ev.who).insert(CombatMovePoints {
                remaining: max_distance,
                max: max_distance,
            });
            info!(
                "Player turn start: set combat move points to {:.2} for {:?}",
                max_distance, ev.who
            );
            if let Ok(link) = link_q.get(ev.who) {
                if let Ok(mut mp) = world_mp_q.get_mut(link.world_entity) {
                    mp.remaining = max_distance;
                    mp.max = max_distance;
                } else {
                    commands.entity(link.world_entity).insert(CombatMovePoints {
                        remaining: max_distance,
                        max: max_distance,
                    });
                }
                info!(
                    "Player turn start: set world move points to {:.2} for {:?}",
                    max_distance, link.world_entity
                );
            }
        }
        pending.entity = Some(ev.who);
    }
}

/// Ensure the world player entity always mirrors the active combat entity's move points.
pub fn sync_combat_move_points_from_world(
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    mut combat_q: Query<(&BattleWorldLink, &mut CombatMovePoints), (With<BattleParticipant>, Without<Player>)>,
    world_q: Query<&CombatMovePoints, (With<Player>, Without<BattleParticipant>)>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }
    let Some(active) = pending.entity else {
        return;
    };
    let Ok((link, mut combat_mp)) = combat_q.get_mut(active) else {
        return;
    };
    if let Ok(world_mp) = world_q.get(link.world_entity) {
        combat_mp.remaining = world_mp.remaining;
        combat_mp.max = world_mp.max;
    }
}

pub fn combat_movement_system(
    mut param_set: ParamSet<(
        Query<(&mut Transform, &mut CombatMovePoints), With<Player>>,
        Query<&mut Transform, With<MainCamera>>,
        ResMut<Global_Variables>,
    )>,
    game_state: Res<GameState>,
    quad_tree: Res<QuadTree>,
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    // Ensure we only move while actually in battle mode.
    if game_state.0 != Game_State::Battle {
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
        let mut new_x_out: Option<f32> = None;
        let mut new_y_out: Option<f32> = None;

        if direction.x != 0.0 && direction.y != 0.0 {
            let diagonal_speed = movement_speed / (2.0_f32.sqrt());

        let mut p0 = param_set.p0();

        for (mut transform, mut mp) in p0.iter_mut() {
            if mp.remaining <= 0.0 {
                continue;
            }
            let new_x = transform.translation.x + direction.x * diagonal_speed;
            let new_y = transform.translation.y + direction.y * diagonal_speed;

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
                    let step = diagonal_speed.min(mp.remaining);
                    let final_x = transform.translation.x + direction.x * step;
                    let final_y = transform.translation.y + direction.y * step;
                    transform.translation.x = final_x;
                    transform.translation.y = final_y;
                    mp.remaining -= step;
                    info!("Combat move points remaining: {:.2}", mp.remaining);
                    new_x_out = Some(final_x);
                    new_y_out = Some(final_y);
                }
            }
        }
    } else {
        for (mut transform, mut mp) in param_set.p0().iter_mut() {
            if mp.remaining <= 0.0 {
                continue;
            }
            let new_x = transform.translation.x + direction.x * movement_speed;
            let new_y = transform.translation.y + direction.y * movement_speed;

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
                    let step = movement_speed.min(mp.remaining);
                    let final_x = transform.translation.x + direction.x * step;
                    let final_y = transform.translation.y + direction.y * step;
                    transform.translation.x = final_x;
                    transform.translation.y = final_y;
                    mp.remaining -= step;
                    info!("Combat move points remaining: {:.2}", mp.remaining);
                    new_x_out = Some(final_x);
                    new_y_out = Some(final_y);
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

fn rotate_to_direction(start_x: f32, start_y: f32, destination_x: f32, destination_y: f32) -> f32 {
    let direction = Vec2::new(destination_x - start_x, destination_y - start_y);
    direction.y.atan2(direction.x) - std::f32::consts::PI / 2.0
}

// pub fn combat_movement_system(
//     mut commands: Commands,
//     input: Res<ButtonInput<KeyCode>>,
//     game_state: Res<GameState>,
//     mut pending: ResMut<PendingPlayerAction>,
//     quad_tree: Res<crate::quadtree::QuadTree>,
//     mouse_input: Res<ButtonInput<MouseButton>>,
//     windows: Query<&Window>,
//     camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
//     mut movers: Query<
//         (
//             Entity,
//             &mut Transform,
//             &mut CombatMovePoints,
//             Option<&BattleWorldLink>,
//             Option<&CombatMoveTarget>,
//         ),
//         With<BattleParticipant>,
//     >,
//     mut world_transforms: Query<&mut Transform, (With<Player>, Without<BattleParticipant>)>,
//     time: Res<Time>,
// ) {
//     if game_state.0 != Game_State::Battle {
//         return;
//     }

//     let Some(actor) = pending.entity else {
//         return;
//     };

//     let Ok((entity, mut tf, mut mp, link, move_target)) = movers.get_mut(actor) else {
//         return;
//     };

//     if mp.remaining <= 0.0 {
//         return;
//     }

//     if mouse_input.just_pressed(MouseButton::Left) {
//         let Some(window) = windows.iter().next() else {
//             return;
//         };
//         let Some(cursor_pos) = window.cursor_position() else {
//             return;
//         };
//         let Some((camera, cam_tf)) = camera_q.iter().next() else {
//             return;
//         };
//         let Ok(world_pos) = camera.viewport_to_world_2d(cam_tf, cursor_pos) else {
//             return;
//         };
//         let cost = tf.translation.truncate().distance(world_pos);
//         if cost <= mp.remaining {
//             commands.entity(entity).insert(CombatMoveTarget { target: world_pos });
//         } else {
//             info!(
//                 "Combat move denied: cost {:.2} > remaining {:.2}",
//                 cost, mp.remaining
//             );
//         }
//         return;
//     }

//     let mut delta = Vec2::ZERO;
//     if input.pressed(KeyCode::KeyW) {
//         delta.y += 1.0;
//     }
//     if input.pressed(KeyCode::KeyS) {
//         delta.y -= 1.0;
//     }
//     if input.pressed(KeyCode::KeyD) {
//         delta.x += 1.0;
//     }
//     if input.pressed(KeyCode::KeyA) {
//         delta.x -= 1.0;
//     }
//     if delta == Vec2::ZERO {
//         if let Some(target) = move_target {
//             let to_target = target.target - tf.translation.truncate();
//             if to_target.length_squared() == 0.0 {
//                 commands.entity(entity).remove::<CombatMoveTarget>();
//                 return;
//             }
//             delta = to_target.normalize_or_zero();
//         } else {
//             return;
//         }
//     }

//     let mut movement_speed = crate::constants::PLAYER_SPEED * time.delta_secs();
//     if delta.x != 0.0 && delta.y != 0.0 && move_target.is_none() {
//         movement_speed /= 2.0_f32.sqrt();
//     }
//     let dir = delta.normalize_or_zero();
//     let step = movement_speed.min(mp.remaining);

//     let new_x = tf.translation.x + dir.x * step;
//     let new_y = tf.translation.y + dir.y * step;
//     let new_pos = crate::core::Position {
//         x: new_x as i32,
//         y: new_y as i32,
//     };
//     if crate::pathfinding::is_walkable_move(new_pos, &quad_tree) {
//         tf.translation.x = new_x;
//         tf.translation.y = new_y;
//         mp.remaining -= step;
//         if let Some(target) = move_target {
//             if tf.translation.truncate().distance(target.target) <= 0.5 {
//                 commands.entity(entity).remove::<CombatMoveTarget>();
//             }
//         }
//     }

//     let world_entity = link.map(|l| l.world_entity);
//     let new_pos = tf.translation;
//     drop(tf);
//     drop(mp);
//     drop(link);

//     if let Some(world_entity) = world_entity {
//         if let Ok(mut world_tf) = world_transforms.get_mut(world_entity) {
//             world_tf.translation.x = new_pos.x;
//             world_tf.translation.y = new_pos.y;
//         }
//     }
// }

pub fn combat_end_turn_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }
    if pending.entity.is_none() {
        return;
    }
    if input.just_pressed(KeyCode::Enter) || input.just_pressed(KeyCode::Space) {
        actions.write(PlayerActionEvent {
            action: PlayerAction::Wait,
        });
    }
}

/// Test hook: turn a nearby NPC into an enemy encounter.
pub fn transform_npc_to_enemy(
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    asset_server: Res<AssetServer>,
    player_q: Query<&Transform, With<Player>>,
    npc_q: Query<(Entity, &Transform, &WorldNpc)>,
) {
    if !input.just_pressed(KeyCode::KeyB) {
        return;
    }

    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    for (entity, tf, npc) in npc_q.iter() {
        if player_pos.distance(tf.translation.truncate()) <= 48.0 {
            commands.entity(entity).despawn();
            commands.spawn((
                Sprite {
                    image: asset_server.load("character.png"),
                    color: Color::srgb(0.85, 0.2, 0.2),
                    custom_size: Some(Vec2::new(32.0, 32.0)),
                    ..default()
                },
                Transform::from_translation(tf.translation),
                EnemyEncounter { id: npc.id },
            ));
            info!("NPC {} turned into enemy encounter", npc.id);
            break;
        }
    }
}

/// Test hook: log important combat-related state.
pub fn test_log_button(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    battle_state: Res<BattleState>,
    pending: Res<PendingPlayerAction>,
) {
    if input.just_pressed(KeyCode::KeyP) {
        info!(
            "TEST LOG: state={:?} battle_active={} participants={} pending={:?}",
            game_state.0,
            battle_state.active,
            battle_state.participants.len(),
            pending.entity
        );
    }
}

pub fn end_battle_on_death(
    mut commands: Commands,
    mut death_events: MessageReader<crate::combat_plugin::DeathEvent>,
    mut game_state: ResMut<GameState>,
    mut battle_state: ResMut<BattleState>,
    mut tm: ResMut<TurnManager>,
    mut turn_order: ResMut<TurnOrder>,
    participants_q: Query<&BattleSide, With<BattleParticipant>>,
    mut camera_q: Query<&mut Transform, With<MainCamera>>,
) {
    if !battle_state.active || game_state.0 != Game_State::Battle {
        return;
    }

    let mut battle_over = false;
    for ev in death_events.read() {
        if let Ok(side) = participants_q.get(ev.entity) {
            if matches!(side, BattleSide::Enemy) {
                battle_over = true;
                break;
            }
        }
    }

    if !battle_over {
        return;
    }

    for entity in battle_state.participants.drain(..) {
        commands.entity(entity).despawn();
    }
    tm.participants.clear();
    turn_order.queue.clear();
    battle_state.active = false;
    battle_state.enemy_id = None;
    game_state.0 = Game_State::Exploring;

    if let Ok(mut cam_tf) = camera_q.single_mut() {
        cam_tf.translation.z = 0.0;
    }
    info!("Battle ended");
}

pub fn end_battle(
    mut game_state: ResMut<GameState>,
    _turn_manager: Res<TurnManager>,
) {
    game_state.0 = Game_State::Exploring;
}
