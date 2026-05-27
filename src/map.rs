use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::input::{keyboard::KeyCode, mouse::MouseButton};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use rand::Rng;

use crate::core::{GameState, Game_State, MainCamera, Player, PlayerMapPosition, Position, Timestamp};
use crate::constants::{WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::light_plugin::Occluder;
use crate::quadtree::Collider;
use crate::ui_style::{font_size, palette, radius, spacing};
use bevy_camera::visibility::RenderLayers;

/// World-space size of each map tile background (square).
pub const TILE_WORLD_SIZE: f32 = 4096.0;
pub const LOCAL_MAP_BORDER_THICKNESS: f32 = 24.0;
pub const LOCAL_MAP_BORDER_INSET: f32 = 8.0;

pub enum TravelingSpeed {
    Slow,
    Normal,
    Fast,
}

pub enum TravelingMethod {
    Walk,
    Horse,
    Chariot,
    Boat,
}

#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct MapTiles {
    pub tiles: Vec<Vec<MapTile>>,
}

#[derive(Clone, Debug)]
pub struct TerrainSlowEffect {
    /// Label for debug/UI.
    pub name: String,
    /// Tile where this local effect applies (e.g. mud puddle).
    pub tile: Position,
    /// Subtractive penalty applied after base terrain multiplier.
    pub speed_penalty: f32,
    /// Extra travel-time cost used by path/travel time calculations.
    pub extra_time_cost: u32,
}

#[derive(Resource, Default, Clone, Debug)]
pub struct TerrainSlowEffectList {
    pub effects: Vec<TerrainSlowEffect>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TerrainSlowEffectTotals {
    pub speed_penalty: f32,
    pub extra_time_cost: u32,
}

#[derive(Resource, Default, Clone, Debug)]
pub struct TerrainSlowEffectIndex {
    pub totals: HashMap<Position, TerrainSlowEffectTotals>,
    pub revision: u64,
    pub initialized: bool,
}

/// Tracks the currently loaded area/location.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CurrentArea(pub u16);

/// Tracks spawned background entities for nearby tiles.
#[derive(Resource, Default)]
pub struct ActiveMapBackgrounds {
    pub entities: HashMap<Position, Entity>,
    pub border_entities: Vec<Entity>,
}

/// Tracks the map overlay sprite shown while the travel map is open.
#[derive(Resource, Default)]
pub struct MapOverlay {
    pub entity: Option<Entity>,
}

#[derive(Component)]
pub struct MapOverlayRoot;

#[derive(Resource, Default)]
pub struct MapTravelUi {
    pub root: Option<Entity>,
    pub label: Option<Entity>,
}

#[derive(Component)]
pub struct MapTravelUiRoot;

#[derive(Component)]
pub struct MapTravelUiText;

/// Marks spawned content for a specific tile so it can be culled when far away.
#[derive(Component, Clone, Copy, Debug)]
pub struct TileSpawn {
    pub coords: Position,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TileContentState {
    pub spawned_obstacle: bool,
}

/// Stores per-tile content state so it persists across despawn/respawn.
#[derive(Resource, Default)]
pub struct TileContentCache {
    pub states: HashMap<Position, TileContentState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapTile {
    pub time: u32,
    pub location_id: u16,
    pub type_id: u8,
    pub event_ids: Vec<u32>,
    pub items_id: Option<Vec<u16>>,
    pub image_path: String, // The path will be named with the coordinates of each tile, e.g., "map_tiles/tile_0_0.png". This way I can make a script to auto-generate the map tiles later.
}

impl Default for MapTile {
    fn default() -> Self {
        Self {
            time: 1,
            location_id: 0,
            type_id: 0,
            event_ids: Vec::new(),
            items_id: None,
            image_path: "character.png".to_string(), // placeholder; replace with real tile art
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainTravelProfile {
    pub movement_speed_multiplier: f32,
    pub tile_time_cost: u32,
}

/// Returns tuning values shared by manual movement and map fast travel.
/// Type mapping is provisional; adjust when terrain IDs are finalized.
/// 0 = road, 1 = plains, 2 = forest, 3 = mountains, fallback = road.
pub fn terrain_travel_profile_for_type(type_id: u8) -> TerrainTravelProfile {
    match type_id {
        // road
        0 => TerrainTravelProfile {
            movement_speed_multiplier: 1.0,
            tile_time_cost: 1,
        },
        // plains
        1 => TerrainTravelProfile {
            movement_speed_multiplier: 0.92,
            tile_time_cost: 2,
        },
        // forest
        2 => TerrainTravelProfile {
            movement_speed_multiplier: 0.86,
            tile_time_cost: 3,
        },
        // mountains
        3 => TerrainTravelProfile {
            movement_speed_multiplier: 0.80,
            tile_time_cost: 4,
        },
        _ => TerrainTravelProfile {
            movement_speed_multiplier: 1.0,
            tile_time_cost: 1,
        },
    }
}

pub fn terrain_travel_profile_for_tile(tile: &MapTile) -> TerrainTravelProfile {
    terrain_travel_profile_for_type(tile.type_id)
}

/// Returns the time cost to enter/traverse a tile based on its type.
pub fn time_cost_for_tile(tile: &MapTile) -> u32 {
    terrain_travel_profile_for_tile(tile).tile_time_cost
}

pub fn rebuild_terrain_slow_effect_index(
    slow_effects: Res<TerrainSlowEffectList>,
    mut index: ResMut<TerrainSlowEffectIndex>,
) {
    if !slow_effects.is_changed() && index.initialized {
        return;
    }

    index.totals.clear();
    for effect in &slow_effects.effects {
        let entry = index.totals.entry(effect.tile).or_default();
        entry.speed_penalty += effect.speed_penalty.max(0.0);
        entry.extra_time_cost = entry
            .extra_time_cost
            .saturating_add(effect.extra_time_cost);
    }
    index.revision = index.revision.wrapping_add(1);
    index.initialized = true;
}

pub fn slow_effect_totals_at_tile(
    tile_pos: Position,
    effects: &TerrainSlowEffectIndex,
) -> TerrainSlowEffectTotals {
    effects.totals.get(&tile_pos).copied().unwrap_or_default()
}

pub fn additional_speed_penalty_at_tile(
    tile_pos: Position,
    effects: &TerrainSlowEffectList,
) -> f32 {
    effects
        .effects
        .iter()
        .filter(|effect| effect.tile == tile_pos)
        .map(|effect| effect.speed_penalty.max(0.0))
        .sum()
}

pub fn additional_speed_penalty_at_tile_indexed(
    tile_pos: Position,
    effects: &TerrainSlowEffectIndex,
) -> f32 {
    slow_effect_totals_at_tile(tile_pos, effects).speed_penalty
}

pub fn additional_time_cost_at_tile(tile_pos: Position, effects: &TerrainSlowEffectList) -> u32 {
    effects
        .effects
        .iter()
        .filter(|effect| effect.tile == tile_pos)
        .map(|effect| effect.extra_time_cost)
        .sum()
}

pub fn additional_time_cost_at_tile_indexed(
    tile_pos: Position,
    effects: &TerrainSlowEffectIndex,
) -> u32 {
    slow_effect_totals_at_tile(tile_pos, effects).extra_time_cost
}

pub fn total_time_cost_for_tile(
    tile: &MapTile,
    tile_pos: Position,
    effects: &TerrainSlowEffectList,
) -> u32 {
    time_cost_for_tile(tile).saturating_add(additional_time_cost_at_tile(tile_pos, effects))
}

pub fn total_time_cost_for_tile_indexed(
    tile: &MapTile,
    tile_pos: Position,
    effects: &TerrainSlowEffectIndex,
) -> u32 {
    time_cost_for_tile(tile).saturating_add(additional_time_cost_at_tile_indexed(tile_pos, effects))
}

/// Returns the movement multiplier applied for manual exploration movement.
pub fn movement_speed_multiplier_for_tile(tile: &MapTile) -> f32 {
    terrain_travel_profile_for_tile(tile).movement_speed_multiplier
}

/// Looks up terrain movement multiplier from a world-space position.
pub fn movement_speed_multiplier_at_world(map: &MapTiles, world_pos: Vec2) -> f32 {
    let tile = world_to_map_tile(world_pos);
    let tile_x = tile.x;
    let tile_y = tile.y;

    if tile_x < 0 || tile_y < 0 {
        return 1.0;
    }

    map.tiles
        .get(tile_y as usize)
        .and_then(|row| row.get(tile_x as usize))
        .map(movement_speed_multiplier_for_tile)
        .unwrap_or(1.0)
}

pub fn movement_speed_multiplier_with_effects_at_world(
    map: &MapTiles,
    effects: &TerrainSlowEffectIndex,
    world_pos: Vec2,
) -> f32 {
    let tile_pos = world_to_map_tile(world_pos);

    let base = movement_speed_multiplier_at_world(map, world_pos);
    let penalty = additional_speed_penalty_at_tile_indexed(tile_pos, effects);
    (base - penalty).clamp(0.2, 1.0)
}

pub fn world_to_map_tile(world_pos: Vec2) -> Position {
    Position {
        x: (world_pos.x / TILE_WORLD_SIZE).floor() as i32,
        y: (world_pos.y / TILE_WORLD_SIZE).floor() as i32,
    }
}

pub fn tile_origin_world(tile: Position) -> Vec2 {
    Vec2::new(
        tile.x as f32 * TILE_WORLD_SIZE,
        tile.y as f32 * TILE_WORLD_SIZE,
    )
}

pub fn tile_center_world(tile: Position) -> Vec2 {
    tile_origin_world(tile) + Vec2::splat(TILE_WORLD_SIZE * 0.5)
}

pub fn tile_bounds_world(tile: Position) -> Rect {
    Rect::from_corners(
        tile_origin_world(tile),
        tile_origin_world(tile) + Vec2::splat(TILE_WORLD_SIZE),
    )
}

fn clamp_world_inside_tile(world_pos: Vec2, tile: Position) -> Vec2 {
    let bounds = tile_bounds_world(tile);
    Vec2::new(
        world_pos
            .x
            .clamp(bounds.min.x + LOCAL_MAP_BORDER_INSET, bounds.max.x - LOCAL_MAP_BORDER_INSET),
        world_pos
            .y
            .clamp(bounds.min.y + LOCAL_MAP_BORDER_INSET, bounds.max.y - LOCAL_MAP_BORDER_INSET),
    )
}

/// Stores the player's cursor/selection when in map (travel) mode.
#[derive(Resource, Default)]
pub struct MapSelection(pub Position);

/// Tracks the most recently entered tile while exploring.
#[derive(Resource, Default)]
pub struct LastEnteredTile(pub Position);

/// Tracks pending and active tile events (only one can run at a time).
#[derive(Resource, Default)]
pub struct ActiveTileEvent {
    pub current: Option<u32>,
    pub pending: Vec<u32>,
}

/// Fired when a tile event should start.
#[derive(Message)]
pub struct TileEventTriggered {
    pub tile: Position,
    pub event_id: u32,
}

/// Fired by event handlers when the active event is complete.
#[derive(Message)]
pub struct TileEventCompleted {
    pub event_id: u32,
}

/// Fired when the player enters a tile with a different area/location id.
#[derive(Message)]
pub struct AreaChanged {
    pub from: u16,
    pub to: u16,
    pub tile: Position,
}

/// Fires once when the player crosses into a new tile, *before* area-change
/// and tile-event resolution. Listeners can react conditionally on the cause
/// (player walk, teleport ability, scripted world event) and mutate world
/// state before the entry is finalized.
#[derive(Debug, Clone, Message)]
pub struct BeforeTileEnterEvent {
    pub tile: Position,
    pub from: Position,
    pub cause: crate::combat_plugin::ActionCause,
}

/// Fires once after a new-tile entry has been fully processed (area change
/// recorded, pending events queued). Symmetric counterpart to
/// `BeforeTileEnterEvent`; suitable for post-arrival reactions that should
/// not block the entry itself.
#[derive(Debug, Clone, Message)]
pub struct AfterTileEnterEvent {
    pub tile: Position,
    pub from: Position,
    pub cause: crate::combat_plugin::ActionCause,
}

#[derive(Component)]
pub struct MapPathMarker;

#[derive(Resource, Default)]
pub struct MapPathPreview {
    pub entities: Vec<Entity>,
    pub last_start: Option<Position>,
    pub last_dest: Option<Position>,
    pub last_effect_revision: u64,
}

#[derive(Resource, Default)]
pub struct MapTravelPathCache {
    pub start: Option<Position>,
    pub dest: Option<Position>,
    pub effect_revision: u64,
    pub available: bool,
    pub path: Vec<Position>,
    pub cost: u32,
}

#[derive(Resource, Default)]
pub struct AreaTransitionLog {
    pub last: Option<AreaChanged>,
}

/// Generates a simple flat map to enable travel mode.
pub fn generate_map_tiles() -> MapTiles {
    // Use GRID_WIDTH/HEIGHT as coarse bounds; keep it manageable.
    let width = 64;
    let height = 64;
    let mut tiles = Vec::with_capacity(height);
    let region_size = 8;

    for y in 0..height {
        let mut row = Vec::with_capacity(width);
        for x in 0..width {
            let type_id = ((x + y) % 4) as u8;
            let location_id = (x / region_size + (y / region_size) * (width / region_size)) as u16;
            let image_path = match type_id {
                0 => "character.png",
                1 => "dot.png",
                2 => "dot.png",
                _ => "character.png",
            }
            .to_string();

            let mut event_ids = Vec::new();
            if (x * y) % 17 == 0 {
                event_ids.push(1000);
            }
            if (x + y) % 29 == 0 {
                event_ids.push(2000);
            }

            row.push(MapTile {
                time: 1,
                location_id,
                type_id,
                event_ids,
                items_id: None,
                image_path,
            });
        }
        tiles.push(row);
    }

    MapTiles { tiles }
}

/// Toggle entering the map (travel mode) with `M` when exploring.
pub fn toggle_map_mode(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    map_position: Res<PlayerMapPosition>,
    mut selection: ResMut<MapSelection>,
    mut commands: Commands,
    mut overlay: ResMut<MapOverlay>,
    asset_server: Res<AssetServer>,
    windows: Query<&Window>,
) {
    if !input.just_pressed(KeyCode::KeyM) {
        return;
    }

    game_state.0 = match game_state.0 {
        Game_State::Exploring => {
            // When entering travel mode, start the cursor at the player's current map position.
            selection.0 = map_position.0;
            if overlay.entity.is_none() {
                let (width, height) = windows
                    .iter()
                    .next()
                    .map(|w| (w.width(), w.height()))
                    .unwrap_or((WINDOW_WIDTH, WINDOW_HEIGHT));

                let texture = asset_server.load("dot.png");
                let entity = commands
                    .spawn((
                        Sprite {
                            image: texture,
                            color: Color::srgba(1.0, 1.0, 1.0, 0.85),
                            custom_size: Some(Vec2::new(width, height)),
                            ..default()
                        },
                        Transform::from_translation(Vec3::new(0.0, 0.0, 100.0)),
                        MapOverlayRoot,
                        Name::new("MapOverlay"),
                    ))
                    .id();
                overlay.entity = Some(entity);
            }
            info!(
                "Entering travel map mode at selection ({}, {})",
                selection.0.x, selection.0.y
            );
            Game_State::MapOpen
        }
        Game_State::MapOpen => {
            info!("Exiting travel map mode, returning to exploring");
            if let Some(entity) = overlay.entity.take() {
                commands.entity(entity).despawn();
            }
            Game_State::Exploring
        }
        other => other,
    };
}

/// Move the map selection tile-by-tile with WASD/arrow keys when in travel mode.
pub fn navigate_map_selection_keyboard(
    input: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<MapSelection>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<Timestamp>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_tf_q: Query<&mut Transform, (With<MainCamera>, Without<Player>)>,
    mut game_state: ResMut<GameState>,
    map: Res<MapTiles>,
) {
    if game_state.0 != Game_State::MapOpen {
        return;
    }

    let mut delta = IVec2::ZERO;
    if input.just_pressed(KeyCode::ArrowUp) || input.just_pressed(KeyCode::KeyW) {
        delta.y += 1;
    }
    if input.just_pressed(KeyCode::ArrowDown) || input.just_pressed(KeyCode::KeyS) {
        delta.y -= 1;
    }
    if input.just_pressed(KeyCode::ArrowRight) || input.just_pressed(KeyCode::KeyD) {
        delta.x += 1;
    }
    if input.just_pressed(KeyCode::ArrowLeft) || input.just_pressed(KeyCode::KeyA) {
        delta.x -= 1;
    }

    let height = map.tiles.len() as i32;
    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;

    if delta != IVec2::ZERO {
        let new_x = (selection.0.x + delta.x).clamp(0, width.saturating_sub(1));
        let new_y = (selection.0.y + delta.y).clamp(0, height.saturating_sub(1));
        selection.0.x = new_x;
        selection.0.y = new_y;
        return;
    }

}

/// Click-to-travel while the map is open.
pub fn navigate_map_selection_mouse(
    mouse_input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut selection: ResMut<MapSelection>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<Timestamp>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_tf_q: Query<&mut Transform, (With<MainCamera>, Without<Player>)>,
    mut game_state: ResMut<GameState>,
    map: Res<MapTiles>,
    slow_effects: Res<TerrainSlowEffectIndex>,
    mut path_cache: ResMut<MapTravelPathCache>,
) {
    if game_state.0 != Game_State::MapOpen {
        return;
    }

    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(window) = windows.iter().next() else {
        warn!("navigate_map_selection_mouse: could not get primary window for click");
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Some((camera, cam_tf)) = camera_q.iter().next() else {
        warn!("navigate_map_selection_mouse: missing main camera for click handling");
        return;
    };
    let Ok(world_pos) = camera.viewport_to_world_2d(cam_tf, cursor_pos) else {
        return;
    };

    let target_tile = world_to_map_tile(world_pos);

    if travel_to_destination(
        target_tile,
        &mut selection,
        &mut map_position,
        &mut current_area,
        &map,
        &slow_effects,
        &mut path_cache,
        &mut timestamp,
        &mut player_q,
        &mut camera_tf_q,
    ) {
        game_state.0 = Game_State::Exploring;
    }
}

/// Confirm travel to the selected tile with Enter/Space.
/// Teleports the player and camera to the tile center and returns to exploring mode.
pub fn confirm_travel(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    mut selection: ResMut<MapSelection>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_q: Query<&mut Transform, (With<MainCamera>, Without<Player>)>,
    map: Res<MapTiles>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<Timestamp>,
    slow_effects: Res<TerrainSlowEffectIndex>,
    mut path_cache: ResMut<MapTravelPathCache>,
) {
    if game_state.0 != Game_State::MapOpen {
        return;
    }

    if !(input.just_pressed(KeyCode::Enter) || input.just_pressed(KeyCode::Space)) {
        return;
    }

    if travel_to_destination(
        selection.0,
        &mut selection,
        &mut map_position,
        &mut current_area,
        &map,
        &slow_effects,
        &mut path_cache,
        &mut timestamp,
        &mut player_q,
        &mut camera_q,
    ) {
        game_state.0 = Game_State::Exploring;
    }
}

/// Find the lowest-time path (Dijkstra) between tiles, returning the path and cost.
fn shortest_time_path_and_cost(
    start: Position,
    dest: Position,
    map: &MapTiles,
    slow_effects: &TerrainSlowEffectIndex,
) -> Option<(Vec<Position>, u32)> {
    let height = map.tiles.len() as i32;
    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;
    if width == 0 || height == 0 {
        return None;
    }

    let start = Position {
        x: start.x.clamp(0, width.saturating_sub(1)),
        y: start.y.clamp(0, height.saturating_sub(1)),
    };
    let dest = Position {
        x: dest.x.clamp(0, width.saturating_sub(1)),
        y: dest.y.clamp(0, height.saturating_sub(1)),
    };

    if start == dest {
        return Some((vec![start], 0));
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let cell_count = width_usize * height_usize;
    let tile_index =
        |pos: Position| -> usize { pos.x as usize + pos.y as usize * width_usize };
    let tile_position = |index: usize| -> Position {
        let x = (index % width_usize) as i32;
        let y = (index / width_usize) as i32;
        Position { x, y }
    };

    let start_index = tile_index(start);
    let dest_index = tile_index(dest);

    let mut dist = vec![u32::MAX; cell_count];
    let mut prev = vec![None; cell_count];
    let mut heap: BinaryHeap<(Reverse<u32>, usize)> = BinaryHeap::new();

    dist[start_index] = 0;
    heap.push((Reverse(0), start_index));

    while let Some((Reverse(cost), index)) = heap.pop() {
        if index == dest_index {
            break;
        }
        if cost > dist[index] {
            continue;
        }

        let pos = tile_position(index);
        if pos == dest {
            break;
        }

        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let nx = pos.x + dx;
            let ny = pos.y + dy;
            if nx < 0 || ny < 0 || nx >= width || ny >= height {
                continue;
            }

            let Some(tile) = map
                .tiles
                .get(ny as usize)
                .and_then(|row| row.get(nx as usize))
            else {
                continue;
            };

            let next_pos = Position { x: nx, y: ny };
            let next_index = tile_index(next_pos);
            let step_cost = total_time_cost_for_tile_indexed(tile, next_pos, slow_effects);
            let next_cost = cost.saturating_add(step_cost);

            if next_cost < dist[next_index] {
                dist[next_index] = next_cost;
                prev[next_index] = Some(index);
                heap.push((Reverse(next_cost), next_index));
            }
        }
    }

    let total_cost = dist[dest_index];
    if total_cost == u32::MAX {
        return None;
    }

    let mut path = vec![dest];
    let mut current = dest_index;
    while current != start_index {
        let Some(parent) = prev[current] else {
            break;
        };
        current = parent;
        path.push(tile_position(current));
    }
    path.reverse();

    Some((path, total_cost))
}

fn cached_shortest_time_path_and_cost<'a>(
    start: Position,
    dest: Position,
    map: &MapTiles,
    slow_effects: &TerrainSlowEffectIndex,
    cache: &'a mut MapTravelPathCache,
) -> Option<(&'a [Position], u32)> {
    if cache.start == Some(start)
        && cache.dest == Some(dest)
        && cache.effect_revision == slow_effects.revision
    {
        return if cache.available {
            Some((cache.path.as_slice(), cache.cost))
        } else {
            None
        };
    }

    cache.start = Some(start);
    cache.dest = Some(dest);
    cache.effect_revision = slow_effects.revision;

    if let Some((path, cost)) = shortest_time_path_and_cost(start, dest, map, slow_effects) {
        cache.path = path;
        cache.cost = cost;
        cache.available = true;
        Some((cache.path.as_slice(), cache.cost))
    } else {
        cache.path.clear();
        cache.cost = 0;
        cache.available = false;
        None
    }
}

/// Apply travel to a destination, updating selection, time, area, and transforms.
fn travel_to_destination(
    dest: Position,
    selection: &mut MapSelection,
    map_position: &mut PlayerMapPosition,
    current_area: &mut CurrentArea,
    map: &MapTiles,
    slow_effects: &TerrainSlowEffectIndex,
    path_cache: &mut MapTravelPathCache,
    timestamp: &mut Timestamp,
    player_q: &mut Query<&mut Transform, With<Player>>,
    camera_q: &mut Query<&mut Transform, (With<MainCamera>, Without<Player>)>,
) -> bool {
    let start = map_position.0;
    let Some((path, travel_time)) =
        cached_shortest_time_path_and_cost(start, dest, map, slow_effects, path_cache)
    else {
        warn!(
            "travel_to_destination: could not compute path from ({}, {}) to ({}, {})",
            start.x, start.y, dest.x, dest.y
        );
        return false;
    };

    let final_dest = path.last().copied().unwrap_or(start);
    selection.0 = final_dest;
    map_position.0 = final_dest;

    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;
    let height = map.tiles.len() as i32;
    let x = final_dest.x.clamp(0, width.saturating_sub(1)) as usize;
    let y = final_dest.y.clamp(0, height.saturating_sub(1)) as usize;
    if let Some(tile) = map.tiles.get(y).and_then(|row| row.get(x)) {
        current_area.0 = tile.location_id;
    }

    timestamp.0 = timestamp.0.saturating_add(travel_time.max(1));

    let world_center = tile_center_world(final_dest);

    if let Some(mut tf) = player_q.iter_mut().next() {
        tf.translation.x = world_center.x;
        tf.translation.y = world_center.y;
    } else {
        warn!("travel_to_destination: player transform not found");
    }

    // Snap the iso camera to the destination (ground focus + fixed offset) so a
    // long-distance travel lands cleanly instead of panning across the world.
    if let Some(mut cam_tf) = camera_q.iter_mut().next() {
        cam_tf.translation = world_center.extend(0.0) + crate::render3d::iso_camera_offset();
    } else {
        warn!("travel_to_destination: camera transform not found");
    }

    info!(
        "Traveling from ({}, {}) to ({}, {}) with cost {}",
        start.x, start.y, final_dest.x, final_dest.y, travel_time
    );

    true
}

/// In exploring mode, keep the background synced to the nearest map tile and load its image.
pub fn update_active_tile_background(
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    placeholders: Res<crate::render3d::PlaceholderAssets>,
    mut commands: Commands,
    mut active_bgs: ResMut<ActiveMapBackgrounds>,
    mut content_cache: ResMut<TileContentCache>,
    player_q: Query<&Transform, With<Player>>,
    mut tile_spawns: Query<(Entity, &TileSpawn)>,
) {
    if game_state.0 != Game_State::Exploring {
        return;
    }

    let Ok(player_tf) = player_q.single() else {
        return;
    };

    let height = map.tiles.len() as i32;
    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;
    if width == 0 || height == 0 {
        return;
    }

    let player_tile = world_to_map_tile(player_tf.translation.truncate());
    let mut desired: HashSet<(i32, i32)> = HashSet::new();
    if player_tile.x >= 0
        && player_tile.y >= 0
        && player_tile.x < width
        && player_tile.y < height
    {
        desired.insert((player_tile.x, player_tile.y));
    }

    // Despawn backgrounds that are no longer desired.
    active_bgs.entities.retain(|pos, entity| {
        if desired.contains(&(pos.x, pos.y)) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });

    for entity in active_bgs.border_entities.drain(..) {
        commands.entity(entity).despawn();
    }

    // Despawn tile content outside the desired set.
    for (entity, spawn) in tile_spawns.iter_mut() {
        if !desired.contains(&(spawn.coords.x, spawn.coords.y)) {
            commands.entity(entity).despawn();
        }
    }

    // Spawn backgrounds (and content) for desired tiles.
    for (tx, ty) in desired.iter().copied() {
        let pos = Position { x: tx, y: ty };
        if active_bgs.entities.contains_key(&pos) {
            continue;
        }

        let Some(row) = map.tiles.get(pos.y as usize) else {
            continue;
        };
        let Some(tile) = row.get(pos.x as usize) else {
            continue;
        };

        // Placeholder ground: a flat quad in the XY plane just below z = 0 so
        // entities (at z >= 0) sit on top. Real tile textures return later via
        // the glTF pipeline. The unit quad is scaled to one tile.
        let center = tile_center_world(pos);
        let entity = commands
            .spawn((
                Mesh3d(placeholders.ground_quad.clone()),
                MeshMaterial3d(placeholders.ground_mat.clone()),
                Transform::from_translation(Vec3::new(center.x, center.y, -1.0))
                    .with_scale(Vec3::new(TILE_WORLD_SIZE, TILE_WORLD_SIZE, 1.0)),
                Name::new(format!("MapTileBackground({}, {})", pos.x, pos.y)),
            ))
            .id();

        active_bgs.entities.insert(pos, entity);

        // Spawn content (placeholder NPC/occluder/collider) for this tile if not present.
        let has_spawn = tile_spawns
            .iter()
            .any(|(_, t)| t.coords.x == pos.x && t.coords.y == pos.y);
        if !has_spawn && should_spawn_tile_content(tile, pos, &mut content_cache) {
            spawn_tile_content(&mut commands, &placeholders, pos, tile);
        }
    }

    if desired.len() == 1 {
        let pos = Position {
            x: player_tile.x,
            y: player_tile.y,
        };
        let bounds = tile_bounds_world(pos);
        let center = bounds.center();
        let size = bounds.size();
        let border_defs = [
            (
                Vec3::new(center.x, bounds.max.y - LOCAL_MAP_BORDER_THICKNESS * 0.5, -40.0),
                Vec2::new(size.x, LOCAL_MAP_BORDER_THICKNESS),
            ),
            (
                Vec3::new(center.x, bounds.min.y + LOCAL_MAP_BORDER_THICKNESS * 0.5, -40.0),
                Vec2::new(size.x, LOCAL_MAP_BORDER_THICKNESS),
            ),
            (
                Vec3::new(bounds.min.x + LOCAL_MAP_BORDER_THICKNESS * 0.5, center.y, -40.0),
                Vec2::new(LOCAL_MAP_BORDER_THICKNESS, size.y),
            ),
            (
                Vec3::new(bounds.max.x - LOCAL_MAP_BORDER_THICKNESS * 0.5, center.y, -40.0),
                Vec2::new(LOCAL_MAP_BORDER_THICKNESS, size.y),
            ),
        ];
        for (translation, border_size) in border_defs {
            // Flat strip just above the ground marking the tile boundary.
            let entity = commands
                .spawn((
                    Mesh3d(placeholders.ground_quad.clone()),
                    MeshMaterial3d(placeholders.border_mat.clone()),
                    Transform::from_translation(Vec3::new(translation.x, translation.y, 1.0))
                        .with_scale(Vec3::new(border_size.x, border_size.y, 1.0)),
                    Name::new("LocalMapBorder"),
                ))
                .id();
            active_bgs.border_entities.push(entity);
        }
    }
}

/// Spawn placeholder content for a tile: an occluder/collider marker you can extend later.
fn spawn_tile_content(
    commands: &mut Commands,
    placeholders: &crate::render3d::PlaceholderAssets,
    coords: Position,
    _tile: &MapTile,
) {
    let world_pos = tile_center_world(coords).extend(0.0);

    // Collider matching a small obstacle; adjust size/shape per tile data as needed.
    let bounds = Rect::from_center_size(world_pos.truncate(), Vec2::splat(32.0));

    // Placeholder obstacle: a 32×32 footprint cube standing 48 tall on the
    // ground (scaled shared unit cube, base at z = 0).
    commands.spawn((
        Mesh3d(placeholders.unit_cube.clone()),
        MeshMaterial3d(placeholders.obstacle_mat.clone()),
        Transform::from_translation(Vec3::new(world_pos.x, world_pos.y, 24.0))
            .with_scale(Vec3::new(32.0, 32.0, 48.0)),
        Collider { bounds },
        Occluder::new(Vec2::splat(32.0)),
        TileSpawn { coords },
        Name::new(format!("TileContent({}, {})", coords.x, coords.y)),
    ));

    // In the future, extend this to spawn NPCs/props based on `tile` metadata.
}

fn should_spawn_tile_content(
    tile: &MapTile,
    coords: Position,
    cache: &mut TileContentCache,
) -> bool {
    let state = cache.states.entry(coords).or_default();
    if state.spawned_obstacle {
        return true;
    }

    if tile.type_id == 2 {
        state.spawned_obstacle = true;
        return true;
    }

    false
}

/// Keep exploration inside the current local map, and wrap into adjacent world tiles with time cost.
pub fn handle_local_map_boundary_crossing(
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    slow_effects: Res<TerrainSlowEffectIndex>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<Timestamp>,
    mut area_changed: ResMut<Messages<AreaChanged>>,
    mut player_q: Query<&mut Transform, With<Player>>,
) {
    if game_state.0 != Game_State::Exploring {
        return;
    }

    let height = map.tiles.len() as i32;
    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;
    if width == 0 || height == 0 {
        return;
    }

    let Ok(mut player_tf) = player_q.single_mut() else {
        return;
    };

    let current_tile = map_position.0;
    let mut destination_tile = current_tile;
    let bounds = tile_bounds_world(current_tile);
    let mut next_world = player_tf.translation.truncate();
    let mut transitioned = false;

    if next_world.x < bounds.min.x {
        destination_tile.x -= 1;
        next_world.x += TILE_WORLD_SIZE;
        transitioned = true;
    } else if next_world.x > bounds.max.x {
        destination_tile.x += 1;
        next_world.x -= TILE_WORLD_SIZE;
        transitioned = true;
    }

    if next_world.y < bounds.min.y {
        destination_tile.y -= 1;
        next_world.y += TILE_WORLD_SIZE;
        transitioned = true;
    } else if next_world.y > bounds.max.y {
        destination_tile.y += 1;
        next_world.y -= TILE_WORLD_SIZE;
        transitioned = true;
    }

    if !transitioned {
        let clamped = clamp_world_inside_tile(next_world, current_tile);
        player_tf.translation.x = clamped.x;
        player_tf.translation.y = clamped.y;
        return;
    }

    if destination_tile.x < 0
        || destination_tile.y < 0
        || destination_tile.x >= width
        || destination_tile.y >= height
    {
        let clamped = clamp_world_inside_tile(next_world, current_tile);
        player_tf.translation.x = clamped.x;
        player_tf.translation.y = clamped.y;
        return;
    }

    let Some(destination_map_tile) = map
        .tiles
        .get(destination_tile.y as usize)
        .and_then(|row| row.get(destination_tile.x as usize))
    else {
        return;
    };

    timestamp.0 = timestamp.0.saturating_add(total_time_cost_for_tile_indexed(
        destination_map_tile,
        destination_tile,
        &slow_effects,
    ));

    let previous_area = current_area.0;
    current_area.0 = destination_map_tile.location_id;
    map_position.0 = destination_tile;

    let clamped = clamp_world_inside_tile(next_world, destination_tile);
    player_tf.translation.x = clamped.x;
    player_tf.translation.y = clamped.y;

    if previous_area != current_area.0 {
        area_changed.write(AreaChanged {
            from: previous_area,
            to: current_area.0,
            tile: destination_tile,
        });
    }
}

/// While exploring, update the current tile position, clamp to map bounds, and trigger tile events.
pub fn handle_tile_entry(
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut current_area: ResMut<CurrentArea>,
    mut last_tile: ResMut<LastEnteredTile>,
    mut active_event: ResMut<ActiveTileEvent>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut event_triggered: ResMut<Messages<TileEventTriggered>>,
    mut area_changed: ResMut<Messages<AreaChanged>>,
    mut before_enter: ResMut<Messages<BeforeTileEnterEvent>>,
    mut after_enter: ResMut<Messages<AfterTileEnterEvent>>,
) {
    if game_state.0 != Game_State::Exploring {
        return;
    }

    let height = map.tiles.len() as i32;
    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;
    if width == 0 || height == 0 {
        return;
    }

    let Ok(mut player_tf) = player_q.single_mut() else {
        return;
    };

    let world_tile = world_to_map_tile(player_tf.translation.truncate());
    let mut tile_x = world_tile.x;
    let mut tile_y = world_tile.y;

    tile_x = tile_x.clamp(0, width.saturating_sub(1));
    tile_y = tile_y.clamp(0, height.saturating_sub(1));

    // Don't clamp world position here; allow free exploration beyond the map bounds.

    let tile = Position { x: tile_x, y: tile_y };
    map_position.0 = tile;

    if tile != last_tile.0 {
        let from = last_tile.0;
        last_tile.0 = tile;

        // Fire Before before any state mutation listeners care about, so they
        // can read the previous area / queue cleanups based on cause.
        before_enter.write(BeforeTileEnterEvent {
            tile,
            from,
            cause: crate::combat_plugin::ActionCause::World,
        });

        if let Some(map_tile) = map
            .tiles
            .get(tile.y as usize)
            .and_then(|row| row.get(tile.x as usize))
        {
            let previous_area = current_area.0;
            current_area.0 = map_tile.location_id;
            if current_area.0 != previous_area {
                area_changed.write(AreaChanged {
                    from: previous_area,
                    to: current_area.0,
                    tile,
                });
            }
            active_event.pending = map_tile.event_ids.clone();
            active_event.current = None;
        }

        after_enter.write(AfterTileEnterEvent {
            tile,
            from,
            cause: crate::combat_plugin::ActionCause::World,
        });
    }

    if active_event.current.is_none() {
        if let Some(next_id) = active_event.pending.first().copied() {
            active_event.pending.remove(0);
            active_event.current = Some(next_id);
            event_triggered.write(TileEventTriggered { tile, event_id: next_id });
        }
    }
}

/// Clears the active tile event when completion is reported.
pub fn clear_completed_tile_events(
    mut active_event: ResMut<ActiveTileEvent>,
    mut completed: ResMut<Messages<TileEventCompleted>>,
) {
    for ev in completed.drain() {
        if active_event.current == Some(ev.event_id) {
            active_event.current = None;
        }
    }
}

/// Draw a path preview from the player's map position to the current selection while the map is open.
pub fn update_path_preview(
    mut commands: Commands,
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    map_position: Res<PlayerMapPosition>,
    selection: Res<MapSelection>,
    slow_effects: Res<TerrainSlowEffectIndex>,
    mut preview: ResMut<MapPathPreview>,
    mut path_cache: ResMut<MapTravelPathCache>,
) {
    if game_state.0 != Game_State::MapOpen {
        if !preview.entities.is_empty() {
            for e in preview.entities.drain(..) {
                commands.entity(e).despawn();
            }
            preview.last_start = None;
            preview.last_dest = None;
            preview.last_effect_revision = 0;
        }
        return;
    }

    if preview.last_start == Some(map_position.0)
        && preview.last_dest == Some(selection.0)
        && preview.last_effect_revision == slow_effects.revision
    {
        return;
    }

    for e in preview.entities.drain(..) {
        commands.entity(e).despawn();
    }

    let Some((path, _)) = cached_shortest_time_path_and_cost(
        map_position.0,
        selection.0,
        &map,
        &slow_effects,
        &mut path_cache,
    ) else {
        preview.last_start = Some(map_position.0);
        preview.last_dest = Some(selection.0);
        preview.last_effect_revision = slow_effects.revision;
        return;
    };

    let marker_size = TILE_WORLD_SIZE * 0.2;
    for &pos in path {
        let world_center = tile_center_world(pos);
        let entity = commands
            .spawn((
                Sprite {
                    color: Color::srgba(0.2, 0.7, 1.0, 0.6),
                    custom_size: Some(Vec2::splat(marker_size)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(world_center.x, world_center.y, 110.0)),
                MapPathMarker,
                Name::new(format!("MapPathMarker({}, {})", pos.x, pos.y)),
            ))
            .id();
        preview.entities.push(entity);
    }

    preview.last_start = Some(map_position.0);
    preview.last_dest = Some(selection.0);
    preview.last_effect_revision = slow_effects.revision;
}

/// Shows a minimal travel UI with selection and path cost while the map is open.
pub fn update_travel_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    map_position: Res<PlayerMapPosition>,
    selection: Res<MapSelection>,
    slow_effects: Res<TerrainSlowEffectIndex>,
    asset_server: Res<AssetServer>,
    mut ui: ResMut<MapTravelUi>,
    mut text_q: Query<&mut Text, With<MapTravelUiText>>,
    mut path_cache: ResMut<MapTravelPathCache>,
) {
    if game_state.0 != Game_State::MapOpen {
        if let Some(label) = ui.label.take() {
            commands.entity(label).despawn();
        }
        if let Some(root) = ui.root.take() {
            commands.entity(root).despawn();
        }
        return;
    }

    if ui.root.is_none() {
        let font = asset_server.load("fonts/FiraSans-Bold.ttf");
        let label = commands
            .spawn((
                Text::new(""),
                TextFont {
                    font,
                    font_size: font_size::SUBHEADING,
                    ..default()
                },
                TextColor(palette::TEXT_HEADING),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(spacing::XL),
                    top: Val::Px(spacing::XL),
                    padding: UiRect::all(Val::Px(spacing::MD)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(radius::MD)),
                    ..default()
                },
                BackgroundColor(palette::BG_PANEL),
                BorderColor::all(palette::BORDER_SUBTLE),
                MapTravelUiText,
            ))
            .id();

        let root = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                MapTravelUiRoot,
            ))
            .add_child(label)
            .id();

        ui.root = Some(root);
        ui.label = Some(label);
    }

    let (steps, cost) = cached_shortest_time_path_and_cost(
        map_position.0,
        selection.0,
        &map,
        &slow_effects,
        &mut path_cache,
    )
    .map(|(path, cost)| (path.len().saturating_sub(1), cost))
    .unwrap_or((0, 0));
    let text = format!(
        "From ({}, {}) to ({}, {})\nSteps: {}\nTime cost: {}",
        map_position.0.x,
        map_position.0.y,
        selection.0.x,
        selection.0.y,
        steps,
        cost
    );

    if let Some(label) = ui.label {
        if let Ok(mut t) = text_q.get_mut(label) {
            t.0 = text;
        }
    }
}

/// Placeholder hook for area transitions (replace with real loading logic).
pub fn handle_area_changed(
    mut events: ResMut<Messages<AreaChanged>>,
    mut log: ResMut<AreaTransitionLog>,
) {
    for ev in events.drain() {
        info!(
            "Area changed: {} -> {} at ({}, {})",
            ev.from, ev.to, ev.tile.x, ev.tile.y
        );
        log.last = Some(ev);
    }
}

/// Example handler: rolls a probability for tile events and completes them immediately.
/// Replace this with real event logic per event_id.
pub fn demo_tile_event_handler(
    mut triggered: ResMut<Messages<TileEventTriggered>>,
    mut completed: ResMut<Messages<TileEventCompleted>>,
) {
    let mut rng = rand::thread_rng();
    for ev in triggered.drain() {
        let probability = event_probability(ev.event_id);
        let roll: f32 = rng.gen();
        if roll <= probability {
            info!(
                "Tile event {} triggered at ({}, {}) (roll {:.2} <= {:.2})",
                ev.event_id, ev.tile.x, ev.tile.y, roll, probability
            );
        } else {
            info!(
                "Tile event {} skipped at ({}, {}) (roll {:.2} > {:.2})",
                ev.event_id, ev.tile.x, ev.tile.y, roll, probability
            );
        }
        completed.write(TileEventCompleted {
            event_id: ev.event_id,
        });
    }
}

fn event_probability(event_id: u32) -> f32 {
    match event_id {
        1000..=1999 => 0.25,
        2000..=2999 => 0.5,
        _ => 1.0,
    }
}
