//! Headless end-to-end run of the shikigami summon feature.
//!
//! This is not a GUI playthrough — the game is a windowed Bevy app and the
//! Paper Shikigami ability is skill-tree-locked, so driving it through the UI
//! is impractical to automate. Instead we boot a headless `App` with the REAL
//! `CombatPlugin` + `AiDecisionPlugin` (the same turn-order, AI, damage and
//! summon systems the game runs) and drive the actual pipeline:
//!
//!   SummonEvent  ->  resolve_summon_system spawns a Shikigami ally
//!                ->  register/compute turn-order systems include it
//!                ->  evaluate_behavior_tree_system drives its turns ("aggressive")
//!                ->  tick_summon_lifetime_system despawns it after 3 of its turns
//!
//! Asserts: it spawns Ally-side with SummonLifetime(3); it acts on its own
//! (deals damage to the enemy); it takes exactly 3 of its own turns; it is gone
//! afterwards.

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use bevy::MinimalPlugins;

use SeireiKuniBevy::ai_decision::AiDecisionPlugin;
use SeireiKuniBevy::battle::{
    resolve_summon_system, tick_summon_lifetime_system, BattleParticipant, BattleSide, BattleState,
    CombatMovePoints, SummonLifetime,
};
use SeireiKuniBevy::combat_ability::SummonKind;
use SeireiKuniBevy::combat_plugin::{
    AccumulatedSpeed, Abilities, CombatPlugin, CombatStats, DamageQueue, Experience,
    GrowthAttributes, Level, Reactions, StatModifiers, StatPool, SummonEvent, TurnEndEvent,
    TurnInProgress, TurnStartEvent,
};
use SeireiKuniBevy::core::{GameState, Game_State, Timestamp};
use SeireiKuniBevy::status_effects::StatusEffectsPlugin;

/// Counts how many turns a unit with `SummonLifetime` has ended — i.e. how many
/// turns the shikigami actually took on its own before expiring.
#[derive(Resource, Default)]
struct ShikiTurns(u32);

fn count_shiki_turns(
    mut reader: MessageReader<TurnEndEvent>,
    lifetimes: Query<(), With<SummonLifetime>>,
    mut count: ResMut<ShikiTurns>,
) {
    for ev in reader.read() {
        if lifetimes.get(ev.who).is_ok() {
            count.0 += 1;
        }
    }
}

/// Stand-in for the game's `setup_player_turns` + input loop: the inert
/// caster/enemy have no driver in this harness, so their turns would wedge
/// `turn_in_progress` forever. End any non-shikigami turn immediately so the
/// turn pipeline keeps advancing; the real BT AI drives the shikigami itself.
fn end_inert_turns(
    mut reader: MessageReader<TurnStartEvent>,
    summons: Query<(), With<SummonLifetime>>,
    mut turn_end: MessageWriter<TurnEndEvent>,
    mut in_progress: ResMut<TurnInProgress>,
) {
    for ev in reader.read() {
        if summons.get(ev.who).is_err() {
            turn_end.write(TurnEndEvent { who: ev.who });
            in_progress.0 = false;
        }
    }
}

/// Spawn a neutralized combatant: `PlayerControlled` so neither the BT AI nor
/// the legacy demo AI acts for it (it just exists as a turn-order member /
/// target), with no input system in the harness to drive it.
fn spawn_inert(app: &mut App, side: BattleSide, hp: i32, speed: i32) -> Entity {
    use SeireiKuniBevy::combat_plugin::PlayerControlled;
    let stats = CombatStats {
        health: <StatPool<i32>>::new(hp),
        morale: <StatPool<i32>>::new(60),
        action_points: <StatPool<i32>>::new(4),
        movement: <StatPool<i32>>::new(5),
        kiho: <StatPool<f32>>::new(0.0),
        onmyodo: <StatPool<f32>>::new(0.0),
        yokaijutsu: <StatPool<f32>>::new(0.0),
        kamishin: <StatPool<f32>>::new(0.0),
        lethality: <StatPool<i32>>::new(10),
        hit: <StatPool<i32>>::new(60),
        armor: <StatPool<i32>>::new(2),
        speed: <StatPool<i32>>::new(speed),
        evasion: <StatPool<i32>>::new(0), // never dodge, so shikigami damage lands
        mind: <StatPool<i32>>::new(4),
        health_per_rest_hour: 0,
        morale_per_rest_hour: 0,
        kiho_per_rest_hour: 0.0,
        onmyodo_per_rest_hour: 0.0,
        yokaijutsu_per_rest_hour: 0.0,
        kamishin_per_rest_hour: 0.0,
    };
    app.world_mut()
        .spawn((
            Name::new(format!("{side:?}Inert")),
            BattleParticipant,
            side,
            PlayerControlled,
            Transform::from_translation(Vec3::ZERO),
            stats,
            GrowthAttributes::default(),
            Abilities(vec![]),
            Experience(0),
            Level(1),
            AccumulatedSpeed(0),
            StatModifiers(Vec::new()),
            Reactions::default(),
            CombatMovePoints::default(),
        ))
        .id()
}

#[test]
fn shikigami_summons_acts_and_expires_after_three_turns() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        // `resolve_summon_system` reads `PlaceholderAssets` to give the spawned
        // unit its placeholder box (added during the 2D→3D port). MinimalPlugins
        // has no rendering, so stand the asset collections up headlessly and
        // build the resource the same way `world::setup` does.
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_systems(Startup, |mut commands: Commands,
                                mut meshes: ResMut<Assets<Mesh>>,
                                mut materials: ResMut<Assets<StandardMaterial>>| {
            commands.insert_resource(SeireiKuniBevy::render3d::PlaceholderAssets::build(
                &mut meshes,
                &mut materials,
            ));
        })
        .add_plugins(CombatPlugin)
        .add_plugins(AiDecisionPlugin)
        .add_plugins(StatusEffectsPlugin)
        // App-level resources the game inserts outside CombatPlugin:
        .insert_resource(GameState(Game_State::Battle))
        .insert_resource(BattleState {
            active: true,
            participants: Vec::new(),
            enemy_id: None,
        })
        .insert_resource(Timestamp(0))
        .insert_resource(DamageQueue::default())
        // `resolve_summon_system` also queries the collision `QuadTree` (to find
        // free ground for obstacle wards); an empty one is fine for a combatant
        // summon, which just spawns beside the caster.
        .init_resource::<SeireiKuniBevy::quadtree::QuadTree>()
        .insert_resource(ShikiTurns::default())
        // The two summon systems live in the game's app builder (lib.rs), not in
        // CombatPlugin — wire them here exactly as the game does.
        .add_systems(Update, resolve_summon_system)
        .add_systems(Update, tick_summon_lifetime_system)
        .add_systems(Update, count_shiki_turns)
        .add_systems(Update, end_inert_turns);

    // First tick runs Startup systems (loads the ability tree + AI profiles).
    app.update();

    // A caster (whose position the familiar spawns beside) and a punching-bag
    // enemy with enough HP to survive the shikigami's whole lifetime.
    let _caster = spawn_inert(&mut app, BattleSide::Ally, 100, 8);
    let enemy = spawn_inert(&mut app, BattleSide::Enemy, 400, 8);
    let enemy_hp_before = app
        .world()
        .get::<CombatStats>(enemy)
        .unwrap()
        .health
        .current;

    // Fire the summon exactly as `handle_ability` does for a `Summon` effect.
    app.world_mut()
        .resource_mut::<Messages<SummonEvent>>()
        .write(SummonEvent {
            summoner: _caster,
            kind: SummonKind::Shikigami,
            lifetime_turns: 3,
            // Combatant summons ignore this (they spawn beside the caster).
            target: None,
        });

    // Let resolve_summon_system spawn it.
    app.update();
    app.update();

    let alive_after_spawn = count_summons(&mut app);
    assert_eq!(
        alive_after_spawn, 1,
        "exactly one shikigami should spawn from the SummonEvent"
    );
    // Confirm it's an ally with a 3-turn lifetime.
    let (life, side) = summon_state(&mut app).expect("shikigami should exist");
    assert_eq!(side, BattleSide::Ally, "shikigami must be ally-side");
    assert_eq!(life, 3, "shikigami should start with a 3-turn lifetime");

    // Run the battle forward until the shikigami expires (or a safety cap).
    let mut ticks = 0;
    while count_summons(&mut app) > 0 && ticks < 400 {
        app.update();
        ticks += 1;
    }

    let turns = app.world().resource::<ShikiTurns>().0;
    let alive_end = count_summons(&mut app);
    let enemy_hp_after = app
        .world()
        .get::<CombatStats>(enemy)
        .unwrap()
        .health
        .current;

    eprintln!(
        "shikigami: turns_taken={turns}, alive_at_end={alive_end}, \
         enemy_hp {enemy_hp_before}->{enemy_hp_after}, ticks={ticks}"
    );

    assert_eq!(alive_end, 0, "shikigami should be despawned after expiry");
    assert_eq!(
        turns, 3,
        "shikigami should take exactly 3 of its own turns before expiring"
    );
    assert!(
        enemy_hp_after < enemy_hp_before,
        "shikigami should act on its own and damage the enemy ({enemy_hp_before} -> {enemy_hp_after})"
    );
}

fn count_summons(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<Entity, With<SummonLifetime>>();
    q.iter(app.world()).count()
}

fn summon_state(app: &mut App) -> Option<(u8, BattleSide)> {
    let mut q = app
        .world_mut()
        .query::<(&SummonLifetime, &BattleSide)>();
    q.iter(app.world())
        .next()
        .map(|(l, s)| (l.remaining_turns, *s))
}
