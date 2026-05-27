use std::collections::HashMap;

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::city_data::{City, CityAuthorityState, CityCatalog};
use crate::core::{GameState, Game_State, Player};
use crate::economy::{PlayerWallet, TradeLogEvent};
use crate::governance::{
    CastleAssaultClock, GlobalPunishmentState, PlayerCrimeStatus, ReputationLedger, WantedTier,
};
use crate::constants::TIMESTAMP_TICKS_PER_HOUR;
use crate::map::{tile_center_world, CurrentArea, MapTiles, TILE_WORLD_SIZE};
use crate::ui_style::{font_size, palette, radius, spacing};

const SERVICE_INTERACT_DISTANCE: f32 = 96.0;
const INN_REST_COST: u32 = 280;
const INN_REST_HOURS: u32 = 8;
type RegionId = u16;
type CityId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    Inn,
    Transport,
    CraftingHall,
    CastleGate,
}

impl ServiceKind {
    pub fn label(self) -> &'static str {
        match self {
            ServiceKind::Inn => "inn",
            ServiceKind::Transport => "transport",
            ServiceKind::CraftingHall => "crafting_hall",
            ServiceKind::CastleGate => "castle_gate",
        }
    }

    pub fn hotkey(self) -> KeyCode {
        match self {
            ServiceKind::Inn => KeyCode::KeyC,
            ServiceKind::Transport => KeyCode::KeyV,
            ServiceKind::CraftingHall => KeyCode::KeyB,
            ServiceKind::CastleGate => KeyCode::KeyN,
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct ServiceNpc {
    pub city_id: CityId,
    pub region_id: RegionId,
    pub kind: ServiceKind,
}

#[derive(Debug, Clone)]
struct TransportRoute {
    destination_city_id: CityId,
    destination_city_name: String,
    destination_region_id: RegionId,
    distance_units: u16,
    cost_coins: u32,
    travel_ticks: u32,
}

#[derive(Resource, Debug, Clone, Default)]
struct TransportUiState {
    open: bool,
    root: Option<Entity>,
    selected: usize,
    source_city_id: Option<CityId>,
    routes: Vec<TransportRoute>,
}

#[derive(Component)]
struct TransportUiRoot;

pub struct ServicesPlugin;

impl Plugin for ServicesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TransportUiState>()
            .add_systems(Update, service_interaction_input)
            .add_systems(Update, ensure_transport_ui_root)
            .add_systems(Update, update_transport_ui_text)
            .add_systems(Update, handle_transport_ui_input);
    }
}

fn city_for_region(cities: &CityCatalog, region_id: RegionId) -> Option<&City> {
    cities
        .0
        .values()
        .find(|city| city.region_ids.contains(&region_id))
}

fn city_for_region_mut(cities: &mut CityCatalog, region_id: RegionId) -> Option<&mut City> {
    let city_id = cities
        .0
        .values()
        .find(|city| city.region_ids.contains(&region_id))
        .map(|city| city.id)?;
    cities.0.get_mut(&city_id)
}

fn region_centroid_from_map(map: &MapTiles, region_id: RegionId) -> Option<Vec2> {
    let mut count: u32 = 0;
    let mut sx = 0.0_f32;
    let mut sy = 0.0_f32;
    for (y, row) in map.tiles.iter().enumerate() {
        for (x, tile) in row.iter().enumerate() {
            if tile.location_id == region_id {
                let center = tile_center_world(crate::core::Position {
                    x: x as i32,
                    y: y as i32,
                });
                sx += center.x;
                sy += center.y;
                count = count.saturating_add(1);
            }
        }
    }
    if count == 0 {
        return None;
    }
    Some(Vec2::new(sx / count as f32, sy / count as f32))
}

fn city_centroid_from_map(map: &MapTiles, city: &City) -> Option<Vec2> {
    let mut count = 0_u32;
    let mut sx = 0.0_f32;
    let mut sy = 0.0_f32;
    for &region_id in &city.region_ids {
        if let Some(c) = region_centroid_from_map(map, region_id) {
            sx += c.x;
            sy += c.y;
            count = count.saturating_add(1);
        }
    }
    if count == 0 {
        return None;
    }
    Some(Vec2::new(sx / count as f32, sy / count as f32))
}

fn city_centroids(cities: &CityCatalog, map: &MapTiles) -> HashMap<CityId, Vec2> {
    let mut out = HashMap::new();
    for city in cities.0.values() {
        if let Some(c) = city_centroid_from_map(map, city) {
            out.insert(city.id, c);
        }
    }
    out
}

fn import_weight_by_distance(world_distance: f32) -> f32 {
    let tiles = world_distance / TILE_WORLD_SIZE.max(1.0);
    1.0 / (1.0 + (tiles / 2.0).powi(2))
}

fn nearest_city_weights_for_region(
    region_id: RegionId,
    cities: &CityCatalog,
    map: &MapTiles,
    centers: &HashMap<CityId, Vec2>,
    max_count: usize,
) -> Vec<(CityId, f32)> {
    if let Some(city) = city_for_region(cities, region_id) {
        return vec![(city.id, 1.0)];
    }
    let Some(region_center) = region_centroid_from_map(map, region_id) else {
        return Vec::new();
    };
    let mut weighted: Vec<(CityId, f32)> = centers
        .iter()
        .map(|(city_id, center)| {
            (
                *city_id,
                import_weight_by_distance(region_center.distance(*center)),
            )
        })
        .filter(|(_, w)| *w > 0.0001)
        .collect();
    weighted.sort_by(|a, b| b.1.total_cmp(&a.1));
    weighted.truncate(max_count.max(1));
    let sum: f32 = weighted.iter().map(|(_, w)| *w).sum();
    if sum <= 0.0 {
        return Vec::new();
    }
    weighted.into_iter().map(|(id, w)| (id, w / sum)).collect()
}

fn service_lock_reason_for_region(
    region_id: RegionId,
    cities: &CityCatalog,
    reputation: &ReputationLedger,
    crime: &PlayerCrimeStatus,
    map: &MapTiles,
) -> Option<String> {
    let centers = city_centroids(cities, map);
    let weights = nearest_city_weights_for_region(region_id, cities, map, &centers, 3);
    if weights.is_empty() {
        return None;
    }

    let mut governor_rep = 0.0_f32;
    let mut clan_rep = 0.0_f32;
    for (city_id, weight) in weights {
        let Some(city) = cities.0.get(&city_id) else {
            continue;
        };
        governor_rep += reputation.get_governor(city.id) as f32 * weight;
        clan_rep += reputation.get_clan(&city.clan_name) as f32 * weight;
    }
    let governor_rep = governor_rep.round() as i32;
    let clan_rep = clan_rep.round() as i32;

    match crime.wanted_tier {
        WantedTier::RealmThreat => Some("realm threat status".to_string()),
        WantedTier::Outlaw if governor_rep <= -30 || clan_rep <= -35 => Some(format!(
            "outlaw with hostile local rule (gov_rep={} clan_rep={})",
            governor_rep, clan_rep
        )),
        WantedTier::Suspect if governor_rep <= -75 && clan_rep <= -80 => Some(format!(
            "suspect with severe local hostility (gov_rep={} clan_rep={})",
            governor_rep, clan_rep
        )),
        _ => None,
    }
}

fn nearest_service_of_kind(
    player_pos: Vec2,
    service_kind: ServiceKind,
    query: &Query<(&Transform, &ServiceNpc)>,
) -> Option<ServiceNpc> {
    let mut nearest: Option<(ServiceNpc, f32)> = None;
    for (tf, svc) in query.iter() {
        if svc.kind != service_kind {
            continue;
        }
        let d = player_pos.distance(tf.translation.truncate());
        if d > SERVICE_INTERACT_DISTANCE {
            continue;
        }
        let better = nearest.map(|(_, dist)| d < dist).unwrap_or(true);
        if better {
            nearest = Some((*svc, d));
        }
    }
    nearest.map(|(svc, _)| svc)
}

fn wanted_tier_from_infamy(infamy: u32, assassinations_total: u32) -> WantedTier {
    if assassinations_total >= 3 && infamy >= 420 {
        return WantedTier::RealmThreat;
    }
    match infamy {
        0..=149 => WantedTier::None,
        150..=399 => WantedTier::Suspect,
        400..=899 => WantedTier::Outlaw,
        _ => WantedTier::RealmThreat,
    }
}

fn first_tile_for_region(map: &MapTiles, region_id: RegionId) -> Option<(i32, i32)> {
    for (y, row) in map.tiles.iter().enumerate() {
        for (x, tile) in row.iter().enumerate() {
            if tile.location_id == region_id {
                return Some((x as i32, y as i32));
            }
        }
    }
    None
}

fn route_cost_from_distance(distance_units: u16, from: &City, to: &City) -> u32 {
    let base = 180_u32
        .saturating_add(u32::from(distance_units) * 4)
        .saturating_add(u32::from(from.market_fee_bps) / 3)
        .saturating_add(u32::from(to.market_fee_bps) / 4)
        .saturating_add(u32::from(from.tax_rate_bps) / 8);
    match to.authority.state {
        CityAuthorityState::CollapsedAuthority => base.saturating_add(500),
        CityAuthorityState::Interregnum => base.saturating_add(300),
        CityAuthorityState::MartialLaw => base.saturating_add(180),
        _ => base,
    }
}

fn route_ticks_from_distance(distance_units: u16) -> u32 {
    let hours = (u32::from(distance_units) / 70).max(2);
    hours.saturating_mul(TIMESTAMP_TICKS_PER_HOUR)
}

fn build_transport_routes(source_city: &City, cities: &CityCatalog) -> Vec<TransportRoute> {
    let mut routes: Vec<TransportRoute> = Vec::new();
    let mut target_ids = source_city.trade_route_city_ids.clone();
    target_ids.sort_unstable();
    target_ids.dedup();

    for target_id in target_ids {
        let Some(target_city) = cities.0.get(&target_id) else {
            continue;
        };
        let distance_units = source_city
            .trade_distance_by_city_id
            .get(&target_id)
            .copied()
            .or_else(|| target_city.trade_distance_by_city_id.get(&source_city.id).copied())
            .unwrap_or(260);
        let Some(&dest_region) = target_city.region_ids.first() else {
            continue;
        };
        routes.push(TransportRoute {
            destination_city_id: target_city.id,
            destination_city_name: target_city.name.clone(),
            destination_region_id: dest_region,
            distance_units,
            cost_coins: route_cost_from_distance(distance_units, source_city, target_city),
            travel_ticks: route_ticks_from_distance(distance_units),
        });
    }
    routes.sort_by(|a, b| a.cost_coins.cmp(&b.cost_coins));
    routes
}

fn service_interaction_input(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    current_area: Res<CurrentArea>,
    map: Res<MapTiles>,
    mut cities: ResMut<CityCatalog>,
    reputation: Res<ReputationLedger>,
    mut crime: ResMut<PlayerCrimeStatus>,
    punish: Res<GlobalPunishmentState>,
    mut wallet: ResMut<PlayerWallet>,
    mut assault_clock: ResMut<CastleAssaultClock>,
    mut timestamp: ResMut<crate::core::Timestamp>,
    player_q: Query<&Transform, With<Player>>,
    service_q: Query<(&Transform, &ServiceNpc)>,
    mut transport_ui: ResMut<TransportUiState>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Exploring || transport_ui.open {
        return;
    }

    let mut requested_kind = None;
    for kind in [
        ServiceKind::Inn,
        ServiceKind::Transport,
        ServiceKind::CraftingHall,
        ServiceKind::CastleGate,
    ] {
        if input.just_pressed(kind.hotkey()) {
            requested_kind = Some(kind);
            break;
        }
    }
    let Some(service_kind) = requested_kind else {
        return;
    };

    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let Some(service) = nearest_service_of_kind(
        player_tf.translation.truncate(),
        service_kind,
        &service_q,
    )
    else {
        logs.write(TradeLogEvent {
            message: format!(
                "service {} unavailable: no NPC within {:.0} units",
                service_kind.label(),
                SERVICE_INTERACT_DISTANCE
            ),
        });
        return;
    };

    if let Some(reason) = service_lock_reason_for_region(
        service.region_id,
        &cities,
        &reputation,
        &crime,
        &map,
    ) {
        logs.write(TradeLogEvent {
            message: format!(
                "service {} blocked in city {}: {}",
                service_kind.label(),
                service.city_id,
                reason
            ),
        });
        return;
    }

    let Some(city) = cities.0.get(&service.city_id).cloned() else {
        return;
    };

    match service_kind {
        ServiceKind::Inn => {
            if wallet.coins < INN_REST_COST {
                logs.write(TradeLogEvent {
                    message: format!("inn denied: need {} coins", INN_REST_COST),
                });
                return;
            }
            wallet.coins = wallet.coins.saturating_sub(INN_REST_COST);
            timestamp.0 = timestamp
                .0
                .saturating_add(INN_REST_HOURS * TIMESTAMP_TICKS_PER_HOUR);
            crime.infamy = crime.infamy.saturating_sub(16);
            crime.wanted_tier = wanted_tier_from_infamy(crime.infamy, punish.assassinations_total);
            assault_clock.next_allowed_timestamp = assault_clock
                .next_allowed_timestamp
                .saturating_sub(2 * TIMESTAMP_TICKS_PER_HOUR);
            if let Some(local_city) = city_for_region_mut(&mut cities, service.region_id) {
                local_city.stability = local_city.stability.saturating_add(8).min(1000);
                local_city.security = local_city.security.saturating_add(6).min(1000);
                local_city.prosperity = local_city.prosperity.saturating_add(3).min(1000);
            }
            logs.write(TradeLogEvent {
                message: format!(
                    "inn stay accepted [{}]: cost={} coins, rested {}h, infamy={} wanted={:?}",
                    city.name, INN_REST_COST, INN_REST_HOURS, crime.infamy, crime.wanted_tier
                ),
            });
        }
        ServiceKind::Transport => {
            let Some(source_city) = cities.0.get(&service.city_id) else {
                return;
            };
            let routes = build_transport_routes(source_city, &cities);
            if routes.is_empty() {
                logs.write(TradeLogEvent {
                    message: format!("transport unavailable [{}]: no active routes", city.name),
                });
                return;
            }
            transport_ui.routes = routes;
            transport_ui.source_city_id = Some(source_city.id);
            transport_ui.selected = 0;
            transport_ui.open = true;
            logs.write(TradeLogEvent {
                message: format!(
                    "transport desk opened [{}]: choose route with W/S, ENTER to buy, ESC to close",
                    city.name
                ),
            });
        }
        ServiceKind::CraftingHall => {
            logs.write(TradeLogEvent {
                message: format!(
                    "crafting hall accessed [{}]: artisans ready (placeholder service)",
                    city.name
                ),
            });
        }
        ServiceKind::CastleGate => {
            if matches!(
                city.authority.state,
                CityAuthorityState::CollapsedAuthority | CityAuthorityState::Interregnum
            ) || city.authority.under_siege
            {
                logs.write(TradeLogEvent {
                    message: format!(
                        "castle gate denied [{}]: city status {} / siege={}",
                        city.name,
                        city.authority.state.label(),
                        city.authority.under_siege
                    ),
                });
                return;
            }
            logs.write(TradeLogEvent {
                message: format!(
                    "castle gate entry granted [{}] by guards of {}",
                    city.name, city.clan_name
                ),
            });
        }
    }

    logs.write(TradeLogEvent {
        message: format!(
            "service_hint: C=inn V=transport B=crafting_hall N=castle_gate | current_region={}",
            current_area.0
        ),
    });
}

fn ensure_transport_ui_root(
    mut commands: Commands,
    mut state: ResMut<TransportUiState>,
    roots: Query<Entity, With<TransportUiRoot>>,
) {
    if !state.open {
        if let Some(entity) = state.root.take() {
            commands.entity(entity).despawn();
        }
        return;
    }
    if let Some(root) = state.root {
        if roots.get(root).is_ok() {
            return;
        }
        state.root = None;
    }
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(8.0),
                right: Val::Percent(8.0),
                top: Val::Percent(10.0),
                bottom: Val::Percent(10.0),
                padding: UiRect::all(Val::Px(spacing::LG)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::LG)),
                ..default()
            },
            BackgroundColor(palette::BG_PANEL),
            BorderColor::all(palette::BORDER_ACCENT),
            Text::new("Transport UI"),
            TextFont {
                font_size: font_size::BODY_LG,
                ..default()
            },
            TextColor(palette::TEXT_PRIMARY),
            TransportUiRoot,
        ))
        .id();
    state.root = Some(root);
}

fn update_transport_ui_text(
    state: Res<TransportUiState>,
    wallet: Res<PlayerWallet>,
    mut roots: Query<&mut Text, With<TransportUiRoot>>,
) {
    if !state.open {
        return;
    }
    let Some(root) = state.root else {
        return;
    };
    let Ok(mut text) = roots.get_mut(root) else {
        return;
    };
    let mut out = String::new();
    out.push_str("=== TRANSPORT ROUTES === (ESC close)\n");
    out.push_str("Controls: W/S select route, ENTER purchase travel\n");
    out.push_str(&format!("Player coins: {}\n", wallet.coins));
    if state.routes.is_empty() {
        out.push_str("No routes available.\n");
    } else {
        for (idx, route) in state.routes.iter().enumerate() {
            let marker = if idx == state.selected { ">" } else { " " };
            out.push_str(&format!(
                "{} {} | cost {} | travel {}h | dist {}\n",
                marker,
                route.destination_city_name,
                route.cost_coins,
                route.travel_ticks / TIMESTAMP_TICKS_PER_HOUR,
                route.distance_units
            ));
        }
    }
    text.0 = out;
}

fn handle_transport_ui_input(
    input: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<TransportUiState>,
    mut wallet: ResMut<PlayerWallet>,
    mut game_state: ResMut<GameState>,
    map: Res<MapTiles>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<crate::core::Timestamp>,
    mut player_map_pos: ResMut<crate::core::PlayerMapPosition>,
    mut map_selection: ResMut<crate::map::MapSelection>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_q: Query<&mut Transform, (With<crate::core::MainCamera>, Without<Player>)>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if !state.open {
        return;
    }

    if input.just_pressed(KeyCode::Escape) {
        state.open = false;
        return;
    }
    if state.routes.is_empty() {
        return;
    }

    if input.just_pressed(KeyCode::KeyS) || input.just_pressed(KeyCode::ArrowDown) {
        state.selected = (state.selected + 1) % state.routes.len();
    }
    if input.just_pressed(KeyCode::KeyW) || input.just_pressed(KeyCode::ArrowUp) {
        state.selected = if state.selected == 0 {
            state.routes.len() - 1
        } else {
            state.selected - 1
        };
    }
    if !input.just_pressed(KeyCode::Enter) && !input.just_pressed(KeyCode::Space) {
        return;
    }

    let route = state.routes[state.selected].clone();
    if wallet.coins < route.cost_coins {
        logs.write(TradeLogEvent {
            message: format!(
                "transport purchase failed: need {} coins, have {}",
                route.cost_coins, wallet.coins
            ),
        });
        return;
    }
    let Some((tile_x, tile_y)) = first_tile_for_region(&map, route.destination_region_id) else {
        logs.write(TradeLogEvent {
            message: format!(
                "transport purchase failed: destination region {} has no tile",
                route.destination_region_id
            ),
        });
        return;
    };

    wallet.coins = wallet.coins.saturating_sub(route.cost_coins);
    timestamp.0 = timestamp.0.saturating_add(route.travel_ticks.max(1));
    current_area.0 = route.destination_region_id;
    player_map_pos.0 = crate::core::Position { x: tile_x, y: tile_y };
    map_selection.0 = player_map_pos.0;

    let world_center = tile_center_world(crate::core::Position { x: tile_x, y: tile_y });
    if let Ok(mut tf) = player_q.single_mut() {
        tf.translation.x = world_center.x;
        tf.translation.y = world_center.y;
    }
    if let Ok(mut cam_tf) = camera_q.single_mut() {
        cam_tf.translation = world_center.extend(0.0) + crate::render3d::iso_camera_offset();
    }

    state.open = false;
    game_state.0 = Game_State::Exploring;
    logs.write(TradeLogEvent {
        message: format!(
            "transport purchased -> city {} (city_id {}) cost={} travel_ticks={}",
            route.destination_city_name,
            route.destination_city_id,
            route.cost_coins,
            route.travel_ticks
        ),
    });
}
