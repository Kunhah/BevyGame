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
use bevy_camera::visibility::RenderLayers;

/// World-space size of each map tile background (square).
pub const TILE_WORLD_SIZE: f32 = 512.0;

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

/// Tracks the currently loaded area/location.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CurrentArea(pub u32);

/// Tracks spawned background entities for nearby tiles.
#[derive(Resource, Default)]
pub struct ActiveMapBackgrounds {
    pub entities: HashMap<Position, Entity>,
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
    pub location_id: u32,
    pub type_id: u8,
    pub event_ids: Vec<u32>,
    pub items_id: Option<Vec<u32>>,
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

/// Returns the time cost to enter/traverse a tile based on its type.
/// Type mapping is provisional; adjust when you formalize terrain IDs:
/// 0 = road/plain (cheap), 1 = forest, 2 = mountain, fallback = road.
pub fn time_cost_for_tile(tile: &MapTile) -> u32 {
    match tile.type_id {
        0 => 1,
        1 => 3,
        2 => 5,
        _ => 1,
    }
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
    pub from: u32,
    pub to: u32,
    pub tile: Position,
}

#[derive(Component)]
pub struct MapPathMarker;

#[derive(Resource, Default)]
pub struct MapPathPreview {
    pub entities: Vec<Entity>,
    pub last_start: Option<Position>,
    pub last_dest: Option<Position>,
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
            let location_id = (x / region_size + (y / region_size) * (width / region_size)) as u32;
            let image_path = match type_id {
                0 => "character.png",
                1 => "dot.png",
                2 => "dot.webp",
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

    let target_tile = Position {
        x: (world_pos.x / TILE_WORLD_SIZE).floor() as i32,
        y: (world_pos.y / TILE_WORLD_SIZE).floor() as i32,
    };

    if travel_to_destination(
        target_tile,
        &mut selection,
        &mut map_position,
        &mut current_area,
        &map,
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

    let mut dist: HashMap<Position, u32> = HashMap::new();
    let mut prev: HashMap<Position, Position> = HashMap::new();
    let mut heap: BinaryHeap<(Reverse<u32>, i32, i32)> = BinaryHeap::new();

    dist.insert(start, 0);
    heap.push((Reverse(0), start.x, start.y));

    while let Some((Reverse(cost), x, y)) = heap.pop() {
        let pos = Position { x, y };
        if pos == dest {
            break;
        }
        if cost > *dist.get(&pos).unwrap_or(&u32::MAX) {
            continue;
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

            let step_cost = time_cost_for_tile(tile);
            let next_cost = cost.saturating_add(step_cost);
            let next_pos = Position { x: nx, y: ny };

            if next_cost < *dist.get(&next_pos).unwrap_or(&u32::MAX) {
                dist.insert(next_pos, next_cost);
                prev.insert(next_pos, pos);
                heap.push((Reverse(next_cost), nx, ny));
            }
        }
    }

    let total_cost = *dist.get(&dest)?;
    let mut path = vec![dest];
    let mut current = dest;
    while current != start {
        let Some(&parent) = prev.get(&current) else {
            break;
        };
        current = parent;
        path.push(current);
    }
    path.reverse();

    Some((path, total_cost))
}

/// Apply travel to a destination, updating selection, time, area, and transforms.
fn travel_to_destination(
    dest: Position,
    selection: &mut MapSelection,
    map_position: &mut PlayerMapPosition,
    current_area: &mut CurrentArea,
    map: &MapTiles,
    timestamp: &mut Timestamp,
    player_q: &mut Query<&mut Transform, With<Player>>,
    camera_q: &mut Query<&mut Transform, (With<MainCamera>, Without<Player>)>,
) -> bool {
    let start = map_position.0;
    let Some((path, travel_time)) = shortest_time_path_and_cost(start, dest, map) else {
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

    let world_x = final_dest.x as f32;
    let world_y = final_dest.y as f32;
    let world_x = world_x * TILE_WORLD_SIZE;
    let world_y = world_y * TILE_WORLD_SIZE;

    if let Some(mut tf) = player_q.iter_mut().next() {
        tf.translation.x = world_x;
        tf.translation.y = world_y;
    } else {
        warn!("travel_to_destination: player transform not found");
    }

    if let Some(mut cam_tf) = camera_q.iter_mut().next() {
        cam_tf.translation.x = world_x;
        cam_tf.translation.y = world_y;
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
    asset_server: Res<AssetServer>,
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

    // Determine current tile coordinates from player position.
    let world_pos = player_tf.translation.truncate();
    let tile_x = (world_pos.x / TILE_WORLD_SIZE).floor() as i32;
    let tile_y = (world_pos.y / TILE_WORLD_SIZE).floor() as i32;

    let height = map.tiles.len() as i32;
    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;
    if width == 0 || height == 0 {
        return;
    }

    // How many tiles in each direction to keep loaded around the player.
    const RADIUS_TILES: i32 = 1;

    // Collect desired tiles in the radius (clamped to map bounds).
    let mut desired: HashSet<(i32, i32)> = HashSet::new();
    for dy in -RADIUS_TILES..=RADIUS_TILES {
        for dx in -RADIUS_TILES..=RADIUS_TILES {
            let tx = (tile_x + dx).clamp(0, width.saturating_sub(1));
            let ty = (tile_y + dy).clamp(0, height.saturating_sub(1));
            desired.insert((tx, ty));
        }
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

        let texture = asset_server.load(tile.image_path.clone());
        let entity = commands
            .spawn((
                Sprite {
                    image: texture,
                    custom_size: Some(Vec2::splat(TILE_WORLD_SIZE)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(
                    pos.x as f32 * TILE_WORLD_SIZE,
                    pos.y as f32 * TILE_WORLD_SIZE,
                    -50.0,
                )),
                Name::new(format!("MapTileBackground({}, {})", pos.x, pos.y)),
            ))
            .id();

        active_bgs.entities.insert(pos, entity);

        // Spawn content (placeholder NPC/occluder/collider) for this tile if not present.
        let has_spawn = tile_spawns
            .iter()
            .any(|(_, t)| t.coords.x == pos.x && t.coords.y == pos.y);
        if !has_spawn && should_spawn_tile_content(tile, pos, &mut content_cache) {
            spawn_tile_content(&mut commands, pos, tile);
        }
    }
}

/// Spawn placeholder content for a tile: an occluder/collider marker you can extend later.
fn spawn_tile_content(commands: &mut Commands, coords: Position, tile: &MapTile) {
    let world_pos = Vec3::new(
        coords.x as f32 * TILE_WORLD_SIZE,
        coords.y as f32 * TILE_WORLD_SIZE,
        0.0,
    );

    // Collider matching a small obstacle; adjust size/shape per tile data as needed.
    let bounds = Rect::from_center_size(world_pos.truncate(), Vec2::splat(32.0));

    commands.spawn((
        Sprite {
            color: Color::srgba(0.8, 0.1, 0.1, 0.4),
            custom_size: Some(Vec2::splat(32.0)),
            ..default()
        },
        Transform::from_translation(world_pos + Vec3::new(0.0, 0.0, 10.0)),
        Collider { bounds },
        Occluder,
        RenderLayers::layer(0),
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

    let mut tile_x = (player_tf.translation.x / TILE_WORLD_SIZE).floor() as i32;
    let mut tile_y = (player_tf.translation.y / TILE_WORLD_SIZE).floor() as i32;

    tile_x = tile_x.clamp(0, width.saturating_sub(1));
    tile_y = tile_y.clamp(0, height.saturating_sub(1));

    // Don't clamp world position here; allow free exploration beyond the map bounds.

    let tile = Position { x: tile_x, y: tile_y };
    map_position.0 = tile;

    if tile != last_tile.0 {
        last_tile.0 = tile;

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
    mut preview: ResMut<MapPathPreview>,
) {
    if game_state.0 != Game_State::MapOpen {
        if !preview.entities.is_empty() {
            for e in preview.entities.drain(..) {
                commands.entity(e).despawn();
            }
            preview.last_start = None;
            preview.last_dest = None;
        }
        return;
    }

    if preview.last_start == Some(map_position.0) && preview.last_dest == Some(selection.0) {
        return;
    }

    for e in preview.entities.drain(..) {
        commands.entity(e).despawn();
    }

    let Some((path, _)) = shortest_time_path_and_cost(map_position.0, selection.0, &map) else {
        preview.last_start = Some(map_position.0);
        preview.last_dest = Some(selection.0);
        return;
    };

    let marker_size = TILE_WORLD_SIZE * 0.2;
    for pos in path {
        let world_x = pos.x as f32 * TILE_WORLD_SIZE;
        let world_y = pos.y as f32 * TILE_WORLD_SIZE;
        let entity = commands
            .spawn((
                Sprite {
                    color: Color::srgba(0.2, 0.7, 1.0, 0.6),
                    custom_size: Some(Vec2::splat(marker_size)),
                    ..default()
                },
                Transform::from_translation(Vec3::new(world_x, world_y, 110.0)),
                MapPathMarker,
                Name::new(format!("MapPathMarker({}, {})", pos.x, pos.y)),
            ))
            .id();
        preview.entities.push(entity);
    }

    preview.last_start = Some(map_position.0);
    preview.last_dest = Some(selection.0);
}

/// Shows a minimal travel UI with selection and path cost while the map is open.
pub fn update_travel_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    map: Res<MapTiles>,
    map_position: Res<PlayerMapPosition>,
    selection: Res<MapSelection>,
    asset_server: Res<AssetServer>,
    mut ui: ResMut<MapTravelUi>,
    mut text_q: Query<&mut Text, With<MapTravelUiText>>,
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
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.95, 1.0)),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(24.0),
                    top: Val::Px(24.0),
                    ..default()
                },
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

    let (path, cost) = shortest_time_path_and_cost(map_position.0, selection.0, &map)
        .map(|(p, c)| (p, c))
        .unwrap_or((Vec::new(), 0));
    let steps = path.len().saturating_sub(1);
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
