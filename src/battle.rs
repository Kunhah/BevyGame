use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::prelude::Messages;

use crate::combat_plugin::{
    Abilities, AccumulatedSpeed, Bound, CombatStats, DeathEvent, Experience, GrowthAttributes,
    Level, MagicDistribution, PendingPlayerAction, PlayerAction, PlayerActionEvent,
    PlayerControlled, ResurrectionStanding, StatModifiers, StatPool, TurnManager, TurnOrder,
    TurnStartEvent,
};
use crate::dialogue::{DialogueBoxTriggerEvent, DialogueCatalog, DialogueRuntime};
use crate::quests::HuntRegistry;
use crate::constants::{DEFAULT_ACTION_POINTS, GRID_HEIGHT, GRID_WIDTH, PLAYER_SPEED};
use crate::core::{GameState, Game_State, Global_Variables, MainCamera, Player, Position};
use crate::economy::MerchantNpc;
use crate::governance::{
    CastleAssaultStartedEvent, GovernorCombatant, GovernorNpc, SuccessorCombatant, SuccessorNpc,
};
use crate::combat_ability::MagicSchool;
use crate::pathfinding::is_walkable_move;
use crate::quadtree::QuadTree;
use crate::skill_tree::{LearnedSkills, MagicCostMultipliers, SkillPoints, SkillTreeAccess};

#[derive(Component, Clone, Copy, Debug)]
pub struct EnemyEncounter {
    pub id: u32,
}

/// Tags an `EnemyEncounter` as one of the GDD-flavored yokai species. When
/// present, the battle system spawns the encounter via
/// `spawn_yokai_combatant` (which wires the species' BT profile, abilities,
/// and stat block) rather than the generic `spawn_enemy_combat` lookup.
#[derive(Component, Clone, Copy, Debug)]
pub struct WorldYokai {
    pub kind: YokaiKind,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct WorldNpc {
    pub id: u32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct WorldAlly;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
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
    mut assault_starts: MessageWriter<CastleAssaultStartedEvent>,
    input: Res<ButtonInput<KeyCode>>,
    player_q: Query<(Entity, &Transform), With<Player>>,
    enemy_q: Query<
        (
            Entity,
            &Transform,
            &EnemyEncounter,
            Option<&GovernorNpc>,
            Option<&SuccessorNpc>,
            Option<&WorldYokai>,
        ),
    >,
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
    for (enemy_entity, enemy_tf, encounter, governor_opt, successor_opt, yokai_opt) in
        enemy_q.iter()
    {
        let enemy_pos = enemy_tf.translation.truncate();
        if player_pos.distance(enemy_pos) <= 32.0 {
            game_state.0 = Game_State::Battle;
            let governor_city_id = governor_opt.map(|g| g.city_id);
            let successor_target = successor_opt.map(|s| (s.city_id, s.successor_id));
            if let Some(city_id) = governor_city_id.or(successor_target.map(|(id, _)| id)) {
                assault_starts.write(CastleAssaultStartedEvent { city_id });
            }
            start_battle(
                &mut commands,
                &mut battle_state,
                &mut tm,
                &mut turn_order,
                encounter.id,
                governor_city_id,
                successor_target,
                yokai_opt.map(|y| y.kind),
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

#[allow(clippy::too_many_arguments)]
pub fn start_battle(
    commands: &mut Commands,
    battle_state: &mut BattleState,
    tm: &mut TurnManager,
    turn_order: &mut TurnOrder,
    enemy_id: u32,
    governor_city_id: Option<u16>,
    successor_target: Option<(u16, u32)>,
    yokai_kind: Option<YokaiKind>,
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
    // Yokai-tagged encounters use the species-specific spawn (wires
    // BehaviorTreeProfile, abilities, and the right stat block); other
    // encounters fall back to the generic enemy spawn.
    let enemy = match yokai_kind {
        Some(kind) => spawn_yokai_combatant(commands, kind, enemy_world_pos),
        None => spawn_enemy_combat(
            commands,
            enemy_id,
            enemy_world_pos,
            governor_city_id,
            successor_target,
        ),
    };
    participants.push(enemy);

    battle_state.participants = participants;
    tm.participants = battle_state.participants.clone();
    turn_order.queue.clear();

    commands.entity(enemy_world_entity).despawn();
    info!(
        "Battle started against enemy {} (yokai: {:?})",
        enemy_id,
        yokai_kind.map(|k| k.label())
    );
}

fn spawn_player_combat(commands: &mut Commands, world_entity: Entity, world_pos: Vec3) -> Entity {
    let mut e = commands.spawn_empty();
    e.insert(Name::new("PlayerCombat"));
    e.insert(BattleParticipant);
    e.insert(BattleSide::Ally);
    e.insert(PlayerControlled);
    e.insert(BattleWorldLink { world_entity });
    e.insert(Transform::from_translation(world_pos));
    e.insert(CombatStats {
        health: <StatPool<i32>>::new(120),
        morale: <StatPool<i32>>::new(90),
        action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
        movement: <StatPool<i32>>::new(5),
        kiho: <StatPool<f32>>::new(2.0),
        chiseijutsu: <StatPool<f32>>::new(2.0),
        yokaijutsu: <StatPool<f32>>::new(1.0),
        kamishin: <StatPool<f32>>::new(1.0),
        lethality: <StatPool<i32>>::new(14),
        hit: <StatPool<i32>>::new(80),
        armor: <StatPool<i32>>::new(10),
        speed: <StatPool<i32>>::new(10),
        evasion: <StatPool<i32>>::new(10),
        mind: <StatPool<i32>>::new(8),
        health_per_rest_hour: 2,
        morale_per_rest_hour: 5,
        kiho_per_rest_hour: 0.4,
        chiseijutsu_per_rest_hour: 0.4,
        yokaijutsu_per_rest_hour: 0.2,
        kamishin_per_rest_hour: 0.2,
    });
    e.insert(GrowthAttributes {
        vitality: 12,
        endurance: 10,
        spirit: 10,
        power: 12,
        control: 10,
        celerity: 10,
        reflex: 10,
        insight: 10,
        resolve: 12,
        // Generalist player: spirit=10 → 30 points, balanced split.
        magic_distribution: MagicDistribution {
            kiho: 8,
            chiseijutsu: 8,
            yokaijutsu: 7,
            kamishin: 7,
        },
    });
    e.insert(Abilities(vec![]));
    e.insert(Experience(0));
    e.insert(Level(1));
    e.insert(AccumulatedSpeed(0));
    e.insert(StatModifiers(Vec::new()));
    e.insert(CombatMovePoints::default());
    e.insert(SkillPoints::default());
    e.insert(LearnedSkills::default());
    e.insert(MagicCostMultipliers::default());
    // Generic player: bound to the contract, generalist across every magic
    // school, no protagonist-specific class tree.
    e.insert(
        SkillTreeAccess::new()
            .with_universal()
            .with_magic([
                MagicSchool::Kiho,
                MagicSchool::Chiseijutsu,
                MagicSchool::Yokaijutsu,
                MagicSchool::Kamishin,
            ]),
    );
    e.id()
}

fn spawn_enemy_combat(
    commands: &mut Commands,
    enemy_id: u32,
    world_pos: Vec3,
    governor_city_id: Option<u16>,
    successor_target: Option<(u16, u32)>,
) -> Entity {
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
    e.insert(CombatStats {
        health: <StatPool<i32>>::new(hp),
        morale: <StatPool<i32>>::new(70),
        action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
        movement: <StatPool<i32>>::new(4),
        kiho: <StatPool<f32>>::new(1.0),
        chiseijutsu: <StatPool<f32>>::new(0.5),
        yokaijutsu: <StatPool<f32>>::new(0.5),
        kamishin: <StatPool<f32>>::new(0.0),
        lethality: <StatPool<i32>>::new(lethality),
        hit: <StatPool<i32>>::new(hit),
        armor: <StatPool<i32>>::new(armor),
        speed: <StatPool<i32>>::new(agility),
        evasion: <StatPool<i32>>::new(agility),
        mind: <StatPool<i32>>::new(6),
        health_per_rest_hour: 1,
        morale_per_rest_hour: 3,
        kiho_per_rest_hour: 0.25,
        chiseijutsu_per_rest_hour: 0.1,
        yokaijutsu_per_rest_hour: 0.1,
        kamishin_per_rest_hour: 0.0,
    });
    e.insert(GrowthAttributes {
        vitality: 8,
        endurance: 8,
        spirit: 6,
        power: 8,
        control: 8,
        celerity: 8,
        reflex: 8,
        insight: 6,
        resolve: 6,
        // Generic enemy: spirit=6 → 18 points, yokai-leaning.
        magic_distribution: MagicDistribution {
            kiho: 4,
            chiseijutsu: 4,
            yokaijutsu: 8,
            kamishin: 2,
        },
    });
    e.insert(Abilities(vec![]));
    e.insert(Experience(0));
    e.insert(Level(1));
    e.insert(AccumulatedSpeed(0));
    e.insert(StatModifiers(Vec::new()));
    e.insert(CombatMovePoints::default());
    if let Some(city_id) = governor_city_id {
        e.insert(GovernorCombatant { city_id });
    }
    if let Some((city_id, successor_id)) = successor_target {
        e.insert(SuccessorCombatant {
            city_id,
            successor_id,
        });
    }
    e.id()
}

/// The yokai species that the GDD-flavored content authors. Each variant
/// carries the stat block, the ability ids it knows, and the BT profile name
/// so a single helper can spawn it as a battle participant.
#[derive(Debug, Clone, Copy)]
pub enum YokaiKind {
    /// Onibi — will-o'-wisp. Fast, fragile, fire-leaning.
    Onibi,
    /// Kappa — river demon. Balanced melee with a slow effect.
    Kappa,
    /// Kasha — cat-cart yokai. Mental caster with an AOE Final cry.
    Kasha,
}

impl YokaiKind {
    pub fn label(self) -> &'static str {
        match self {
            YokaiKind::Onibi => "Onibi",
            YokaiKind::Kappa => "Kappa",
            YokaiKind::Kasha => "Kasha",
        }
    }

    /// BT profile name (matches a key in `assets/data/decision_tree.ron`).
    fn behavior_profile(self) -> &'static str {
        match self {
            YokaiKind::Onibi => "yokai_onibi",
            YokaiKind::Kappa => "yokai_kappa",
            YokaiKind::Kasha => "yokai_kasha",
        }
    }

    /// Ability ids granted to this yokai (match `AbilitiesExample.ron`).
    fn abilities(self) -> Vec<u16> {
        match self {
            YokaiKind::Onibi => vec![3840],
            YokaiKind::Kappa => vec![3841],
            YokaiKind::Kasha => vec![3842],
        }
    }
}

/// Spawn a yokai as a battle participant. Wires `CombatStats`, `Reactions`
/// (empty for now — author-time hookable), and the BT profile string so
/// `crate::ai_decision::evaluate_behavior_tree_system` drives its turns.
pub fn spawn_yokai_combatant(
    commands: &mut Commands,
    kind: YokaiKind,
    world_pos: Vec3,
) -> Entity {
    use crate::combat_plugin::Reactions;

    // Stat block per species. Onibi is the fragile striker; Kappa is sturdy
    // melee; Kasha is squishy but high-mind.
    let (hp, lethality, hit, armor, speed, mind, yokai_pool) = match kind {
        YokaiKind::Onibi => (35, 14, 70, 4, 18, 12, 6.0_f32),
        YokaiKind::Kappa => (90, 16, 65, 12, 9, 6, 3.0_f32),
        YokaiKind::Kasha => (55, 8, 60, 6, 12, 18, 8.0_f32),
    };

    let mut e = commands.spawn_empty();
    e.insert(Name::new(format!("Yokai({})", kind.label())));
    e.insert(BattleParticipant);
    e.insert(BattleSide::Enemy);
    e.insert(Transform::from_translation(world_pos));
    e.insert(CombatStats {
        health: <StatPool<i32>>::new(hp),
        morale: <StatPool<i32>>::new(60),
        action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
        movement: <StatPool<i32>>::new(5),
        kiho: <StatPool<f32>>::new(0.0),
        chiseijutsu: <StatPool<f32>>::new(if matches!(kind, YokaiKind::Kappa) { 4.0 } else { 0.0 }),
        yokaijutsu: <StatPool<f32>>::new(yokai_pool),
        kamishin: <StatPool<f32>>::new(0.0),
        lethality: <StatPool<i32>>::new(lethality),
        hit: <StatPool<i32>>::new(hit),
        armor: <StatPool<i32>>::new(armor),
        speed: <StatPool<i32>>::new(speed),
        evasion: <StatPool<i32>>::new(speed),
        mind: <StatPool<i32>>::new(mind),
        health_per_rest_hour: 0,
        morale_per_rest_hour: 0,
        kiho_per_rest_hour: 0.0,
        chiseijutsu_per_rest_hour: 0.0,
        yokaijutsu_per_rest_hour: 0.0,
        kamishin_per_rest_hour: 0.0,
    });
    e.insert(GrowthAttributes::default());
    e.insert(Abilities(kind.abilities()));
    e.insert(Experience(0));
    e.insert(Level(1));
    e.insert(AccumulatedSpeed(0));
    e.insert(StatModifiers(Vec::new()));
    e.insert(Reactions::default());
    e.insert(CombatMovePoints::default());
    e.insert(crate::ai_decision::BehaviorTreeProfile(
        kind.behavior_profile().to_string(),
    ));
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
    e.insert(CombatStats {
        health: <StatPool<i32>>::new(100),
        morale: <StatPool<i32>>::new(85),
        action_points: <StatPool<i32>>::new(DEFAULT_ACTION_POINTS),
        movement: <StatPool<i32>>::new(5),
        kiho: <StatPool<f32>>::new(1.0),
        chiseijutsu: <StatPool<f32>>::new(1.5),
        yokaijutsu: <StatPool<f32>>::new(1.0),
        kamishin: <StatPool<f32>>::new(0.5),
        lethality: <StatPool<i32>>::new(12),
        hit: <StatPool<i32>>::new(75),
        armor: <StatPool<i32>>::new(8),
        speed: <StatPool<i32>>::new(9),
        evasion: <StatPool<i32>>::new(9),
        mind: <StatPool<i32>>::new(8),
        health_per_rest_hour: 2,
        morale_per_rest_hour: 4,
        kiho_per_rest_hour: 0.25,
        chiseijutsu_per_rest_hour: 0.4,
        yokaijutsu_per_rest_hour: 0.25,
        kamishin_per_rest_hour: 0.15,
    });
    e.insert(GrowthAttributes {
        vitality: 10,
        endurance: 9,
        spirit: 8,
        power: 10,
        control: 9,
        celerity: 9,
        reflex: 9,
        insight: 8,
        resolve: 10,
        // Ally combatant: spirit=8 → 24 points, nature-leaning support.
        magic_distribution: MagicDistribution {
            kiho: 6,
            chiseijutsu: 10,
            yokaijutsu: 4,
            kamishin: 4,
        },
    });
    e.insert(Abilities(vec![]));
    e.insert(Experience(0));
    e.insert(Level(1));
    e.insert(AccumulatedSpeed(0));
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
            let movement = stats.movement.current.max(0) as f32;
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
        // Camera following is owned by `camera_follow_player` (iso offset); the
        // old 2D snap-to-player here fought it and caused jitter.
        let _ = (new_x_out, new_y_out, camera_locked);
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
    npc_q: Query<(Entity, &Transform, &WorldNpc), Without<MerchantNpc>>,
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
                crate::render3d::PlaceholderVisual::character(Color::srgb(0.85, 0.2, 0.2)),
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

// ---------------------------------------------------------------------------
// Hunt trigger pipeline
// ---------------------------------------------------------------------------

/// Marks a world enemy entity as the target of a specific hunt. The
/// proximity-trigger system uses `hunt_id` to look up the hunt's
/// `pre_battle_scene` + conditions in [`HuntRegistry`].
#[derive(Component, Debug, Clone, Copy)]
pub struct HuntTarget {
    pub hunt_id: u32,
}

/// Tagged on a hunt target after its pre-battle cutscene has played to
/// prevent re-triggering on the same approach.
#[derive(Component)]
pub struct HuntCutscenePlayed;

/// Battle queued to start after the pre-battle cutscene closes.
#[derive(Resource, Default)]
pub struct PendingHuntBattle {
    pub hunt_target: Option<Entity>,
}

const HUNT_PROXIMITY_RADIUS: f32 = 96.0;

/// When the player walks within `HUNT_PROXIMITY_RADIUS` of a `HuntTarget`,
/// look up the hunt's `pre_battle_scene`. If set, play the cutscene and
/// queue the battle for after the cutscene closes; otherwise battle starts
/// immediately on the next frame via `start_pending_hunt_battle`.
pub fn hunt_proximity_trigger(
    mut commands: Commands,
    catalog: Res<DialogueCatalog>,
    mut runtime: ResMut<DialogueRuntime>,
    mut events_dialogue_box: ResMut<Messages<DialogueBoxTriggerEvent>>,
    mut game_state: ResMut<GameState>,
    mut pending: ResMut<PendingHuntBattle>,
    hunts: Res<HuntRegistry>,
    player_q: Query<&Transform, (With<Player>, Without<HuntTarget>)>,
    target_q: Query<(Entity, &Transform, &HuntTarget), Without<HuntCutscenePlayed>>,
) {
    if !matches!(game_state.0, Game_State::Exploring) {
        return;
    }
    if runtime.active || pending.hunt_target.is_some() {
        return;
    }
    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();
    for (entity, tf, target) in target_q.iter() {
        if player_pos.distance(tf.translation.truncate()) > HUNT_PROXIMITY_RADIUS {
            continue;
        }
        commands.entity(entity).insert(HuntCutscenePlayed);
        pending.hunt_target = Some(entity);
        if let Some(hunt) = hunts.0.get(&target.hunt_id) {
            if let Some(scene) = hunt.pre_battle_scene.as_ref() {
                if catalog.scenes.contains_key(scene)
                    && runtime.start(scene.clone(), &catalog)
                {
                    events_dialogue_box.write(DialogueBoxTriggerEvent);
                    game_state.0 = Game_State::Interacting;
                    info!(
                        "hunt_proximity_trigger: scene '{scene}' for hunt {}",
                        target.hunt_id
                    );
                    return;
                }
                warn!(
                    "hunt_proximity_trigger: scene '{scene}' missing for hunt {}",
                    target.hunt_id
                );
            }
        } else {
            warn!(
                "hunt_proximity_trigger: HuntTarget hunt_id {} not in HuntRegistry",
                target.hunt_id
            );
        }
        info!(
            "hunt_proximity_trigger: hunt {} battle queued (no cutscene)",
            target.hunt_id
        );
        return;
    }
}

/// When the queued cutscene closes (or there was no cutscene), kick the
/// real battle against the hunt target.
pub fn start_pending_hunt_battle(
    mut commands: Commands,
    mut pending: ResMut<PendingHuntBattle>,
    runtime: Res<DialogueRuntime>,
    mut battle_state: ResMut<BattleState>,
    mut tm: ResMut<TurnManager>,
    mut turn_order: ResMut<TurnOrder>,
    mut game_state: ResMut<GameState>,
    player_q: Query<(Entity, &Transform), With<Player>>,
    hunt_q: Query<
        (&Transform, &EnemyEncounter, Option<&WorldYokai>),
        With<HuntTarget>,
    >,
) {
    let Some(target) = pending.hunt_target else {
        return;
    };
    if runtime.active || battle_state.active {
        return;
    }
    let Ok((player_entity, player_tf)) = player_q.single() else {
        return;
    };
    let Ok((hunt_tf, encounter, yokai)) = hunt_q.get(target) else {
        // World entity gone (despawned by some other path). Drop the queue.
        pending.hunt_target = None;
        return;
    };
    game_state.0 = Game_State::Battle;
    start_battle(
        &mut commands,
        &mut battle_state,
        &mut tm,
        &mut turn_order,
        encounter.id,
        None,
        None,
        yokai.map(|y| y.kind),
        target,
        player_entity,
        player_tf.translation,
        hunt_tf.translation,
        Vec::new(),
    );
    pending.hunt_target = None;
}

/// Copy `Bound` + `ResurrectionStanding` from the world entity onto any
/// freshly-spawned battle participant for the player. Without this the death
/// pipeline would refuse to enqueue a resurrection (it queries those
/// components on the dying entity), and player loss would dead-end.
pub fn sync_player_combat_bound(
    mut commands: Commands,
    new_participants: Query<
        (Entity, &BattleWorldLink),
        (Added<PlayerControlled>, With<BattleParticipant>),
    >,
    world_q: Query<(&Bound, &ResurrectionStanding), With<Player>>,
) {
    for (entity, link) in new_participants.iter() {
        if let Ok((_, standing)) = world_q.get(link.world_entity) {
            commands
                .entity(entity)
                .insert((Bound, standing.clone()));
        }
    }
}

/// When a player-controlled battle participant dies, end the battle
/// ourselves (the shipped `end_battle_on_death` only ends on Enemy death)
/// and re-emit `DeathEvent` on the world player so the existing
/// resurrection pipeline (which queries `Bound` / `ResurrectionStanding` on
/// the world entity) fires.
pub fn bridge_player_death_to_world(
    // Reads `DeathEvent` and re-emits one targeting the world entity. Bevy 0.18
    // forbids `Res<Messages<T>>` + `ResMut<Messages<T>>` in one system, so reader
    // and writer share a `ParamSet`: collect the bridged event while reading,
    // then write it once the read borrow is released.
    mut deaths: ParamSet<(MessageReader<DeathEvent>, MessageWriter<DeathEvent>)>,
    participants_q: Query<
        (&BattleSide, &BattleWorldLink),
        (With<BattleParticipant>, With<PlayerControlled>),
    >,
    mut commands: Commands,
    mut battle_state: ResMut<BattleState>,
    mut tm: ResMut<TurnManager>,
    mut turn_order: ResMut<TurnOrder>,
    mut game_state: ResMut<GameState>,
) {
    let mut bridged: Option<DeathEvent> = None;
    for ev in deaths.p0().read() {
        let Ok((side, link)) = participants_q.get(ev.entity) else {
            continue;
        };
        if !matches!(side, BattleSide::Ally) {
            continue;
        }
        for entity in battle_state.participants.drain(..) {
            commands.entity(entity).despawn();
        }
        tm.participants.clear();
        turn_order.queue.clear();
        battle_state.active = false;
        battle_state.enemy_id = None;
        game_state.0 = Game_State::Exploring;

        bridged = Some(DeathEvent {
            entity: link.world_entity,
            killer: ev.killer,
        });
        break;
    }

    if let Some(ev) = bridged {
        deaths.p1().write(ev);
        info!("bridge_player_death_to_world: player died in battle — bridged to world");
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

    info!("Battle ended");
}

pub fn end_battle(
    mut game_state: ResMut<GameState>,
    _turn_manager: Res<TurnManager>,
) {
    game_state.0 = Game_State::Exploring;
}
