use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::input::{keyboard::KeyCode, mouse::MouseButton};
use bevy::prelude::*;

use crate::core::{GameState, Game_State, MainCamera, Player, PlayerMapPosition, Position, Timestamp};
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

#[derive(Resource, Clone)]
pub struct MapTiles {
    pub tiles: Vec<Vec<MapTile>>,
}

/// Tracks the currently loaded area/location.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CurrentArea(pub u32);

/// Tracks the currently spawned background entity for the active tile.
#[derive(Resource, Default)]
pub struct ActiveMapBackground {
    pub entity: Option<Entity>,
    pub coords: Position,
}

/// Marks spawned content for a specific tile so it can be culled when far away.
#[derive(Component, Clone, Copy, Debug)]
pub struct TileSpawn {
    pub coords: Position,
}

#[derive(Clone)]
pub struct MapTile {
    pub time: u32,
    pub location_id: u32,
    pub type_id: u8,
    pub event_id: Option<u32>,
    pub items_id: Option<Vec<u32>>,
    pub image_path: String, // The path will be named with the coordinates of each tile, e.g., "map_tiles/tile_0_0.png". This way I can make a script to auto-generate the map tiles later.
}

impl Default for MapTile {
    fn default() -> Self {
        Self {
            time: 1,
            location_id: 0,
            type_id: 0,
            event_id: None,
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

/// Generates a simple flat map to enable travel mode.
pub fn generate_map_tiles() -> MapTiles {
    // Use GRID_WIDTH/HEIGHT as coarse bounds; keep it manageable.
    let width = 64;
    let height = 64;
    let row = vec![MapTile::default(); width];
    let tiles = (0..height).map(|_| row.clone()).collect();
    MapTiles { tiles }
}

/// Toggle entering the map (travel mode) with `M` when exploring.
pub fn toggle_map_mode(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    map_position: Res<PlayerMapPosition>,
    mut selection: ResMut<MapSelection>,
) {
    if !input.just_pressed(KeyCode::KeyM) {
        return;
    }

    game_state.0 = match game_state.0 {
        Game_State::Exploring => {
            // When entering travel mode, start the cursor at the player's current map position.
            selection.0 = map_position.0;
            info!(
                "Entering travel map mode at selection ({}, {})",
                selection.0.x, selection.0.y
            );
            Game_State::Traveling
        }
        Game_State::Traveling => {
            info!("Exiting travel map mode, returning to exploring");
            Game_State::Exploring
        }
        other => other,
    };
}

/// Move the map selection tile-by-tile with WASD/arrow keys when in travel mode.
pub fn navigate_map_selection(
    input: Res<ButtonInput<KeyCode>>,
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
    if game_state.0 != Game_State::Traveling {
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

    if delta == IVec2::ZERO {
        return;
    }

    let height = map.tiles.len() as i32;
    let width = map.tiles.get(0).map(|r| r.len()).unwrap_or(0) as i32;

    let new_x = (selection.0.x + delta.x).clamp(0, width.saturating_sub(1));
    let new_y = (selection.0.y + delta.y).clamp(0, height.saturating_sub(1));
    selection.0.x = new_x;
    selection.0.y = new_y;

    // If no directional input, allow clicking a tile to travel using the lowest-time path.
    if delta != IVec2::ZERO {
        return;
    }

    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(window) = windows.iter().next() else {
        warn!("navigate_map_selection: could not get primary window for click");
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Some((camera, cam_tf)) = camera_q.iter().next() else {
        warn!("navigate_map_selection: missing main camera for click handling");
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
    if game_state.0 != Game_State::Traveling {
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
    mut active_bg: ResMut<ActiveMapBackground>,
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
    if let Some(e) = active_bg.entity.take() {
        // If the stored background is outside the desired set, despawn it.
        if !desired.contains(&(active_bg.coords.x, active_bg.coords.y)) {
            commands.entity(e).despawn();
        } else {
            // Keep it and restore tracking.
            active_bg.entity = Some(e);
        }
    }

    // Despawn tile content outside the desired set.
    for (entity, spawn) in tile_spawns.iter_mut() {
        if !desired.contains(&(spawn.coords.x, spawn.coords.y)) {
            commands.entity(entity).despawn();
        }
    }

    // If we already have the center tile loaded and tracked, nothing to do.
    let center = Position {
        x: tile_x.clamp(0, width.saturating_sub(1)),
        y: tile_y.clamp(0, height.saturating_sub(1)),
    };
    if active_bg
        .entity
        .is_some_and(|_| active_bg.coords.x == center.x && active_bg.coords.y == center.y)
    {
        return;
    }

    // Spawn/replace the center tile background.
    if let Some(row) = map.tiles.get(center.y as usize) {
        if let Some(tile) = row.get(center.x as usize) {
            let texture = asset_server.load(tile.image_path.clone());
            let entity = commands
                .spawn((
                    Sprite {
                        image: texture,
                        custom_size: Some(Vec2::splat(TILE_WORLD_SIZE)),
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(
                        center.x as f32 * TILE_WORLD_SIZE,
                        center.y as f32 * TILE_WORLD_SIZE,
                        -50.0,
                    )),
                    Name::new(format!("MapTileBackground({}, {})", center.x, center.y)),
                ))
                .id();

            active_bg.entity = Some(entity);
            active_bg.coords = center;
            info!(
                "Loaded map tile background at ({}, {}), image '{}'",
                center.x, center.y, tile.image_path
            );

            // Spawn content (placeholder NPC/occluder/collider) for this tile if not present.
            let has_spawn = tile_spawns
                .iter()
                .any(|(_, t)| t.coords.x == center.x && t.coords.y == center.y);
            if !has_spawn {
                spawn_tile_content(&mut commands, center, tile);
            }
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
        // Visible to main camera and occlusion camera.
        RenderLayers::from_layers(&[0, 1]),
        TileSpawn { coords },
        Name::new(format!("TileContent({}, {})", coords.x, coords.y)),
    ));

    // In the future, extend this to spawn NPCs/props based on `tile` metadata.
}
