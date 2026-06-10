//! In-world / on-screen battle overlay.
//!
//! Purely presentational — it reads the combat model ([`CombatStats`],
//! [`TurnOrder`], [`DamageEvent`], …) and never writes to it. Everything is
//! spawned while a battle is active and torn down when it ends.
//!
//! Widgets:
//! * a docked **health roster** (left) listing every combatant's HP / morale,
//!   plus the active actor's move points,
//! * a **turn-order bar** (right) reading the already-calculated [`TurnOrder`]
//!   queue,
//! * floating **damage numbers**, a **battle log**,
//! * a **hover ring** over the combatant under the cursor, and
//! * a **move-destination ring** for a queued click-to-move.

use std::collections::VecDeque;

use bevy::prelude::*;

use crate::battle::{BattleParticipant, BattleSide, CombatMovePoints, CombatMoveTarget};
use crate::combat_plugin::{
    CombatStats, DamageEvent, DamageType, PendingPlayerAction, TurnOrder, TurnStartEvent,
};
use crate::core::{GameState, Game_State, MainCamera, Player};
use crate::status_effects::{StatusEffects, StatusKind};
use crate::ui_style::{font_size, palette, radius, spacing};

/// World +Z lift used when projecting a combatant for floating widgets.
const ANCHOR_WORLD_LIFT: f32 = 60.0;
/// Cursor pick radius (screen pixels) for hovering a combatant.
const HOVER_PICK_PX: f32 = 60.0;
/// Body-height (world +Z) at which a combatant is projected for hover testing,
/// so the pick lands on the model rather than its feet on the ground plane.
const HOVER_BODY_LIFT: f32 = 26.0;
/// Maximum status pips drawn on a roster row.
const MAX_PIPS: usize = 6;
/// Lifetime (seconds) of a floating damage number.
const DMG_LIFETIME: f32 = 1.1;
/// How far (px) a damage number rises over its life.
const DMG_RISE: f32 = 46.0;
/// Most recent battle-log lines kept / shown.
const LOG_LINES: usize = 6;
/// Roster bar / row width (px).
const ROSTER_WIDTH: f32 = 212.0;
/// Portrait square + vertical-bar height (px) on a roster card.
const PORTRAIT_SIZE: f32 = 46.0;

pub struct CombatOverlayPlugin;

impl Plugin for CombatOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BattleLog>()
            .init_resource::<HoveredCombatant>()
            .add_systems(
                Update,
                (
                    manage_overlay_lifetime,
                    track_hovered_combatant,
                    rebuild_roster.after(manage_overlay_lifetime),
                    rebuild_turn_bar.after(manage_overlay_lifetime),
                    spawn_damage_numbers,
                    record_battle_log,
                    render_battle_log,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    update_roster,
                    animate_damage_numbers,
                    sync_hover_ring,
                    sync_move_marker,
                    sync_move_ring,
                ),
            );
    }
}

/// Root marker for every overlay widget so teardown is a single query.
#[derive(Component)]
struct OverlayRoot;

/// The combatant currently under the cursor (for highlight + quick reference).
#[derive(Resource, Default)]
struct HoveredCombatant(Option<Entity>);

/// Despawn every overlay widget the moment we leave the Battle state.
fn manage_overlay_lifetime(
    mut commands: Commands,
    game_state: Res<GameState>,
    overlay_q: Query<Entity, With<OverlayRoot>>,
    mut log: ResMut<BattleLog>,
    mut hovered: ResMut<HoveredCombatant>,
) {
    if game_state.0 == Game_State::Battle {
        return;
    }
    if overlay_q.is_empty() {
        return;
    }
    for e in &overlay_q {
        commands.entity(e).despawn();
    }
    log.lines.clear();
    hovered.0 = None;
}

// ---------------------------------------------------------------------------
// Cursor hover tracking
// ---------------------------------------------------------------------------

/// Record which combatant (if any) the cursor is over. Each combatant is
/// projected to the screen at body height and compared against the cursor in
/// *pixel* space, so the hover lands on the model where it actually appears
/// (projecting the cursor to the ground plane drifts badly at this camera tilt).
fn track_hovered_combatant(
    game_state: Res<GameState>,
    mut hovered: ResMut<HoveredCombatant>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    windows: Query<&Window>,
    combatants: Query<(Entity, &Transform), With<BattleParticipant>>,
) {
    if game_state.0 != Game_State::Battle {
        if hovered.0.is_some() {
            hovered.0 = None;
        }
        return;
    }
    let pick = (|| {
        let (camera, cam_tf) = camera_q.iter().next()?;
        let window = windows.iter().next()?;
        let cursor = window.cursor_position()?;
        let mut best: Option<(Entity, f32)> = None;
        for (e, tf) in &combatants {
            let world = tf.translation + Vec3::new(0.0, 0.0, HOVER_BODY_LIFT);
            let Ok(screen) = camera.world_to_viewport(cam_tf, world) else {
                continue;
            };
            let d = screen.distance(cursor);
            if d <= HOVER_PICK_PX && best.map(|(_, b)| d < b).unwrap_or(true) {
                best = Some((e, d));
            }
        }
        best.map(|(e, _)| e)
    })();
    if hovered.0 != pick {
        hovered.0 = pick;
    }
}

// ---------------------------------------------------------------------------
// Health roster (docked, left)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct RosterPanel;

/// One roster card, with direct handles to the parts the per-frame update writes.
#[derive(Component)]
struct RosterRow {
    target: Entity,
    portrait_text: Entity,
    name_text: Entity,
    hp_fill: Entity,
    morale_fill: Entity,
    mp_text: Entity,
    pips: Vec<Entity>,
}

/// Rebuild the roster whenever the set of combatants changes (spawns / deaths).
fn rebuild_roster(
    mut commands: Commands,
    game_state: Res<GameState>,
    combatants: Query<(Entity, &BattleSide), (With<BattleParticipant>, With<CombatStats>)>,
    panel_q: Query<Entity, With<RosterPanel>>,
    mut signature: Local<Vec<Entity>>,
) {
    if game_state.0 != Game_State::Battle {
        if !signature.is_empty() {
            signature.clear();
        }
        return;
    }

    // Allies first, then enemies; stable order by entity for a steady layout.
    let mut current: Vec<(Entity, BattleSide)> =
        combatants.iter().map(|(e, s)| (e, *s)).collect();
    current.sort_by_key(|(e, s)| (matches!(s, BattleSide::Enemy), e.index()));
    let ids: Vec<Entity> = current.iter().map(|(e, _)| *e).collect();

    if ids == *signature && !panel_q.is_empty() {
        return;
    }
    *signature = ids;

    for e in &panel_q {
        commands.entity(e).despawn();
    }
    if current.is_empty() {
        return;
    }

    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(spacing::XL),
                left: Val::Px(spacing::LG),
                width: Val::Px(ROSTER_WIDTH),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(spacing::XS),
                padding: UiRect::all(Val::Px(spacing::SM)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(radius::MD)),
                ..default()
            },
            BackgroundColor(palette::BG_OVERLAY),
            BorderColor::all(palette::BORDER_SUBTLE),
            RosterPanel,
            OverlayRoot,
        ))
        .id();

    for (target, side) in current {
        spawn_roster_row(&mut commands, panel, target, side);
    }
}

fn spawn_roster_row(commands: &mut Commands, panel: Entity, target: Entity, side: BattleSide) {
    let accent = side_color(side);
    let mut portrait_text = Entity::PLACEHOLDER;
    let mut name_text = Entity::PLACEHOLDER;
    let mut hp_fill = Entity::PLACEHOLDER;
    let mut morale_fill = Entity::PLACEHOLDER;
    let mut mp_text = Entity::PLACEHOLDER;
    let mut pips: Vec<Entity> = Vec::with_capacity(MAX_PIPS);

    // Card: [portrait] [|HP] [|morale] [name / mp / status column].
    let row = commands
        .spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(spacing::XS),
                padding: UiRect::all(Val::Px(3.0)),
                border: UiRect::left(Val::Px(3.0)),
                border_radius: BorderRadius::all(Val::Px(radius::SM)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(accent),
            RosterRowTag { target },
        ))
        .id();

    commands.entity(row).with_children(|r| {
        // Portrait placeholder — a side-tinted square with the name's initial.
        r.spawn((
            Node {
                width: Val::Px(PORTRAIT_SIZE),
                height: Val::Px(PORTRAIT_SIZE),
                flex_shrink: 0.0,
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::SM)),
                ..default()
            },
            BackgroundColor(accent.with_alpha(0.18)),
            BorderColor::all(accent),
        ))
        .with_children(|p| {
            portrait_text = p
                .spawn((
                    Text::new("?"),
                    TextFont { font_size: font_size::SUBHEADING, ..default() },
                    TextColor(palette::TEXT_HEADING),
                ))
                .id();
        });

        // Vertical HP bar (fills bottom-up), on the side of the portrait.
        r.spawn(vbar_track(12.0)).with_children(|t| {
            hp_fill = t.spawn(vfill(palette::HEALTH_FULL)).id();
        });
        // Vertical morale bar (slimmer).
        r.spawn(vbar_track(7.0)).with_children(|t| {
            morale_fill = t.spawn(vfill(palette::MORALE)).id();
        });

        // Detail column: name, move points, status pips.
        r.spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|col| {
            name_text = col
                .spawn((
                    Text::new(""),
                    TextFont { font_size: font_size::SMALL, ..default() },
                    TextColor(accent),
                ))
                .id();
            mp_text = col
                .spawn((
                    Text::new(""),
                    TextFont { font_size: font_size::SMALL, ..default() },
                    TextColor(palette::ACCENT_SUCCESS),
                ))
                .id();
            col.spawn(Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|prow| {
                for _ in 0..MAX_PIPS {
                    pips.push(
                        prow.spawn((
                            Node {
                                width: Val::Px(7.0),
                                height: Val::Px(7.0),
                                display: Display::None,
                                border_radius: BorderRadius::all(Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(palette::TEXT_DIM),
                        ))
                        .id(),
                    );
                }
            });
        });
    });

    commands.entity(row).insert(RosterRow {
        target,
        portrait_text,
        name_text,
        hp_fill,
        morale_fill,
        mp_text,
        pips,
    });
    commands.entity(panel).add_child(row);
}

/// Lightweight tag carrying the row's target, queried during hover highlight.
#[derive(Component)]
struct RosterRowTag {
    target: Entity,
}

/// Refresh roster values each frame and highlight the hovered combatant's row.
#[allow(clippy::type_complexity)]
fn update_roster(
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    stats_q: Query<(&CombatStats, Option<&Name>, Option<&StatusEffects>)>,
    // The overworld player holds the authoritative move points the click-gate
    // spends from; show those (not the combat entity's possibly-stale mirror).
    world_mp_q: Query<&CombatMovePoints, With<Player>>,
    row_q: Query<&RosterRow>,
    mut bg_q: Query<&mut BackgroundColor>,
    mut node_q: Query<&mut Node>,
    mut text_q: Query<&mut Text>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }

    for row in &row_q {
        let Ok((stats, name, statuses)) = stats_q.get(row.target) else {
            continue;
        };

        // Name + portrait initial.
        let display_name = name.map(|n| short_name(n.as_str())).unwrap_or_else(|| "?".into());
        if let Ok(mut t) = text_q.get_mut(row.name_text) {
            if t.0 != display_name {
                t.0 = display_name.clone();
            }
        }
        if let Ok(mut t) = text_q.get_mut(row.portrait_text) {
            let initial = display_name.chars().next().unwrap_or('?').to_string();
            if t.0 != initial {
                t.0 = initial;
            }
        }

        // Vertical HP fill (height = fraction) + drain colour.
        let hp_frac = frac(stats.health.current, stats.health.base);
        if let Ok(mut n) = node_q.get_mut(row.hp_fill) {
            n.height = Val::Percent(hp_frac * 100.0);
        }
        if let Ok(mut c) = bg_q.get_mut(row.hp_fill) {
            c.0 = crate::ui_style::health_fill(hp_frac);
        }

        // Vertical morale fill.
        let mor_frac = frac(stats.morale.current, stats.morale.base);
        if let Ok(mut n) = node_q.get_mut(row.morale_fill) {
            n.height = Val::Percent(mor_frac * 100.0);
        }

        // Move points — only meaningful for the actor taking its turn.
        if let Ok(mut t) = text_q.get_mut(row.mp_text) {
            let desired = if pending.entity == Some(row.target) {
                world_mp_q
                    .iter()
                    .next()
                    .map(|mp| format!("move {:.0}", mp.remaining))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            if t.0 != desired {
                t.0 = desired;
            }
        }

        // Status pips.
        let list = statuses.map(|s| s.0.as_slice()).unwrap_or(&[]);
        for (i, &pip) in row.pips.iter().enumerate() {
            let entry = list.get(i);
            if let Ok(mut node) = node_q.get_mut(pip) {
                node.display = if entry.is_some() { Display::Flex } else { Display::None };
            }
            if let (Some(inst), Ok(mut color)) = (entry, bg_q.get_mut(pip)) {
                color.0 = if matches!(inst.kind, StatusKind::Buff(_)) {
                    palette::ACCENT_SUCCESS
                } else {
                    crate::ui_style::status_tier_color(inst.tier)
                };
            }
        }
        // (Row-background hover highlight is applied in `sync_hover_ring`.)
    }
}

/// A vertical bar track of the given px width and the standard portrait height.
/// Its [`vfill`] child is bottom-anchored, so the fill drains from the top down.
fn vbar_track(width: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width),
            height: Val::Px(PORTRAIT_SIZE),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexEnd,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(radius::SM)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(palette::BAR_TRACK),
        BorderColor::all(palette::BORDER_SUBTLE),
    )
}

fn vfill(color: Color) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
    )
}

fn side_color(side: BattleSide) -> Color {
    match side {
        BattleSide::Ally => palette::ALLY,
        BattleSide::Enemy => palette::ENEMY,
    }
}

fn frac(current: i32, base: i32) -> f32 {
    if base <= 0 {
        0.0
    } else {
        (current.max(0) as f32 / base as f32).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Hover ring + move-destination marker (in-world)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct HoverRing;

#[derive(Component)]
struct MoveMarker;

/// Draw a highlight ring over the hovered combatant (and tint its roster row).
#[allow(clippy::type_complexity)]
fn sync_hover_ring(
    mut commands: Commands,
    game_state: Res<GameState>,
    hovered: Res<HoveredCombatant>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    tf_q: Query<&Transform, With<BattleParticipant>>,
    mut ring_q: Query<(Entity, &mut Node), With<HoverRing>>,
    mut row_bg_q: Query<(&RosterRowTag, &mut BackgroundColor)>,
) {
    // Roster row highlight.
    for (tag, mut bg) in &mut row_bg_q {
        let on = hovered.0 == Some(tag.target);
        let want = if on { palette::BG_PANEL_RAISED } else { Color::NONE };
        if bg.0 != want {
            bg.0 = want;
        }
    }

    let active = game_state.0 == Game_State::Battle && hovered.0.is_some();
    if !active {
        for (e, _) in &ring_q {
            commands.entity(e).despawn();
        }
        return;
    }

    let target = hovered.0.unwrap();
    let Ok(tf) = tf_q.get(target) else { return };
    let Some((camera, cam_tf)) = camera_q.iter().next() else { return };
    let Ok(screen) = camera.world_to_viewport(cam_tf, tf.translation + Vec3::new(0.0, 0.0, 8.0))
    else {
        return;
    };
    let size = 54.0;
    let (left, top) = (screen.x - size * 0.5, screen.y - size * 0.5);

    if let Some((_, mut node)) = ring_q.iter_mut().next() {
        node.left = Val::Px(left);
        node.top = Val::Px(top);
    } else {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                width: Val::Px(size),
                height: Val::Px(size),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(size * 0.5)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(palette::ACCENT_WARNING),
            HoverRing,
            OverlayRoot,
        ));
    }
}

/// Flat ground ring showing how far the active player can still move this turn.
#[derive(Component)]
struct MoveRing;

/// Spawn / position a glowing ground ring at the player's reachable radius
/// during their turn (a movement indicator that reads even on dark ground).
#[allow(clippy::type_complexity)]
fn sync_move_ring(
    mut commands: Commands,
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    mut cached: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    player_q: Query<(&Transform, &CombatMovePoints), With<Player>>,
    mut ring_q: Query<(&mut Transform, &mut Visibility), (With<MoveRing>, Without<Player>)>,
) {
    let show = game_state.0 == Game_State::Battle && pending.entity.is_some();
    let player = player_q.iter().next();
    let radius = player.map(|(_, mp)| mp.remaining).unwrap_or(0.0);
    let active = show && radius > 1.0;

    // Hide the existing ring when not applicable.
    if !active {
        for (_, mut vis) in &mut ring_q {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
        return;
    }
    let Some((player_tf, _)) = player else { return };
    let pos = Vec3::new(player_tf.translation.x, player_tf.translation.y, 1.5);

    // Lazily build the unit-ring mesh + an unlit translucent material.
    let (mesh, mat) = cached
        .get_or_insert_with(|| {
            (
                meshes.add(Annulus::new(0.985, 1.0)),
                materials.add(StandardMaterial {
                    base_color: palette::ALLY.with_alpha(0.5),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                }),
            )
        })
        .clone();

    if let Some((mut tf, mut vis)) = ring_q.iter_mut().next() {
        tf.translation = pos;
        tf.scale = Vec3::new(radius, radius, 1.0);
        if *vis != Visibility::Visible {
            *vis = Visibility::Visible;
        }
    } else {
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(pos).with_scale(Vec3::new(radius, radius, 1.0)),
            MoveRing,
            OverlayRoot,
        ));
    }
}

/// Draw a ring at the active player's queued click-to-move destination.
fn sync_move_marker(
    mut commands: Commands,
    game_state: Res<GameState>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    move_q: Query<&CombatMoveTarget, With<Player>>,
    mut marker_q: Query<(Entity, &mut Node), With<MoveMarker>>,
) {
    let dest = (game_state.0 == Game_State::Battle)
        .then(|| move_q.iter().next())
        .flatten();

    let Some(dest) = dest else {
        for (e, _) in &marker_q {
            commands.entity(e).despawn();
        }
        return;
    };

    let Some((camera, cam_tf)) = camera_q.iter().next() else { return };
    let world = Vec3::new(dest.target.x, dest.target.y, 2.0);
    let Ok(screen) = camera.world_to_viewport(cam_tf, world) else { return };
    let size = 26.0;
    let (left, top) = (screen.x - size * 0.5, screen.y - size * 0.5);

    if let Some((_, mut node)) = marker_q.iter_mut().next() {
        node.left = Val::Px(left);
        node.top = Val::Px(top);
    } else {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                width: Val::Px(size),
                height: Val::Px(size),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(size * 0.5)),
                ..default()
            },
            BackgroundColor(palette::ALLY.with_alpha(0.18)),
            BorderColor::all(palette::ALLY),
            MoveMarker,
            OverlayRoot,
        ));
    }
}

// ---------------------------------------------------------------------------
// Floating damage numbers
// ---------------------------------------------------------------------------

#[derive(Component)]
struct FloatingNumber {
    age: f32,
    base_top: f32,
}

fn spawn_damage_numbers(
    mut commands: Commands,
    game_state: Res<GameState>,
    mut reader: MessageReader<DamageEvent>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    target_q: Query<&Transform>,
) {
    if game_state.0 != Game_State::Battle {
        reader.clear();
        return;
    }
    let Some((camera, cam_tf)) = camera_q.iter().next() else { return };

    for ev in reader.read() {
        if ev.amount <= 0 {
            continue;
        }
        let Ok(tf) = target_q.get(ev.target) else { continue };
        let world = tf.translation + Vec3::new(0.0, 0.0, ANCHOR_WORLD_LIFT + 18.0);
        let Ok(screen) = camera.world_to_viewport(cam_tf, world) else { continue };

        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(screen.x - 10.0),
                top: Val::Px(screen.y),
                ..default()
            },
            Text::new(format!("{}", ev.amount)),
            TextFont { font_size: font_size::BODY_LG, ..default() },
            TextColor(damage_color(ev.damage_type)),
            FloatingNumber { age: 0.0, base_top: screen.y },
            OverlayRoot,
        ));
    }
}

fn animate_damage_numbers(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut FloatingNumber, &mut Node, &mut TextColor)>,
) {
    let dt = time.delta_secs();
    for (e, mut fnum, mut node, mut color) in &mut q {
        fnum.age += dt;
        let t = (fnum.age / DMG_LIFETIME).clamp(0.0, 1.0);
        node.top = Val::Px(fnum.base_top - DMG_RISE * t);
        color.0 = color.0.with_alpha(1.0 - t);
        if fnum.age >= DMG_LIFETIME {
            commands.entity(e).despawn();
        }
    }
}

fn damage_color(kind: DamageType) -> Color {
    match kind {
        DamageType::Physical => palette::TEXT_HEADING,
        DamageType::Fire => Color::srgb(0.95, 0.55, 0.30),
        DamageType::Ice => Color::srgb(0.55, 0.80, 0.98),
        DamageType::True => palette::ACCENT_WARNING,
    }
}

// ---------------------------------------------------------------------------
// Turn-order bar (docked, right; reads the precalculated queue)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct TurnBar;

fn rebuild_turn_bar(
    mut commands: Commands,
    game_state: Res<GameState>,
    turn_order: Res<TurnOrder>,
    side_q: Query<&BattleSide>,
    name_q: Query<&Name>,
    bar_q: Query<Entity, With<TurnBar>>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }
    if !turn_order.is_changed() && !bar_q.is_empty() {
        return;
    }
    for e in &bar_q {
        commands.entity(e).despawn();
    }
    if turn_order.queue.is_empty() {
        return;
    }

    let root = (
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(spacing::XXL),
            right: Val::Px(spacing::LG),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(spacing::XS),
            padding: UiRect::all(Val::Px(spacing::SM)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(radius::MD)),
            ..default()
        },
        BackgroundColor(palette::BG_OVERLAY),
        BorderColor::all(palette::BORDER_SUBTLE),
        TurnBar,
        OverlayRoot,
    );

    commands.spawn(root).with_children(|col| {
        col.spawn((
            Text::new("Turn order"),
            TextFont { font_size: font_size::SMALL, ..default() },
            TextColor(palette::ACCENT_PRIMARY),
        ));
        for (i, &entity) in turn_order.queue.iter().take(10).enumerate() {
            let side = side_q.get(entity).copied().unwrap_or(BattleSide::Enemy);
            let accent = side_color(side);
            let current = i == 0;
            let label = name_q
                .get(entity)
                .map(|n| short_name(n.as_str()))
                .unwrap_or_else(|_| "?".to_string());

            // The current actor's chip is noticeably larger. Each carries a
            // small *circular* portrait (placeholder = initial), distinct from
            // the roster's larger square portraits, so the two don't feel
            // duplicated when real art is dropped in.
            let disc = if current { 38.0 } else { 26.0 };
            let initial = label.chars().next().unwrap_or('?').to_string();
            col.spawn((
                Node {
                    min_width: Val::Px(150.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(spacing::SM),
                    padding: UiRect::axes(
                        Val::Px(spacing::SM),
                        Val::Px(if current { spacing::SM } else { 3.0 }),
                    ),
                    border: UiRect::all(Val::Px(if current { 2.0 } else { 1.0 })),
                    border_radius: BorderRadius::all(Val::Px(radius::SM)),
                    ..default()
                },
                BackgroundColor(if current {
                    palette::BG_PANEL_RAISED
                } else {
                    palette::BG_PANEL
                }),
                BorderColor::all(if current { accent } else { palette::BORDER_SUBTLE }),
            ))
            .with_children(|chip| {
                // Circular portrait placeholder.
                chip.spawn((
                    Node {
                        width: Val::Px(disc),
                        height: Val::Px(disc),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(Val::Px(1.5)),
                        border_radius: BorderRadius::all(Val::Px(disc * 0.5)),
                        ..default()
                    },
                    BackgroundColor(accent.with_alpha(0.18)),
                    BorderColor::all(accent),
                ))
                .with_children(|p| {
                    p.spawn((
                        Text::new(initial),
                        TextFont {
                            font_size: if current { font_size::LABEL } else { font_size::SMALL },
                            ..default()
                        },
                        TextColor(palette::TEXT_HEADING),
                    ));
                });

                // Name (+ "turn" tag for the current actor).
                chip.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(1.0),
                    ..default()
                })
                .with_children(|txt| {
                    txt.spawn((
                        Text::new(label),
                        TextFont {
                            font_size: if current { font_size::SUBHEADING } else { font_size::SMALL },
                            ..default()
                        },
                        TextColor(if current { palette::TEXT_HEADING } else { accent }),
                    ));
                    if current {
                        txt.spawn((
                            Text::new("turn"),
                            TextFont { font_size: font_size::SMALL, ..default() },
                            TextColor(accent),
                        ));
                    }
                });
            });
        }
    });
}

/// A short, strip-friendly label from a combatant's full name.
fn short_name(full: &str) -> String {
    let inner = full
        .split_once('(')
        .map(|(_, rest)| rest.trim_end_matches(')'))
        .unwrap_or(full);
    let trimmed = inner.trim();
    if trimmed.chars().count() > 12 {
        trimmed.chars().take(11).collect::<String>() + "…"
    } else {
        trimmed.to_string()
    }
}

// ---------------------------------------------------------------------------
// Battle log (bottom-left)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct BattleLog {
    lines: VecDeque<String>,
    dirty: bool,
}

impl BattleLog {
    fn push(&mut self, line: String) {
        self.lines.push_back(line);
        while self.lines.len() > LOG_LINES {
            self.lines.pop_front();
        }
        self.dirty = true;
    }
}

#[derive(Component)]
struct BattleLogText;

fn record_battle_log(
    game_state: Res<GameState>,
    mut log: ResMut<BattleLog>,
    mut damage: MessageReader<DamageEvent>,
    mut turns: MessageReader<TurnStartEvent>,
    name_q: Query<&Name>,
) {
    if game_state.0 != Game_State::Battle {
        damage.clear();
        turns.clear();
        return;
    }
    let name = |e: Entity| {
        name_q
            .get(e)
            .map(|n| short_name(n.as_str()))
            .unwrap_or_else(|_| "?".to_string())
    };

    for ev in turns.read() {
        log.push(format!("▶ {} turn", name(ev.who)));
    }
    for ev in damage.read() {
        if ev.amount <= 0 {
            continue;
        }
        log.push(format!("{} → {} for {}", name(ev.attacker), name(ev.target), ev.amount));
    }
}

fn render_battle_log(
    mut commands: Commands,
    game_state: Res<GameState>,
    mut log: ResMut<BattleLog>,
    panel_q: Query<Entity, With<BattleLogText>>,
    mut text_q: Query<&mut Text, With<BattleLogText>>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }

    if panel_q.is_empty() {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(spacing::LG),
                    left: Val::Px(spacing::LG),
                    max_width: Val::Px(300.0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(spacing::SM)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(radius::MD)),
                    ..default()
                },
                BackgroundColor(palette::BG_OVERLAY),
                BorderColor::all(palette::BORDER_SUBTLE),
                OverlayRoot,
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new(""),
                    TextFont { font_size: font_size::SMALL, ..default() },
                    TextColor(palette::TEXT_SECONDARY),
                    BattleLogText,
                ));
            });
        log.dirty = true;
        return;
    }

    if !log.dirty {
        return;
    }
    log.dirty = false;
    let joined = log.lines.iter().cloned().collect::<Vec<_>>().join("\n");
    for mut text in &mut text_q {
        text.0 = joined.clone();
    }
}
