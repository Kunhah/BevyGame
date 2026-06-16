//! Named overland areas, the world-map travel UI, and time-synchronised travel.
//!
//! The world is one continuous 64×64 tilemap (see [`crate::map`]); only the
//! tiles near the player are ever streamed in (`update_active_tile_background`).
//! "Areas" are *named regions* carved out of that single map: each owns one
//! 8×8 block of tiles (its `location_id`), a dominant terrain type for flavour,
//! an anchor tile the traveller arrives on, and a position on the world-map UI.
//!
//! Travel is overland and *time-synchronised*: picking a destination on the
//! world map computes the shortest route over the area graph (summing each
//! edge's in-game hours), then enters [`Game_State::Traveling`], a brief
//! animated state that advances the [`Timestamp`] clock progressively before
//! dropping the party on the destination's anchor tile. Because the in-game
//! clock is the same one the HUD, magic regen, and status-effect expiries read,
//! a long journey actually costs the player hours of game time.
//!
//! Three pieces, matching the design:
//!   1. [`AreaCatalog`] — the generated areas (RON-driven, hardcoded fallback).
//!   2. [`WorldMapOpen`] node-map UI — a graphical map of area nodes + edges.
//!   3. [`Game_State::Traveling`] — clock-ticking animated travel.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::constants::{TIMESTAMP_SECONDS_PER_TICK, TIMESTAMP_TICKS_PER_HOUR};
use crate::core::{GameState, Game_State, MainCamera, Player, PlayerMapPosition, Timestamp};
use crate::core::Position;
use crate::map::{
    shortest_time_path_and_cost, tile_center_world, travel_ticks_for_cost, AreaChanged,
    CurrentArea, MapTiles, TerrainSlowEffectIndex,
};
use crate::render3d::iso_camera_offset;
use crate::ui_style::{font_size, palette, radius, spacing};

const AREAS_DATA_PATH: &str = "assets/data/areas.ron";

/// Size of the 8×8 tile block each area owns. Mirrors `region_size` in
/// `crate::map::generate_map_tiles`, where `location_id = bx + by * 8`.
const REGION_SIZE: i32 = 8;
/// The world is 8 blocks wide × 8 blocks tall (64×64 tiles / `REGION_SIZE`), so
/// `location_id = bx + by * BLOCKS_PER_ROW` spans a full 8×8 grid of regions.
const BLOCKS_PER_ROW: u16 = 8;
const WORLD_BLOCK_COLS: u16 = 8;
const WORLD_BLOCK_ROWS: u16 = 8;

// --- World-map UI layout (pixels) ------------------------------------------
// `MAP_CANVAS_*` is the on-screen size of the grid and also the coordinate space
// `canvas_px` reports for keyboard-nav direction scoring.
const MAP_CANVAS_W: f32 = 980.0;
const MAP_CANVAS_H: f32 = 560.0;
const CELL_GAP: f32 = 3.0;
const INFO_PANEL_W: f32 = 260.0;
const TRAVEL_BAR_W: f32 = 420.0;
const TRAVEL_BAR_H: f32 = 18.0;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A directed edge in the area graph: travelling to `to` costs `hours` of
/// in-game time. Edges are made bidirectional when the adjacency map is built.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaLink {
    pub to: u16,
    pub hours: u32,
}

/// One named overland area. `id` doubles as the `location_id` stamped onto the
/// area's 8×8 tile block, so [`CurrentArea`] lines up with the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaDef {
    pub id: u16,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Tile the traveller lands on when arriving here.
    pub anchor: Position,
    /// Dominant terrain `type_id` stamped across the block (0 road, 1 plains,
    /// 2 forest, 3 mountains) — purely for overworld flavour.
    #[serde(default)]
    pub terrain: u8,
    /// Position on the world-map UI canvas, normalised to `0.0..=1.0`.
    pub ui_x: f32,
    pub ui_y: f32,
    #[serde(default)]
    pub connections: Vec<AreaLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AreasDataFile {
    #[serde(default)]
    areas: Vec<AreaDef>,
}

/// All named areas plus a precomputed undirected adjacency map for routing.
#[derive(Resource, Debug, Clone)]
pub struct AreaCatalog {
    pub areas: Vec<AreaDef>,
    by_id: HashMap<u16, usize>,
    adjacency: HashMap<u16, Vec<AreaLink>>,
}

impl Default for AreaCatalog {
    fn default() -> Self {
        let areas = load_areas_data_file()
            .map(|f| f.areas)
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| {
                info!("Using built-in default areas (no usable {})", AREAS_DATA_PATH);
                seed_default_areas()
            });
        // Expand the authored areas to cover the entire 8×8 block grid so the
        // whole world is one continuous, travelable map rather than a corner.
        Self::from_areas(fill_world_grid(areas))
    }
}

impl AreaCatalog {
    pub fn from_areas(areas: Vec<AreaDef>) -> Self {
        let mut by_id = HashMap::new();
        for (i, area) in areas.iter().enumerate() {
            by_id.insert(area.id, i);
        }

        // Build an undirected adjacency map, keeping the cheapest hours for any
        // duplicated edge so a route never costs more than the data implies.
        let mut adjacency: HashMap<u16, Vec<AreaLink>> = HashMap::new();
        let push_edge = |adj: &mut HashMap<u16, Vec<AreaLink>>, from: u16, to: u16, hours: u32| {
            let entry = adj.entry(from).or_default();
            if let Some(existing) = entry.iter_mut().find(|l| l.to == to) {
                existing.hours = existing.hours.min(hours);
            } else {
                entry.push(AreaLink { to, hours });
            }
        };
        for area in &areas {
            for link in &area.connections {
                push_edge(&mut adjacency, area.id, link.to, link.hours.max(1));
                push_edge(&mut adjacency, link.to, area.id, link.hours.max(1));
            }
        }

        Self {
            areas,
            by_id,
            adjacency,
        }
    }

    pub fn get(&self, id: u16) -> Option<&AreaDef> {
        self.by_id.get(&id).and_then(|&i| self.areas.get(i))
    }

    pub fn neighbors(&self, id: u16) -> &[AreaLink] {
        self.adjacency.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn name_of(&self, id: u16) -> String {
        self.get(id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("Region {id}"))
    }
}

// ---------------------------------------------------------------------------
// Full-grid expansion: every block becomes a named, travelable region
// ---------------------------------------------------------------------------

/// Evocative prefixes for procedurally-named filler regions.
const NAME_PREFIX: [&str; 16] = [
    "North", "Hidden", "Old", "Lonely", "Misty", "Jade", "Crimson", "Silver",
    "Whispering", "Broken", "Golden", "Quiet", "Frost", "Ember", "Shadow", "Twin",
];

/// Deterministic terrain for an auto-generated block, giving the world variety
/// without hand-authoring all 64 regions. Never returns the impassable type.
fn generated_terrain(bx: u16, by: u16) -> u8 {
    let h = (bx.wrapping_mul(73) ^ by.wrapping_mul(151)).wrapping_add(bx.wrapping_mul(by));
    match h % 7 {
        0 => 0,        // road
        1 | 2 => 1,    // plains
        3 | 4 => 2,    // forest
        _ => 3,        // mountains
    }
}

fn generated_name(id: u16, terrain: u8) -> String {
    let prefix = NAME_PREFIX[(id as usize * 7 + 3) % NAME_PREFIX.len()];
    let suffix = match terrain {
        0 => "Crossing",
        1 => "Fields",
        2 => "Woods",
        3 => "Heights",
        _ => "Wilds",
    };
    format!("{prefix} {suffix}")
}

fn generated_description(terrain: u8) -> String {
    match terrain {
        0 => "A dusty waystation where old roads meet.",
        1 => "Open rice-fields and grazing land under a wide sky.",
        2 => "Dim cedar woods loud with cicada song.",
        3 => "Wind-bitten ridgelines and bare standing stone.",
        _ => "Untamed borderland the maps barely name.",
    }
    .to_string()
}

/// Per-terrain leg cost; mountains are slow, roads quick. Summed across the two
/// regions a leg connects so terrain shapes route times.
fn terrain_leg_cost(terrain: u8) -> u32 {
    match terrain {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        _ => 3,
    }
}

fn travel_hours(a: u8, b: u8) -> u32 {
    (terrain_leg_cost(a) + terrain_leg_cost(b)).max(2)
}

fn block_anchor(bx: u16, by: u16) -> Position {
    // +4 lands mid-block, always inside the one-tile impassable border ring.
    Position {
        x: bx as i32 * REGION_SIZE + 4,
        y: by as i32 * REGION_SIZE + 4,
    }
}

fn generated_area(id: u16, bx: u16, by: u16) -> AreaDef {
    let terrain = generated_terrain(bx, by);
    AreaDef {
        id,
        name: generated_name(id, terrain),
        description: generated_description(terrain),
        anchor: block_anchor(bx, by),
        terrain,
        ui_x: bx as f32 / (WORLD_BLOCK_COLS.max(2) - 1) as f32,
        ui_y: by as f32 / (WORLD_BLOCK_ROWS.max(2) - 1) as f32,
        connections: vec![],
    }
}

fn add_link(area: &mut AreaDef, to: u16, hours: u32) {
    if !area.connections.iter().any(|l| l.to == to) {
        area.connections.push(AreaLink { to, hours });
    }
}

/// Ensure every block of the 8×8 world has a region, keeping authored areas and
/// generating filler for the rest, then link each region to its right and down
/// neighbour so the whole grid is reachable. Authored cross-links survive
/// because [`AreaCatalog::from_areas`] dedups edges and keeps the cheapest.
fn fill_world_grid(authored: Vec<AreaDef>) -> Vec<AreaDef> {
    let mut by_id: HashMap<u16, AreaDef> =
        authored.into_iter().map(|a| (a.id, a)).collect();

    for by in 0..WORLD_BLOCK_ROWS {
        for bx in 0..WORLD_BLOCK_COLS {
            let id = bx + by * BLOCKS_PER_ROW;
            by_id
                .entry(id)
                .or_insert_with(|| generated_area(id, bx, by));
        }
    }

    let terrain_of: HashMap<u16, u8> =
        by_id.iter().map(|(id, a)| (*id, a.terrain)).collect();

    let mut areas = Vec::with_capacity(by_id.len());
    for by in 0..WORLD_BLOCK_ROWS {
        for bx in 0..WORLD_BLOCK_COLS {
            let id = bx + by * BLOCKS_PER_ROW;
            let Some(mut area) = by_id.remove(&id) else {
                continue;
            };
            let here = area.terrain;
            if bx + 1 < WORLD_BLOCK_COLS {
                let to = id + 1;
                add_link(&mut area, to, travel_hours(here, terrain_of[&to]));
            }
            if by + 1 < WORLD_BLOCK_ROWS {
                let to = id + BLOCKS_PER_ROW;
                add_link(&mut area, to, travel_hours(here, terrain_of[&to]));
            }
            areas.push(area);
        }
    }
    areas
}

/// Flat fill colour for a region cell, by terrain `type_id`.
fn terrain_color(terrain: u8) -> Color {
    match terrain {
        0 => Color::srgb(0.40, 0.36, 0.28), // road — dusty tan
        1 => Color::srgb(0.28, 0.44, 0.24), // plains — green
        2 => Color::srgb(0.16, 0.32, 0.20), // forest — deep green
        3 => Color::srgb(0.38, 0.38, 0.44), // mountains — slate
        _ => Color::srgb(0.34, 0.28, 0.40), // wilds — muted violet
    }
}

/// Mix a colour toward white by `t` for hover/focus/here highlight states.
fn lighten(color: Color, t: f32) -> Color {
    let s = color.to_srgba();
    Color::srgb(
        s.red + (1.0 - s.red) * t,
        s.green + (1.0 - s.green) * t,
        s.blue + (1.0 - s.blue) * t,
    )
}

/// Stamp each area's `location_id` and dominant terrain onto its 8×8 tile
/// block. The block formula matches `generate_map_tiles`, so most ids already
/// line up; this also paints the terrain so distinct areas read differently in
/// the overworld. Called once at startup after the map is generated.
pub fn stamp_areas_onto_map(map: &mut MapTiles, catalog: &AreaCatalog) {
    let height = map.tiles.len() as i32;
    let width = map.tiles.first().map(|r| r.len()).unwrap_or(0) as i32;
    for area in &catalog.areas {
        let bx = (area.id % BLOCKS_PER_ROW) as i32;
        let by = (area.id / BLOCKS_PER_ROW) as i32;
        for ty in (by * REGION_SIZE)..((by + 1) * REGION_SIZE).min(height) {
            for tx in (bx * REGION_SIZE)..((bx + 1) * REGION_SIZE).min(width) {
                if let Some(tile) = map
                    .tiles
                    .get_mut(ty as usize)
                    .and_then(|row| row.get_mut(tx as usize))
                {
                    tile.location_id = area.id;
                    tile.type_id = area.terrain;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Route planning over the area graph (Dijkstra on in-game hours)
// ---------------------------------------------------------------------------

/// Returns `(total_hours, [area ids from `from` to `to`])` for the cheapest
/// overland route, or `None` if the destination is unreachable.
pub fn plan_travel(catalog: &AreaCatalog, from: u16, to: u16) -> Option<(u32, Vec<u16>)> {
    if from == to {
        return Some((0, vec![from]));
    }

    let mut dist: HashMap<u16, u32> = HashMap::new();
    let mut prev: HashMap<u16, u16> = HashMap::new();
    let mut heap: BinaryHeap<(Reverse<u32>, u16)> = BinaryHeap::new();

    dist.insert(from, 0);
    heap.push((Reverse(0), from));

    while let Some((Reverse(d), node)) = heap.pop() {
        if node == to {
            break;
        }
        if d > *dist.get(&node).unwrap_or(&u32::MAX) {
            continue;
        }
        for link in catalog.neighbors(node) {
            let next = d.saturating_add(link.hours);
            if next < *dist.get(&link.to).unwrap_or(&u32::MAX) {
                dist.insert(link.to, next);
                prev.insert(link.to, node);
                heap.push((Reverse(next), link.to));
            }
        }
    }

    let total = *dist.get(&to)?;
    let mut path = vec![to];
    let mut cur = to;
    while cur != from {
        let parent = *prev.get(&cur)?;
        path.push(parent);
        cur = parent;
    }
    path.reverse();
    Some((total, path))
}

/// The area the player is currently standing in, resolved for routing. Prefers
/// the live [`CurrentArea`], falls back to the block under the player's tile,
/// and finally to the first catalogued area so routing always has a source.
fn current_area_node(catalog: &AreaCatalog, current_area: u16, player_tile: Position) -> u16 {
    if catalog.get(current_area).is_some() {
        return current_area;
    }
    let block_id = (player_tile.x.max(0) / REGION_SIZE) as u16
        + (player_tile.y.max(0) / REGION_SIZE) as u16 * BLOCKS_PER_ROW;
    if catalog.get(block_id).is_some() {
        return block_id;
    }
    catalog.areas.first().map(|a| a.id).unwrap_or(current_area)
}

// ---------------------------------------------------------------------------
// In-game clock formatting (shared shape with hud.rs)
// ---------------------------------------------------------------------------

fn format_clock(tick: u32) -> String {
    let total_hours = tick / TIMESTAMP_TICKS_PER_HOUR;
    let day = total_hours / 24 + 1;
    let hour = total_hours % 24;
    let minute_ticks = tick % TIMESTAMP_TICKS_PER_HOUR;
    let minute = (minute_ticks * 60 / TIMESTAMP_TICKS_PER_HOUR) % 60;
    format!("Day {day} · {hour:02}:{minute:02}")
}

// ---------------------------------------------------------------------------
// Travel state
// ---------------------------------------------------------------------------

/// Tracks an in-progress overland journey. While `active`, the `Traveling`
/// state ticks the clock from `start_tick` toward `start_tick + total_ticks`
/// over `real_duration` seconds of wall-clock, then drops the party on
/// `anchor`.
#[derive(Resource, Default)]
pub struct ActiveTravel {
    pub active: bool,
    pub from_area: u16,
    pub dest_area: u16,
    pub dest_name: String,
    pub anchor: Position,
    pub start_tick: u32,
    pub total_ticks: u32,
    pub real_elapsed: f32,
    pub real_duration: f32,
}

impl ActiveTravel {
    fn fraction(&self) -> f32 {
        if self.real_duration <= 0.0 {
            1.0
        } else {
            (self.real_elapsed / self.real_duration).clamp(0.0, 1.0)
        }
    }
}

fn begin_travel(
    travel: &mut ActiveTravel,
    from: u16,
    dest: &AreaDef,
    total_ticks: u32,
    now_tick: u32,
) {
    // Scale the on-screen animation to the journey length, but keep it brief:
    // a short hop plays in ~2s, the longest legs cap at ~6s.
    let hours = total_ticks as f32 / TIMESTAMP_TICKS_PER_HOUR as f32;
    let real_duration = (hours * 0.5).clamp(2.0, 6.0);
    *travel = ActiveTravel {
        active: true,
        from_area: from,
        dest_area: dest.id,
        dest_name: dest.name.clone(),
        anchor: dest.anchor,
        start_tick: now_tick,
        total_ticks,
        real_elapsed: 0.0,
        real_duration,
    };
    info!(
        "Travel begun: {} -> {} ({} ticks, ~{:.1}h)",
        from, dest.id, total_ticks, hours
    );
}

/// Advances the clock during [`Game_State::Traveling`] and, on arrival, snaps
/// the party + camera to the destination anchor and returns to exploring.
pub fn tick_active_travel(
    time: Res<Time>,
    mut travel: ResMut<ActiveTravel>,
    mut timestamp: ResMut<Timestamp>,
    mut game_state: ResMut<GameState>,
    mut current_area: ResMut<CurrentArea>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut area_changed: ResMut<Messages<AreaChanged>>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_q: Query<&mut Transform, (With<MainCamera>, Without<Player>)>,
) {
    if game_state.0 != Game_State::Traveling || !travel.active {
        return;
    }

    travel.real_elapsed += time.delta_secs();
    let frac = travel.fraction();
    // Nothing else advances the clock during Traveling (movement/tile systems
    // gate on Exploring), so setting it absolutely keeps the journey exact.
    timestamp.0 = travel
        .start_tick
        .saturating_add((travel.total_ticks as f32 * frac) as u32);

    if frac < 1.0 {
        return;
    }

    // Arrived.
    let previous_area = current_area.0;
    current_area.0 = travel.dest_area;
    map_position.0 = travel.anchor;

    let center = tile_center_world(travel.anchor);
    if let Some(mut tf) = player_q.iter_mut().next() {
        tf.translation.x = center.x;
        tf.translation.y = center.y;
    }
    if let Some(mut cam) = camera_q.iter_mut().next() {
        cam.translation = center.extend(0.0) + iso_camera_offset();
    }

    if previous_area != travel.dest_area {
        area_changed.write(AreaChanged {
            from: previous_area,
            to: travel.dest_area,
            tile: travel.anchor,
        });
    }

    info!(
        "Arrived in {} (area {}) at {}",
        travel.dest_name,
        travel.dest_area,
        format_clock(timestamp.0)
    );
    travel.active = false;
    game_state.0 = Game_State::Exploring;
}

// ---------------------------------------------------------------------------
// World-map UI
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct WorldMapUi {
    map_root: Option<Entity>,
    overlay_root: Option<Entity>,
    /// Keyboard-focused area (the one arrows move and Enter travels to).
    focus: Option<u16>,
    /// Cached route-panel text, keyed by the destination it was computed for, so
    /// the tile-level pathfinder runs once per focus change rather than per frame.
    route_cache_dest: Option<u16>,
    route_cache_text: String,
}

#[derive(Component)]
struct WorldMapRoot;

#[derive(Component)]
pub(crate) struct AreaNodeButton {
    area_id: u16,
}

#[derive(Component)]
pub(crate) struct WorldMapInfoText;

#[derive(Component)]
struct TravelOverlayRoot;

#[derive(Component)]
pub(crate) struct TravelOverlayClock;

#[derive(Component)]
pub(crate) struct TravelOverlayBarFill;

/// `M` toggles the world map open/closed while exploring.
pub fn toggle_world_map(input: Res<ButtonInput<KeyCode>>, mut game_state: ResMut<GameState>) {
    if !input.just_pressed(KeyCode::KeyM) {
        return;
    }
    game_state.0 = match game_state.0 {
        Game_State::Exploring => Game_State::WorldMapOpen,
        Game_State::WorldMapOpen => Game_State::Exploring,
        other => other,
    };
}

/// Spawns the world-map overlay on entry to [`Game_State::WorldMapOpen`] and
/// despawns it on exit.
pub fn manage_world_map_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    catalog: Res<AreaCatalog>,
    current_area: Res<CurrentArea>,
    player_map_pos: Res<PlayerMapPosition>,
    mut ui: ResMut<WorldMapUi>,
) {
    let open = game_state.0 == Game_State::WorldMapOpen;
    if open && ui.map_root.is_none() {
        let here = current_area_node(&catalog, current_area.0, player_map_pos.0);
        ui.map_root = Some(spawn_world_map(&mut commands, &catalog, here));
        // Start the keyboard focus on the current area; clear any stale route.
        ui.focus = Some(here);
        ui.route_cache_dest = None;
        ui.route_cache_text.clear();
    } else if !open {
        if let Some(root) = ui.map_root.take() {
            commands.entity(root).despawn();
        }
        ui.focus = None;
        ui.route_cache_dest = None;
    }
}

fn spawn_world_map(commands: &mut Commands, catalog: &AreaCatalog, here: u16) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(spacing::LG),
                ..default()
            },
            BackgroundColor(palette::BG_OVERLAY),
            WorldMapRoot,
            Name::new("WorldMapOverlay"),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("World Map"),
                TextFont {
                    font_size: font_size::HEADING,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
            ));

            // Map canvas + info panel, side by side.
            root.spawn(Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(spacing::LG),
                align_items: AlignItems::Stretch,
                ..default()
            })
            .with_children(|row| {
                // --- Map canvas: a continuous grid of touching region cells. ---
                let (cols, rows) = grid_extent(catalog);
                row.spawn((
                    Node {
                        width: Val::Px(MAP_CANVAS_W),
                        height: Val::Px(MAP_CANVAS_H),
                        display: Display::Grid,
                        grid_template_columns: vec![RepeatedGridTrack::flex(cols, 1.0)],
                        grid_template_rows: vec![RepeatedGridTrack::flex(rows, 1.0)],
                        column_gap: Val::Px(CELL_GAP),
                        row_gap: Val::Px(CELL_GAP),
                        padding: UiRect::all(Val::Px(CELL_GAP)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(radius::LG)),
                        ..default()
                    },
                    BackgroundColor(palette::BG_PANEL_SUNK),
                    BorderColor::all(palette::BORDER_SUBTLE),
                ))
                .with_children(|canvas| {
                    for area in &catalog.areas {
                        spawn_area_node(canvas, area, area.id == here);
                    }
                });

                // --- Info / route panel. ---
                row.spawn((
                    Node {
                        width: Val::Px(INFO_PANEL_W),
                        height: Val::Px(MAP_CANVAS_H),
                        display: Display::Flex,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(spacing::LG)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(radius::LG)),
                        ..default()
                    },
                    BackgroundColor(palette::BG_PANEL),
                    BorderColor::all(palette::BORDER_ACCENT),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(initial_info_text(catalog, here)),
                        TextFont {
                            font_size: font_size::BODY,
                            ..default()
                        },
                        TextColor(palette::TEXT_PRIMARY),
                        WorldMapInfoText,
                    ));
                });
            });

            root.spawn((
                Text::new(
                    "Hover or arrow-key a region to plan a route · Click or Enter to travel · M/Esc to close",
                ),
                TextFont {
                    font_size: font_size::LABEL,
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
        })
        .id()
}

/// Block grid dimensions actually present in the catalog (normally the full
/// 8×8), used to size the UI grid.
fn grid_extent(catalog: &AreaCatalog) -> (u16, u16) {
    let mut cols = 1u16;
    let mut rows = 1u16;
    for area in &catalog.areas {
        cols = cols.max(area.id % BLOCKS_PER_ROW + 1);
        rows = rows.max(area.id / BLOCKS_PER_ROW + 1);
    }
    (cols, rows)
}

/// Centre of an area's grid cell in canvas pixels — used only for keyboard-nav
/// direction scoring (the cells themselves are laid out by the CSS grid).
fn canvas_px(area: &AreaDef) -> (f32, f32) {
    let bx = (area.id % BLOCKS_PER_ROW) as f32;
    let by = (area.id / BLOCKS_PER_ROW) as f32;
    (
        (bx + 0.5) / WORLD_BLOCK_COLS as f32 * MAP_CANVAS_W,
        (by + 0.5) / WORLD_BLOCK_ROWS as f32 * MAP_CANVAS_H,
    )
}

fn spawn_area_node(parent: &mut ChildSpawnerCommands, area: &AreaDef, is_here: bool) {
    let bx = (area.id % BLOCKS_PER_ROW) as i16;
    let by = (area.id / BLOCKS_PER_ROW) as i16;
    let base = terrain_color(area.terrain);
    let (bg, border) = if is_here {
        (lighten(base, 0.28), palette::ACCENT_SUCCESS)
    } else {
        (base, palette::BORDER)
    };

    parent
        .spawn((
            Button::default(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                grid_column: GridPlacement::start(bx + 1),
                grid_row: GridPlacement::start(by + 1),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::all(Val::Px(spacing::SM)),
                border: UiRect::all(Val::Px(1.0)),
                row_gap: Val::Px(2.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
            AreaNodeButton { area_id: area.id },
            Name::new(format!("AreaCell({})", area.name)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(area.name.clone()),
                TextFont {
                    font_size: font_size::LABEL,
                    ..default()
                },
                TextColor(palette::TEXT_PRIMARY),
                TextLayout {
                    justify: Justify::Center,
                    ..default()
                },
            ));
            btn.spawn((
                Text::new(if is_here { "(you are here)" } else { area_terrain_label(area.terrain) }),
                TextFont {
                    font_size: font_size::SMALL,
                    ..default()
                },
                TextColor(if is_here { palette::ACCENT_SUCCESS } else { palette::TEXT_DIM }),
            ));
        });
}

fn area_terrain_label(terrain: u8) -> &'static str {
    match terrain {
        0 => "Road",
        1 => "Plains",
        2 => "Forest",
        3 => "Mountains",
        _ => "Wilds",
    }
}

fn initial_info_text(catalog: &AreaCatalog, here: u16) -> String {
    let name = catalog.name_of(here);
    let desc = catalog
        .get(here)
        .map(|a| a.description.clone())
        .unwrap_or_default();
    format!("You are in {name}.\n\n{desc}\n\nHover a region to see the route and arrival time.")
}

/// Per-frame: updates the info panel from the hovered node and, on a click,
/// begins travel to a different area.
pub(crate) fn world_map_interaction(
    mut game_state: ResMut<GameState>,
    catalog: Res<AreaCatalog>,
    current_area: Res<CurrentArea>,
    player_map_pos: Res<PlayerMapPosition>,
    timestamp: Res<Timestamp>,
    map: Res<MapTiles>,
    slow: Res<TerrainSlowEffectIndex>,
    mut travel: ResMut<ActiveTravel>,
    mut ui: ResMut<WorldMapUi>,
    nodes: Query<(&Interaction, &AreaNodeButton)>,
    mut info_q: Query<&mut Text, With<WorldMapInfoText>>,
) {
    if game_state.0 != Game_State::WorldMapOpen {
        return;
    }

    let here = current_area_node(&catalog, current_area.0, player_map_pos.0);
    let start_tile = player_map_pos.0;

    let mut hovered: Option<u16> = None;
    let mut pressed: Option<u16> = None;
    for (interaction, node) in &nodes {
        match interaction {
            Interaction::Pressed => pressed = Some(node.area_id),
            Interaction::Hovered => hovered = hovered.or(Some(node.area_id)),
            Interaction::None => {}
        }
    }

    // The mouse takes the focus when it's over a node, so hover and keyboard
    // navigation stay in sync (and the highlight follows the pointer).
    if let Some(over) = pressed.or(hovered) {
        ui.focus = Some(over);
    }

    // Info panel reflects the current focus (mouse or keyboard), else the
    // "you are here" blurb. The route runs the tile-level pathfinder, so cache
    // it by destination and only recompute when the focus changes.
    let info_focus = pressed.or(hovered).or(ui.focus);
    if let Ok(mut text) = info_q.single_mut() {
        text.0 = match info_focus {
            None => initial_info_text(&catalog, here),
            Some(dest) => {
                if ui.route_cache_dest != Some(dest) {
                    ui.route_cache_text =
                        route_info_text(&catalog, here, start_tile, dest, &map, &slow, timestamp.0);
                    ui.route_cache_dest = Some(dest);
                }
                ui.route_cache_text.clone()
            }
        };
    }

    if let Some(dest) = pressed {
        try_travel(
            &catalog, here, start_tile, dest, &map, &slow, &mut travel, &mut game_state,
            timestamp.0,
        );
    }
}

/// Begin travel from the player's tile to `dest`'s anchor if a walkable tile
/// route exists. Travel time is the summed per-tile cost (same model as manual
/// walking), scaled by [`WORLD_TIME_SCALE`]. Shared by clicks and keyboard.
fn try_travel(
    catalog: &AreaCatalog,
    here: u16,
    start_tile: Position,
    dest: u16,
    map: &MapTiles,
    slow: &TerrainSlowEffectIndex,
    travel: &mut ActiveTravel,
    game_state: &mut GameState,
    now: u32,
) {
    if dest == here {
        return;
    }
    let Some(area) = catalog.get(dest) else {
        warn!("Travel target area {dest} not in catalog");
        return;
    };
    match shortest_time_path_and_cost(start_tile, area.anchor, map, slow) {
        Some((_path, cost)) => {
            let total_ticks = travel_ticks_for_cost(cost).max(1);
            begin_travel(travel, here, area, total_ticks, now);
            game_state.0 = Game_State::Traveling;
        }
        None => warn!("No walkable tile route from {start_tile:?} to area {dest}"),
    }
}

/// Keyboard control for the world map: arrow keys move the focus to the nearest
/// region in that direction, `Enter` departs for it, `Esc` closes the map.
fn world_map_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    catalog: Res<AreaCatalog>,
    current_area: Res<CurrentArea>,
    player_map_pos: Res<PlayerMapPosition>,
    timestamp: Res<Timestamp>,
    map: Res<MapTiles>,
    slow: Res<TerrainSlowEffectIndex>,
    mut travel: ResMut<ActiveTravel>,
    mut ui: ResMut<WorldMapUi>,
) {
    if game_state.0 != Game_State::WorldMapOpen {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        game_state.0 = Game_State::Exploring;
        return;
    }

    let here = current_area_node(&catalog, current_area.0, player_map_pos.0);
    let focus = ui.focus.unwrap_or(here);

    let dir = if keys.just_pressed(KeyCode::ArrowRight) {
        Some(Vec2::new(1.0, 0.0))
    } else if keys.just_pressed(KeyCode::ArrowLeft) {
        Some(Vec2::new(-1.0, 0.0))
    } else if keys.just_pressed(KeyCode::ArrowDown) {
        Some(Vec2::new(0.0, 1.0))
    } else if keys.just_pressed(KeyCode::ArrowUp) {
        Some(Vec2::new(0.0, -1.0))
    } else {
        None
    };
    if let Some(dir) = dir {
        if let Some(next) = nearest_in_direction(&catalog, focus, dir) {
            ui.focus = Some(next);
        }
    }

    if keys.just_pressed(KeyCode::Enter) {
        try_travel(
            &catalog, here, player_map_pos.0, focus, &map, &slow, &mut travel, &mut game_state,
            timestamp.0,
        );
    }
}

/// Pick the area whose canvas position lies most directly in `dir` from `from`.
/// Scores by distance along `dir` plus a penalty for sideways drift, so arrows
/// feel like "go to the region over there".
fn nearest_in_direction(catalog: &AreaCatalog, from: u16, dir: Vec2) -> Option<u16> {
    let from_area = catalog.get(from)?;
    let (fx, fy) = canvas_px(from_area);
    let dir = dir.normalize_or_zero();
    let mut best: Option<(u16, f32)> = None;
    for area in &catalog.areas {
        if area.id == from {
            continue;
        }
        let (cx, cy) = canvas_px(area);
        let delta = Vec2::new(cx - fx, cy - fy);
        let along = delta.dot(dir);
        if along <= 1.0 {
            continue; // not in this direction
        }
        let perp = (delta - dir * along).length();
        let score = along + perp * 2.0;
        if best.map(|(_, b)| score < b).unwrap_or(true) {
            best = Some((area.id, score));
        }
    }
    best.map(|(id, _)| id)
}

/// Recolor area nodes each frame so the current area and the focused
/// destination stand out. Runs in `PostUpdate` so it wins over the shared
/// `update_standard_button_visuals` hover restyle.
fn sync_world_map_nodes(
    game_state: Res<GameState>,
    catalog: Res<AreaCatalog>,
    current_area: Res<CurrentArea>,
    player_map_pos: Res<PlayerMapPosition>,
    ui: Res<WorldMapUi>,
    mut nodes: Query<(&AreaNodeButton, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
) {
    if game_state.0 != Game_State::WorldMapOpen {
        return;
    }
    let here = current_area_node(&catalog, current_area.0, player_map_pos.0);
    let focus = ui.focus;

    for (node, interaction, mut bg, mut border) in &mut nodes {
        let is_here = node.area_id == here;
        let is_focus = focus == Some(node.area_id);
        let hovered = *interaction != Interaction::None;

        let base = terrain_color(catalog.get(node.area_id).map(|a| a.terrain).unwrap_or(255));
        let (bg_c, border_c) = if is_here {
            (lighten(base, 0.28), palette::ACCENT_SUCCESS)
        } else if is_focus {
            (lighten(base, 0.18), palette::BORDER_ACCENT)
        } else if hovered {
            (lighten(base, 0.12), palette::BORDER_HOVER)
        } else {
            (base, palette::BORDER)
        };
        // Only write when the color actually changes — an unconditional write
        // flags BackgroundColor/BorderColor dirty every frame and forces the UI
        // to re-extract these nodes even when nothing visually changed.
        if bg.0 != bg_c {
            bg.0 = bg_c;
        }
        if border.top != border_c {
            border.top = border_c;
            border.right = border_c;
            border.bottom = border_c;
            border.left = border_c;
        }
    }
}

fn route_info_text(
    catalog: &AreaCatalog,
    here: u16,
    start_tile: Position,
    dest: u16,
    map: &MapTiles,
    slow: &TerrainSlowEffectIndex,
    now: u32,
) -> String {
    let dest_name = catalog.name_of(dest);
    let desc = catalog
        .get(dest)
        .map(|a| a.description.clone())
        .unwrap_or_default();
    if dest == here {
        return format!("{dest_name}\n\n{desc}\n\nYou are already here.");
    }
    let Some(area) = catalog.get(dest) else {
        return format!("{dest_name}\n\n{desc}\n\nNo known route from here.");
    };
    match shortest_time_path_and_cost(start_tile, area.anchor, map, slow) {
        Some((path, cost)) => {
            let ticks = travel_ticks_for_cost(cost);
            let arrival = now.saturating_add(ticks);
            let route = route_region_names(&path, map, catalog);
            format!(
                "{dest_name}\n\n{desc}\n\nTravel time: {}\nArrive: {}\n\nRoute: {}\n\n[ Click to depart ]",
                format_duration(ticks),
                format_clock(arrival),
                route.join(" → ")
            )
        }
        None => format!("{dest_name}\n\n{desc}\n\nNo overland route from here."),
    }
}

/// Distinct regions the tile path passes through, in order, for the route line.
fn route_region_names(path: &[Position], map: &MapTiles, catalog: &AreaCatalog) -> Vec<String> {
    let mut names = Vec::new();
    let mut last: Option<u16> = None;
    for pos in path {
        let id = map
            .tiles
            .get(pos.y as usize)
            .and_then(|row| row.get(pos.x as usize))
            .map(|tile| tile.location_id);
        if let Some(id) = id {
            if last != Some(id) {
                names.push(catalog.name_of(id));
                last = Some(id);
            }
        }
    }
    names
}

/// Format a tick span as `Hh MMm` (or `Mm` under an hour) of in-game time.
fn format_duration(ticks: u32) -> String {
    let secs = ticks as u64 * TIMESTAMP_SECONDS_PER_TICK as u64;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

// ---------------------------------------------------------------------------
// Traveling overlay (animated progress + live clock)
// ---------------------------------------------------------------------------

/// Spawns/despawns the traveling overlay and updates its progress bar + clock.
pub(crate) fn manage_travel_overlay(
    mut commands: Commands,
    game_state: Res<GameState>,
    travel: Res<ActiveTravel>,
    timestamp: Res<Timestamp>,
    mut ui: ResMut<WorldMapUi>,
    mut clock_q: Query<&mut Text, With<TravelOverlayClock>>,
    mut bar_q: Query<&mut Node, With<TravelOverlayBarFill>>,
) {
    let traveling = game_state.0 == Game_State::Traveling;

    if !traveling {
        if let Some(root) = ui.overlay_root.take() {
            commands.entity(root).despawn();
        }
        return;
    }

    if ui.overlay_root.is_none() {
        ui.overlay_root = Some(spawn_travel_overlay(&mut commands, &travel.dest_name));
    }

    let frac = travel.fraction();
    if let Ok(mut fill) = bar_q.single_mut() {
        fill.width = Val::Px(TRAVEL_BAR_W * frac);
    }
    if let Ok(mut text) = clock_q.single_mut() {
        text.0 = format_clock(timestamp.0);
    }
}

fn spawn_travel_overlay(commands: &mut Commands, dest_name: &str) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(spacing::LG),
                ..default()
            },
            BackgroundColor(palette::BG_OVERLAY),
            TravelOverlayRoot,
            Name::new("TravelOverlay"),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(format!("Traveling to {dest_name}…")),
                TextFont {
                    font_size: font_size::HEADING,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
            ));

            // Progress bar track + fill.
            root.spawn((
                Node {
                    width: Val::Px(TRAVEL_BAR_W),
                    height: Val::Px(TRAVEL_BAR_H),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(radius::SM)),
                    ..default()
                },
                BackgroundColor(palette::BG_PANEL_SUNK),
                BorderColor::all(palette::BORDER_SUBTLE),
            ))
            .with_children(|track| {
                track.spawn((
                    Node {
                        width: Val::Px(0.0),
                        height: Val::Percent(100.0),
                        border_radius: BorderRadius::all(Val::Px(radius::SM)),
                        ..default()
                    },
                    BackgroundColor(palette::ACCENT_PRIMARY),
                    TravelOverlayBarFill,
                ));
            });

            root.spawn((
                Text::new(""),
                TextFont {
                    font_size: font_size::SUBHEADING,
                    ..default()
                },
                TextColor(palette::TEXT_SECONDARY),
                TravelOverlayClock,
            ));
        })
        .id()
}

// ---------------------------------------------------------------------------
// RON loading + built-in fallback
// ---------------------------------------------------------------------------

fn load_areas_data_file() -> Option<AreasDataFile> {
    let contents = match fs::read_to_string(AREAS_DATA_PATH) {
        Ok(s) => s,
        Err(err) => {
            warn!("Failed to open {}: {}", AREAS_DATA_PATH, err);
            return None;
        }
    };
    match ron::de::from_str::<AreasDataFile>(&contents) {
        Ok(data) => Some(data),
        Err(err) => {
            warn!("Failed to parse {}: {}", AREAS_DATA_PATH, err);
            None
        }
    }
}

/// Built-in fallback areas, kept in sync with `assets/data/areas.ron`. Ids are
/// the tile-block `location_id`s (`bx + by*8`): a 4×3 grid — row 0 = 0,1,2,3 ·
/// row 1 = 8,9,10,11 · row 2 = 16,17,18,19. Greenford (0) and Ironpass (1)
/// align with the seeded cities; the world-map canvas lays them out in 4
/// columns (0.13/0.38/0.63/0.88) and 3 rows (0.18/0.50/0.82).
fn seed_default_areas() -> Vec<AreaDef> {
    let link = |to: u16, hours: u32| AreaLink { to, hours };
    let area = |id, name: &str, description: &str, anchor, terrain, ui_x, ui_y, connections| {
        AreaDef {
            id,
            name: name.to_string(),
            description: description.to_string(),
            anchor,
            terrain,
            ui_x,
            ui_y,
            connections,
        }
    };
    vec![
        // --- Row 0 ---
        area(0, "Greenford Village",
            "Terraced rice paddies under the Mizuno banner; the road home.",
            Position { x: 3, y: 3 }, 1, 0.13, 0.18, vec![link(1, 5), link(8, 3)]),
        area(1, "Ironpass Castle Town",
            "A Takeda stronghold of forges and gates guarding the mountain pass.",
            Position { x: 11, y: 3 }, 3, 0.38, 0.18, vec![link(2, 3), link(9, 4)]),
        area(2, "Saltmarsh Crossing",
            "A reed-choked tidal ford where smugglers and pilgrims trade news.",
            Position { x: 19, y: 3 }, 1, 0.63, 0.18, vec![link(3, 4), link(10, 3)]),
        area(3, "Tideport Harbor",
            "A salt-bleached harbor of junk ships and foreign coin at the eastern shore.",
            Position { x: 27, y: 3 }, 1, 0.88, 0.18, vec![link(11, 4)]),
        // --- Row 1 ---
        area(8, "Mistwood",
            "A fog-bound old-growth forest; yokai are said to walk its deer trails.",
            Position { x: 3, y: 11 }, 2, 0.13, 0.50, vec![link(9, 2), link(16, 3)]),
        area(9, "Kaze Shrine Vale",
            "A wind-bell shrine in a quiet valley, sacred to the Kamishin rites.",
            Position { x: 11, y: 11 }, 1, 0.38, 0.50, vec![link(10, 3), link(17, 2)]),
        area(10, "Oni's Hollow",
            "A scarred hollow where the veil thins and the kegare runs deep.",
            Position { x: 19, y: 11 }, 2, 0.63, 0.50, vec![link(11, 3), link(18, 4)]),
        area(11, "Cinderpeak Mine",
            "Terraced ore galleries cut into a smoking peak; the Takeda's iron heart.",
            Position { x: 27, y: 11 }, 3, 0.88, 0.50, vec![link(19, 3)]),
        // --- Row 2 ---
        area(16, "Reedlight Hamlet",
            "A lamp-lit cluster of stilt houses where the marsh meets the southern road.",
            Position { x: 3, y: 19 }, 1, 0.13, 0.82, vec![link(17, 3)]),
        area(17, "Hollowfen",
            "A drowned forest of black water and will-o'-wisps; few return by night.",
            Position { x: 11, y: 19 }, 2, 0.38, 0.82, vec![link(18, 4)]),
        area(18, "Thunder Terraces",
            "Storm-wracked highland steps where mountain ascetics test the Kiho.",
            Position { x: 19, y: 19 }, 3, 0.63, 0.82, vec![link(19, 3)]),
        area(19, "Lantern Bay",
            "A southern cove of paper lanterns and quiet tea-houses below the cliffs.",
            Position { x: 27, y: 19 }, 1, 0.88, 0.82, vec![]),
    ]
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct AreasPlugin;

impl Plugin for AreasPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AreaCatalog>()
            .init_resource::<ActiveTravel>()
            .init_resource::<WorldMapUi>()
            .add_systems(
                Update,
                (
                    toggle_world_map,
                    manage_world_map_ui,
                    world_map_keyboard,
                    world_map_interaction,
                    tick_active_travel,
                    manage_travel_overlay,
                )
                    .chain(),
            )
            .add_systems(PostUpdate, sync_world_map_nodes);
    }
}
