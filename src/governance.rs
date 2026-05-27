use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::battle::{BattleSide, EnemyEncounter};
use crate::city_data::{
    City, CityAuthorityState, CityCatalog, ItemMarketEffect, MaterialMarketEffect, SuccessionSource,
};
use crate::combat_plugin::{DeathEvent, ItemMaterial, PlayerControlled};
use crate::constants::TIMESTAMP_TICKS_PER_HOUR;
use crate::core::{GameState, Game_State, Player, Timestamp};
use crate::dialogue::Interactable;
use crate::economy::{MerchantNpc, Merchants, PlayerInventory, PlayerWallet, TradeLogEvent};
use crate::light_plugin::LightSensitive;
use crate::map::{tile_center_world, CurrentArea, MapTiles, TILE_WORLD_SIZE};

const REPUTATION_MIN: i32 = -100;
const REPUTATION_MAX: i32 = 100;
const SHOP_OPEN_DISTANCE: f32 = 96.0;
const CASTLE_ASSAULT_COOLDOWN_HOURS: u32 = 4;
const CASTLE_ASSAULT_COOLDOWN_TIMESTAMP: u32 =
    TIMESTAMP_TICKS_PER_HOUR * CASTLE_ASSAULT_COOLDOWN_HOURS;
const CASTLE_ASSAULT_DISTANCE: f32 = 160.0;
const LEADERSHIP_IMMUNITY_DAYS: u32 = 4;
const LEADERSHIP_IMMUNITY_WINDOW_TIMESTAMP: u32 =
    LEADERSHIP_IMMUNITY_DAYS * 24 * TIMESTAMP_TICKS_PER_HOUR;
const GOVERNOR_POLICY_INTERVAL_HOURS: u32 = 8;
const GOVERNOR_POLICY_INTERVAL_TIMESTAMP: u32 =
    GOVERNOR_POLICY_INTERVAL_HOURS * TIMESTAMP_TICKS_PER_HOUR;
const COUP_STEP_INTEL_COST: u32 = 0;
const COUP_STEP_BRIBE_COST: u32 = 3_200;
const COUP_STEP_SABOTAGE_COST: u32 = 2_400;

pub type RegionId = u16;
pub type MerchantId = u16;
pub type CityId = u16;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WantedTier {
    None,
    Suspect,
    Outlaw,
    RealmThreat,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct PlayerCrimeStatus {
    pub infamy: u32,
    pub wanted_tier: WantedTier,
    pub bounties_by_clan: HashMap<String, u32>,
}

impl Default for PlayerCrimeStatus {
    fn default() -> Self {
        Self {
            infamy: 0,
            wanted_tier: WantedTier::None,
            bounties_by_clan: HashMap::new(),
        }
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct GlobalPunishmentState {
    pub assassinations_total: u32,
}

impl Default for GlobalPunishmentState {
    fn default() -> Self {
        Self {
            assassinations_total: 0,
        }
    }
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoupChainState {
    pub prepared_by_city: HashSet<CityId>,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceClock {
    pub last_timestamp: u32,
}

impl Default for GovernanceClock {
    fn default() -> Self {
        Self { last_timestamp: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReputationTarget {
    Governor { city_id: CityId },
    Merchant { merchant_id: MerchantId },
    Clan { clan_name: String },
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReputationLedger {
    pub governor_by_city: HashMap<CityId, i32>,
    pub merchant_by_id: HashMap<MerchantId, i32>,
    pub clan_by_name: HashMap<String, i32>,
}

impl ReputationLedger {
    pub fn apply_delta(&mut self, target: &ReputationTarget, delta: i16) -> i32 {
        let delta_i32 = i32::from(delta);
        match target {
            ReputationTarget::Governor { city_id } => {
                let entry = self.governor_by_city.entry(*city_id).or_insert(0);
                *entry = (*entry + delta_i32).clamp(REPUTATION_MIN, REPUTATION_MAX);
                *entry
            }
            ReputationTarget::Merchant { merchant_id } => {
                let entry = self.merchant_by_id.entry(*merchant_id).or_insert(0);
                *entry = (*entry + delta_i32).clamp(REPUTATION_MIN, REPUTATION_MAX);
                *entry
            }
            ReputationTarget::Clan { clan_name } => {
                let entry = self.clan_by_name.entry(clan_name.clone()).or_insert(0);
                *entry = (*entry + delta_i32).clamp(REPUTATION_MIN, REPUTATION_MAX);
                *entry
            }
        }
    }

    pub fn get_governor(&self, city_id: CityId) -> i32 {
        self.governor_by_city
            .get(&city_id)
            .copied()
            .unwrap_or(0)
            .clamp(REPUTATION_MIN, REPUTATION_MAX)
    }

    pub fn get_merchant(&self, merchant_id: MerchantId) -> i32 {
        self.merchant_by_id
            .get(&merchant_id)
            .copied()
            .unwrap_or(0)
            .clamp(REPUTATION_MIN, REPUTATION_MAX)
    }

    pub fn get_clan(&self, clan_name: &str) -> i32 {
        self.clan_by_name
            .get(clan_name)
            .copied()
            .unwrap_or(0)
            .clamp(REPUTATION_MIN, REPUTATION_MAX)
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct CastleAssaultClock {
    pub next_allowed_timestamp: u32,
}

impl Default for CastleAssaultClock {
    fn default() -> Self {
        Self {
            next_allowed_timestamp: 0,
        }
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct GovernorPolicyClock {
    pub last_timestamp: u32,
}

impl Default for GovernorPolicyClock {
    fn default() -> Self {
        Self { last_timestamp: 0 }
    }
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoupPreparationProgress {
    pub stage_by_city: HashMap<CityId, u8>,
}

#[derive(Debug, Clone, Message)]
pub struct ReputationChangeEvent {
    pub target: ReputationTarget,
    pub delta: i16,
    pub reason: String,
}

#[derive(Debug, Clone, Message)]
pub struct ReputationIncidentEvent {
    pub kind: ReputationIncidentKind,
}

#[derive(Debug, Clone)]
pub enum ReputationIncidentKind {
    MerchantStoreRobbed {
        merchant_id: MerchantId,
        region_id: RegionId,
        stolen_value: u32,
    },
    MerchantAssaulted {
        merchant_id: MerchantId,
        region_id: RegionId,
    },
    NpcKilled {
        region_id: RegionId,
        victim: Entity,
    },
    CaravanRaided {
        source_city_id: CityId,
        target_city_id: CityId,
        stolen_quantity: u32,
    },
    CastleAssault {
        city_id: CityId,
        succeeded: bool,
        casualties_estimate: u16,
    },
}

#[derive(Debug, Clone, Message)]
pub struct GovernorAssassinatedEvent {
    pub city_id: CityId,
}

#[derive(Debug, Clone, Message)]
pub struct SuccessorAssassinatedEvent {
    pub city_id: CityId,
    pub successor_id: u32,
}

#[derive(Debug, Clone, Message)]
pub struct CastleAssaultStartedEvent {
    pub city_id: CityId,
}

#[derive(Debug, Clone, Message)]
pub struct CompleteCoupChainEvent {
    pub city_id: CityId,
}

#[derive(Debug, Clone, Message)]
pub struct GovernorPolicyDecisionEvent {
    pub city_id: CityId,
    pub tariff_delta: i16,
    pub subsidy_delta: i16,
    pub war_levy_delta: i16,
    pub rationing_delta: i16,
    pub tax_delta: i16,
    pub market_fee_delta: i16,
    pub add_export_bans: Vec<ItemMaterial>,
    pub clear_export_bans: bool,
    pub reason: String,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct GovernorNpc {
    pub city_id: CityId,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct SuccessorNpc {
    pub city_id: CityId,
    pub successor_id: u32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct GovernorCombatant {
    pub city_id: CityId,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct SuccessorCombatant {
    pub city_id: CityId,
    pub successor_id: u32,
}

pub struct GovernancePlugin;

impl Plugin for GovernancePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReputationLedger>()
            .init_resource::<PlayerCrimeStatus>()
            .init_resource::<GlobalPunishmentState>()
            .init_resource::<CoupChainState>()
            .init_resource::<CoupPreparationProgress>()
            .init_resource::<GovernanceClock>()
            .init_resource::<CastleAssaultClock>()
            .init_resource::<GovernorPolicyClock>()
            .insert_resource(Messages::<ReputationChangeEvent>::default())
            .insert_resource(Messages::<ReputationIncidentEvent>::default())
            .insert_resource(Messages::<GovernorAssassinatedEvent>::default())
            .insert_resource(Messages::<SuccessorAssassinatedEvent>::default())
            .insert_resource(Messages::<CastleAssaultStartedEvent>::default())
            .insert_resource(Messages::<CompleteCoupChainEvent>::default())
            .insert_resource(Messages::<GovernorPolicyDecisionEvent>::default())
            .add_systems(Update, complete_coup_chain_debug_input)
            .add_systems(Update, coup_chain_input)
            .add_systems(Update, process_complete_coup_chain_events)
            .add_systems(Update, tick_governance_timers)
            .add_systems(Update, governor_policy_ai_tick)
            .add_systems(Update, apply_governor_policy_decision_events)
            .add_systems(Update, process_castle_assault_started_events)
            .add_systems(Update, emit_npc_killed_incidents)
            .add_systems(Update, emit_governor_kills_from_combat)
            .add_systems(Update, emit_successor_kills_from_combat)
            .add_systems(Update, sync_successor_npcs)
            .add_systems(Update, apply_reputation_incidents)
            .add_systems(Update, apply_reputation_events)
            .add_systems(Update, process_city_governance_events)
            .add_systems(Update, assault_merchant_input)
            .add_systems(Update, castle_assault_input.before(process_city_governance_events));
    }
}

pub fn recompute_city_authority_state(city: &mut City) {
    if city.authority.state == CityAuthorityState::LegacyUnderSiege {
        city.authority.under_siege = true;
    }
    let severe_low_order = city.security <= 160 && city.stability <= 180;
    let collapse_by_crisis =
        city.authority.succession_crisis_level >= 4 || city.authority.governor_deaths_total >= 5;
    if collapse_by_crisis || severe_low_order {
        city.authority.state = CityAuthorityState::CollapsedAuthority;
        return;
    }

    if !city.authority.current_governor_alive {
        city.authority.state = CityAuthorityState::Interregnum;
        return;
    }

    if city.authority.under_siege
        || city.authority.succession_crisis_level >= 2
        || city.security <= 360
        || city.stability <= 340
    {
        city.authority.state = CityAuthorityState::MartialLaw;
        return;
    }

    let successor_is_ruling = city
        .authority
        .active_successor
        .as_ref()
        .map(|s| s.alive && s.name == city.governor_name)
        .unwrap_or(false);
    if successor_is_ruling
        && city.authority.governor_deaths_total > 0
        && (city.authority.succession_crisis_level > 0
            || city.authority.leadership_immunity_remaining_ticks > 0)
    {
        city.authority.state = CityAuthorityState::SuccessorInstalled;
        return;
    }

    city.authority.state = CityAuthorityState::Stable;
}

fn punishment_multiplier_bps(assassinations_total: u32) -> u32 {
    let quad = assassinations_total.saturating_mul(assassinations_total).saturating_mul(1_250);
    let linear = assassinations_total.saturating_mul(1_200);
    (10_000_u32.saturating_add(linear).saturating_add(quad)).clamp(10_000, 70_000)
}

fn scale_by_bps_u32(base: u32, bps: u32) -> u32 {
    base.saturating_mul(bps).saturating_add(9_999) / 10_000
}

fn scale_by_bps_i16(base: i16, bps: u32) -> i16 {
    let scaled = (i32::from(base)
        .saturating_mul(i32::try_from(bps).unwrap_or(i32::MAX))
        .saturating_add(9_999))
        / 10_000;
    scaled.clamp(i16::MIN as i32, i16::MAX as i32) as i16
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

fn city_for_region(cities: &CityCatalog, region_id: RegionId) -> Option<&City> {
    cities
        .0
        .values()
        .find(|city| city.region_ids.contains(&region_id))
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
    (count > 0).then_some(Vec2::new(sx / count as f32, sy / count as f32))
}

fn city_anchor_position(city_id: CityId, cities: &CityCatalog, map: &MapTiles) -> Option<Vec2> {
    let city = cities.0.get(&city_id)?;
    let region_id = *city.region_ids.first()?;
    region_centroid_from_map(map, region_id)
}

fn assault_target_city_id(region_id: RegionId, cities: &CityCatalog, map: &MapTiles) -> Option<CityId> {
    if let Some(city) = city_for_region(cities, region_id) {
        return Some(city.id);
    }
    let region_center = region_centroid_from_map(map, region_id)?;
    cities
        .0
        .values()
        .filter_map(|city| city_anchor_position(city.id, cities, map).map(|center| (city.id, center)))
        .min_by(|(_, a), (_, b)| region_center.distance(*a).total_cmp(&region_center.distance(*b)))
        .map(|(city_id, _)| city_id)
}

fn complete_coup_chain_debug_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    current_area: Res<CurrentArea>,
    cities: Res<CityCatalog>,
    map: Res<MapTiles>,
    mut complete_events: MessageWriter<CompleteCoupChainEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Exploring || !input.just_pressed(KeyCode::KeyJ) {
        return;
    }
    let Some(city_id) = assault_target_city_id(current_area.0, &cities, &map) else {
        return;
    };
    complete_events.write(CompleteCoupChainEvent { city_id });
    logs.write(TradeLogEvent {
        message: format!(
            "coup_chain_progress city={} step=prepared (debug key J)",
            city_id
        ),
    });
}

fn process_complete_coup_chain_events(
    mut events: MessageReader<CompleteCoupChainEvent>,
    mut coup: ResMut<CoupChainState>,
    mut progress: ResMut<CoupPreparationProgress>,
) {
    for evt in events.read() {
        coup.prepared_by_city.insert(evt.city_id);
        progress.stage_by_city.remove(&evt.city_id);
    }
}

fn apply_u16_delta(value: u16, delta: i16, cap: u16) -> u16 {
    let next = i32::from(value).saturating_add(i32::from(delta));
    next.clamp(0, i32::from(cap)) as u16
}

fn governor_policy_ai_tick(
    timestamp: Res<Timestamp>,
    mut clock: ResMut<GovernorPolicyClock>,
    cities: Res<CityCatalog>,
    mut decisions: MessageWriter<GovernorPolicyDecisionEvent>,
) {
    let now = timestamp.0;
    let elapsed = now.saturating_sub(clock.last_timestamp);
    let mut steps = elapsed / GOVERNOR_POLICY_INTERVAL_TIMESTAMP;
    if steps == 0 {
        return;
    }
    steps = steps.min(8);
    clock.last_timestamp = clock
        .last_timestamp
        .saturating_add(steps.saturating_mul(GOVERNOR_POLICY_INTERVAL_TIMESTAMP));

    for _ in 0..steps {
        for city in cities.0.values() {
            let mut shortage_pressure: i32 = 0;
            for (&material, &priority) in &city.import_material_priority {
                let stock = city.material_stockpile.get(&material).copied().unwrap_or(0);
                let target = 1_800_u32.saturating_add(u32::from(priority) * 20);
                shortage_pressure += (target as i32 - stock as i32).max(0);
            }

            let under_pressure = matches!(
                city.authority.state,
                CityAuthorityState::MartialLaw
                    | CityAuthorityState::Interregnum
                    | CityAuthorityState::CollapsedAuthority
            ) || city.authority.under_siege;

            if under_pressure {
                decisions.write(GovernorPolicyDecisionEvent {
                    city_id: city.id,
                    tariff_delta: 35,
                    subsidy_delta: 20,
                    war_levy_delta: 45,
                    rationing_delta: 55,
                    tax_delta: 30,
                    market_fee_delta: 25,
                    add_export_bans: vec![
                        ItemMaterial::IronIngot,
                        ItemMaterial::SilverSteelIngot,
                        ItemMaterial::CrystalDust,
                    ],
                    clear_export_bans: false,
                    reason: "security_crackdown".to_string(),
                });
                continue;
            }

            if shortage_pressure > 3_500 {
                decisions.write(GovernorPolicyDecisionEvent {
                    city_id: city.id,
                    tariff_delta: -15,
                    subsidy_delta: 35,
                    war_levy_delta: 0,
                    rationing_delta: 40,
                    tax_delta: 0,
                    market_fee_delta: -10,
                    add_export_bans: vec![ItemMaterial::CrystalDust],
                    clear_export_bans: false,
                    reason: "shortage_mitigation".to_string(),
                });
                continue;
            }

            if city.prosperity <= 430 {
                decisions.write(GovernorPolicyDecisionEvent {
                    city_id: city.id,
                    tariff_delta: -20,
                    subsidy_delta: 25,
                    war_levy_delta: -20,
                    rationing_delta: 0,
                    tax_delta: -35,
                    market_fee_delta: -30,
                    add_export_bans: Vec::new(),
                    clear_export_bans: false,
                    reason: "recovery_program".to_string(),
                });
                continue;
            }

            if city.prosperity >= 760 && city.stability >= 720 && city.security >= 720 {
                decisions.write(GovernorPolicyDecisionEvent {
                    city_id: city.id,
                    tariff_delta: -25,
                    subsidy_delta: -15,
                    war_levy_delta: -30,
                    rationing_delta: -40,
                    tax_delta: -20,
                    market_fee_delta: -20,
                    add_export_bans: Vec::new(),
                    clear_export_bans: true,
                    reason: "normalization".to_string(),
                });
            }
        }
    }
}

fn apply_governor_policy_decision_events(
    mut events: MessageReader<GovernorPolicyDecisionEvent>,
    mut cities: ResMut<CityCatalog>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    for evt in events.read() {
        let Some(city) = cities.0.get_mut(&evt.city_id) else {
            continue;
        };

        city.governor_policy.import_tariff_bps =
            apply_u16_delta(city.governor_policy.import_tariff_bps, evt.tariff_delta, 2_000);
        city.governor_policy.market_subsidy_bps =
            apply_u16_delta(city.governor_policy.market_subsidy_bps, evt.subsidy_delta, 1_600);
        city.governor_policy.war_levy_bps =
            apply_u16_delta(city.governor_policy.war_levy_bps, evt.war_levy_delta, 2_000);
        city.governor_policy.rationing_bps =
            apply_u16_delta(city.governor_policy.rationing_bps, evt.rationing_delta, 1_000);
        city.tax_rate_bps = apply_u16_delta(city.tax_rate_bps, evt.tax_delta, 3_000);
        city.market_fee_bps = apply_u16_delta(city.market_fee_bps, evt.market_fee_delta, 3_000);

        if evt.clear_export_bans {
            city.governor_policy.export_ban_materials.clear();
        }
        for m in &evt.add_export_bans {
            if !city.governor_policy.export_ban_materials.contains(m) {
                city.governor_policy.export_ban_materials.push(*m);
            }
        }

        logs.write(TradeLogEvent {
            message: format!(
                "governor_policy_decision city={} reason={} tariff={} subsidy={} war_levy={} rationing={} tax={} fee={} export_bans={}",
                city.name,
                evt.reason,
                city.governor_policy.import_tariff_bps,
                city.governor_policy.market_subsidy_bps,
                city.governor_policy.war_levy_bps,
                city.governor_policy.rationing_bps,
                city.tax_rate_bps,
                city.market_fee_bps,
                city.governor_policy.export_ban_materials.len()
            ),
        });
    }
}

fn coup_chain_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    current_area: Res<CurrentArea>,
    map: Res<MapTiles>,
    cities: Res<CityCatalog>,
    player_inventory: Res<PlayerInventory>,
    mut player_wallet: ResMut<PlayerWallet>,
    mut progress: ResMut<CoupPreparationProgress>,
    mut crime: ResMut<PlayerCrimeStatus>,
    punish: Res<GlobalPunishmentState>,
    mut complete_events: MessageWriter<CompleteCoupChainEvent>,
    mut rep_changes: MessageWriter<ReputationChangeEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Exploring {
        return;
    }
    if !input.just_pressed(KeyCode::KeyU)
        && !input.just_pressed(KeyCode::KeyI)
        && !input.just_pressed(KeyCode::KeyO)
    {
        return;
    }

    let Some(city_id) = assault_target_city_id(current_area.0, &cities, &map) else {
        return;
    };
    let Some(city) = cities.0.get(&city_id) else {
        return;
    };
    let stage = progress.stage_by_city.get(&city_id).copied().unwrap_or(0);

    if input.just_pressed(KeyCode::KeyU) {
        if stage >= 1 {
            logs.write(TradeLogEvent {
                message: format!("coup_chain city={} intel already gathered", city.name),
            });
            return;
        }
        if COUP_STEP_INTEL_COST > 0 {
            if player_wallet.coins < COUP_STEP_INTEL_COST {
                logs.write(TradeLogEvent {
                    message: format!("coup_chain city={} failed: not enough coins for intel", city.name),
                });
                return;
            }
            player_wallet.coins = player_wallet.coins.saturating_sub(COUP_STEP_INTEL_COST);
        }
        progress.stage_by_city.insert(city_id, 1);
        logs.write(TradeLogEvent {
            message: format!("coup_chain city={} step=1 intel_gathered", city.name),
        });
        return;
    }

    if input.just_pressed(KeyCode::KeyI) {
        if stage < 1 {
            logs.write(TradeLogEvent {
                message: format!("coup_chain city={} blocked: gather intel first (U)", city.name),
            });
            return;
        }
        if stage >= 2 {
            logs.write(TradeLogEvent {
                message: format!("coup_chain city={} retainers already bribed", city.name),
            });
            return;
        }
        if player_wallet.coins < COUP_STEP_BRIBE_COST {
            logs.write(TradeLogEvent {
                message: format!(
                    "coup_chain city={} failed: need {} coins to bribe retainers",
                    city.name, COUP_STEP_BRIBE_COST
                ),
            });
            return;
        }
        player_wallet.coins = player_wallet.coins.saturating_sub(COUP_STEP_BRIBE_COST);
        progress.stage_by_city.insert(city_id, 2);
        rep_changes.write(ReputationChangeEvent {
            target: ReputationTarget::Governor { city_id },
            delta: -6,
            reason: "coup_bribery_rumors".to_string(),
        });
        rep_changes.write(ReputationChangeEvent {
            target: ReputationTarget::Clan {
                clan_name: city.clan_name.clone(),
            },
            delta: -8,
            reason: "coup_bribery_rumors".to_string(),
        });
        logs.write(TradeLogEvent {
            message: format!(
                "coup_chain city={} step=2 retainers_bribed cost={}",
                city.name, COUP_STEP_BRIBE_COST
            ),
        });
        return;
    }

    if stage < 2 {
        logs.write(TradeLogEvent {
            message: format!("coup_chain city={} blocked: bribe retainers first (I)", city.name),
        });
        return;
    }
    if player_wallet.coins < COUP_STEP_SABOTAGE_COST {
        logs.write(TradeLogEvent {
            message: format!(
                "coup_chain city={} failed: need {} coins for sabotage assets",
                city.name, COUP_STEP_SABOTAGE_COST
            ),
        });
        return;
    }
    player_wallet.coins = player_wallet.coins.saturating_sub(COUP_STEP_SABOTAGE_COST);

    let has_raid_tools = player_inventory
        .0
        .iter()
        .any(|s| s.item_id == 5005 && s.quantity > 0);
    let mut chance = 40_i32;
    chance += if has_raid_tools { 20 } else { 0 };
    chance += if city.security <= 420 { 18 } else { 0 };
    chance -= if city.security >= 760 { 16 } else { 0 };
    chance += if city.authority.under_siege { 10 } else { 0 };
    chance = chance.clamp(15, 90);

    let roll = rand::rng().random_range(0..100_i32);
    if roll < chance {
        complete_events.write(CompleteCoupChainEvent { city_id });
        progress.stage_by_city.remove(&city_id);
        logs.write(TradeLogEvent {
            message: format!(
                "coup_chain city={} completed step=3 sabotage_gates success chance={} roll={}",
                city.name, chance, roll
            ),
        });
    } else {
        progress.stage_by_city.insert(city_id, 1);
        crime.infamy = crime.infamy.saturating_add(80);
        crime.wanted_tier = wanted_tier_from_infamy(crime.infamy, punish.assassinations_total);
        rep_changes.write(ReputationChangeEvent {
            target: ReputationTarget::Governor { city_id },
            delta: -10,
            reason: "failed_gate_sabotage".to_string(),
        });
        rep_changes.write(ReputationChangeEvent {
            target: ReputationTarget::Clan {
                clan_name: city.clan_name.clone(),
            },
            delta: -12,
            reason: "failed_gate_sabotage".to_string(),
        });
        logs.write(TradeLogEvent {
            message: format!(
                "coup_chain city={} failed step=3 sabotage_gates chance={} roll={} stage_reset=1",
                city.name, chance, roll
            ),
        });
    }
}

fn tick_governance_timers(
    timestamp: Res<Timestamp>,
    mut clock: ResMut<GovernanceClock>,
    mut cities: ResMut<CityCatalog>,
) {
    let now = timestamp.0;
    let elapsed = now.saturating_sub(clock.last_timestamp);
    if elapsed == 0 {
        return;
    }
    clock.last_timestamp = now;
    let dec = elapsed.min(u32::from(u16::MAX)) as u16;
    for city in cities.0.values_mut() {
        city.authority.leadership_immunity_remaining_ticks =
            city.authority.leadership_immunity_remaining_ticks.saturating_sub(dec);
    }
}

fn process_castle_assault_started_events(
    mut events: MessageReader<CastleAssaultStartedEvent>,
    mut cities: ResMut<CityCatalog>,
    mut incidents: MessageWriter<ReputationIncidentEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    for evt in events.read() {
        let Some(city) = cities.0.get_mut(&evt.city_id) else {
            continue;
        };
        city.authority.under_siege = true;
        city.authority.siege_pressure_bps = city.authority.siege_pressure_bps.saturating_add(150).min(1000);
        city.stability = city.stability.saturating_sub(32);
        city.security = city.security.saturating_sub(44);
        recompute_city_authority_state(city);

        incidents.write(ReputationIncidentEvent {
            kind: ReputationIncidentKind::CastleAssault {
                city_id: city.id,
                succeeded: false,
                casualties_estimate: 18,
            },
        });
        logs.write(TradeLogEvent {
            message: format!(
                "castle_assault_started city={} state={} siege={} pressure={}",
                city.name,
                city.authority.state.label(),
                city.authority.under_siege,
                city.authority.siege_pressure_bps
            ),
        });
    }
}

fn emit_governor_kills_from_combat(
    mut deaths: MessageReader<DeathEvent>,
    governor_q: Query<&GovernorCombatant>,
    player_controlled_q: Query<(), With<PlayerControlled>>,
    mut governor_kills: MessageWriter<GovernorAssassinatedEvent>,
) {
    for evt in deaths.read() {
        let Ok(governor) = governor_q.get(evt.entity) else {
            continue;
        };
        let Some(killer) = evt.killer else {
            continue;
        };
        if player_controlled_q.get(killer).is_err() {
            continue;
        }
        governor_kills.write(GovernorAssassinatedEvent {
            city_id: governor.city_id,
        });
    }
}

fn emit_successor_kills_from_combat(
    mut deaths: MessageReader<DeathEvent>,
    successor_q: Query<&SuccessorCombatant>,
    player_controlled_q: Query<(), With<PlayerControlled>>,
    mut successor_kills: MessageWriter<SuccessorAssassinatedEvent>,
) {
    for evt in deaths.read() {
        let Ok(successor) = successor_q.get(evt.entity) else {
            continue;
        };
        let Some(killer) = evt.killer else {
            continue;
        };
        if player_controlled_q.get(killer).is_err() {
            continue;
        }
        successor_kills.write(SuccessorAssassinatedEvent {
            city_id: successor.city_id,
            successor_id: successor.successor_id,
        });
    }
}

fn sync_successor_npcs(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    cities: Res<CityCatalog>,
    map: Res<MapTiles>,
    existing: Query<(Entity, &SuccessorNpc)>,
) {
    let mut desired: HashMap<CityId, u32> = HashMap::new();
    for city in cities.0.values() {
        let Some(active) = city.authority.active_successor.as_ref() else {
            continue;
        };
        if !active.alive || !city.authority.current_governor_alive {
            continue;
        }
        if active.name != city.governor_name {
            continue;
        }
        desired.insert(city.id, active.id);
    }

    let mut existing_by_city: HashMap<CityId, (Entity, u32)> = HashMap::new();
    for (entity, npc) in existing.iter() {
        existing_by_city.insert(npc.city_id, (entity, npc.successor_id));
    }

    for (&city_id, &(entity, successor_id)) in &existing_by_city {
        if desired.get(&city_id).copied() != Some(successor_id) {
            commands.entity(entity).despawn();
        }
    }

    for (&city_id, &successor_id) in &desired {
        if existing_by_city.contains_key(&city_id) {
            continue;
        }
        let Some(city) = cities.0.get(&city_id) else {
            continue;
        };
        let Some(anchor) = city_anchor_position(city_id, &cities, &map) else {
            continue;
        };
        let successor_name = city
            .authority
            .active_successor
            .as_ref()
            .map(|s| format!("{} {}", s.title, s.name))
            .unwrap_or_else(|| city.governor_name.clone());
        commands.spawn((
            crate::render3d::PlaceholderVisual::character(Color::srgb(0.95, 0.45, 0.12)),
            Transform::from_xyz(
                anchor.x + TILE_WORLD_SIZE * 1.4,
                anchor.y + TILE_WORLD_SIZE * 0.45,
                0.0,
            ),
            EnemyEncounter {
                id: 20_000 + u32::from(city_id),
            },
            SuccessorNpc {
                city_id,
                successor_id,
            },
            Interactable {
                name: format!("Successor {}", successor_name),
                dialogue_id: "The last goodbye 1".to_string(),
            },
            LightSensitive { threshold: 0.15 },
            Name::new(format!("SuccessorNPC({})", city.name)),
        ));
    }
}

fn emit_npc_killed_incidents(
    mut deaths: MessageReader<DeathEvent>,
    battle_side_q: Query<&BattleSide>,
    player_controlled_q: Query<(), With<PlayerControlled>>,
    current_area: Res<CurrentArea>,
    mut incidents: MessageWriter<ReputationIncidentEvent>,
) {
    for evt in deaths.read() {
        let Ok(side) = battle_side_q.get(evt.entity) else {
            continue;
        };
        if !matches!(side, BattleSide::Enemy) {
            continue;
        }
        let Some(killer) = evt.killer else {
            continue;
        };
        if player_controlled_q.get(killer).is_err() {
            continue;
        }
        incidents.write(ReputationIncidentEvent {
            kind: ReputationIncidentKind::NpcKilled {
                region_id: current_area.0,
                victim: evt.entity,
            },
        });
    }
}

fn apply_reputation_incidents(
    mut incidents: MessageReader<ReputationIncidentEvent>,
    cities: Res<CityCatalog>,
    merchants: Res<Merchants>,
    mut rep_changes: MessageWriter<ReputationChangeEvent>,
) {
    for evt in incidents.read() {
        match &evt.kind {
            ReputationIncidentKind::MerchantStoreRobbed {
                merchant_id,
                region_id,
                stolen_value,
            } => {
                rep_changes.write(ReputationChangeEvent {
                    target: ReputationTarget::Merchant {
                        merchant_id: *merchant_id,
                    },
                    delta: -26,
                    reason: format!("merchant_store_robbed_value_{}", stolen_value),
                });
                if let Some(city) = city_for_region(&cities, *region_id) {
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Governor { city_id: city.id },
                        delta: -18,
                        reason: "merchant_store_robbery".to_string(),
                    });
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Clan {
                            clan_name: city.clan_name.clone(),
                        },
                        delta: -20,
                        reason: "merchant_store_robbery".to_string(),
                    });
                }
            }
            ReputationIncidentKind::MerchantAssaulted {
                merchant_id,
                region_id,
            } => {
                rep_changes.write(ReputationChangeEvent {
                    target: ReputationTarget::Merchant {
                        merchant_id: *merchant_id,
                    },
                    delta: -35,
                    reason: "merchant_assaulted".to_string(),
                });
                if let Some(city) = city_for_region(&cities, *region_id) {
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Governor { city_id: city.id },
                        delta: -28,
                        reason: "merchant_assaulted".to_string(),
                    });
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Clan {
                            clan_name: city.clan_name.clone(),
                        },
                        delta: -32,
                        reason: "merchant_assaulted".to_string(),
                    });
                }
            }
            ReputationIncidentKind::NpcKilled { region_id, .. } => {
                if let Some(city) = city_for_region(&cities, *region_id) {
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Governor { city_id: city.id },
                        delta: -14,
                        reason: "npc_killed".to_string(),
                    });
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Clan {
                            clan_name: city.clan_name.clone(),
                        },
                        delta: -16,
                        reason: "npc_killed".to_string(),
                    });
                }
            }
            ReputationIncidentKind::CaravanRaided {
                source_city_id,
                target_city_id,
                stolen_quantity,
            } => {
                if let Some(source) = cities.0.get(source_city_id) {
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Governor {
                            city_id: source.id,
                        },
                        delta: -10,
                        reason: format!("raided_caravan_stolen_{}", stolen_quantity),
                    });
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Clan {
                            clan_name: source.clan_name.clone(),
                        },
                        delta: -12,
                        reason: "raided_caravan".to_string(),
                    });
                    if let Some(&region_id) = source.region_ids.first() {
                        if let Some((merchant_id, _)) = merchants
                            .0
                            .iter()
                            .find(|(_, merchant)| merchant.region_id == region_id)
                        {
                            rep_changes.write(ReputationChangeEvent {
                                target: ReputationTarget::Merchant {
                                    merchant_id: *merchant_id,
                                },
                                delta: -8,
                                reason: "raided_caravan".to_string(),
                            });
                        }
                    }
                }
                if let Some(target) = cities.0.get(target_city_id) {
                    rep_changes.write(ReputationChangeEvent {
                        target: ReputationTarget::Clan {
                            clan_name: target.clan_name.clone(),
                        },
                        delta: -6,
                        reason: "raided_caravan".to_string(),
                    });
                }
            }
            ReputationIncidentKind::CastleAssault {
                city_id,
                succeeded,
                casualties_estimate,
            } => {
                let Some(city) = cities.0.get(city_id) else {
                    continue;
                };
                let governor_delta = if *succeeded { -20 } else { -10 };
                let clan_delta = if *succeeded { -26 } else { -14 };
                rep_changes.write(ReputationChangeEvent {
                    target: ReputationTarget::Governor { city_id: *city_id },
                    delta: governor_delta,
                    reason: format!("castle_assault_casualties_{}", casualties_estimate),
                });
                rep_changes.write(ReputationChangeEvent {
                    target: ReputationTarget::Clan {
                        clan_name: city.clan_name.clone(),
                    },
                    delta: clan_delta,
                    reason: "castle_assault".to_string(),
                });
            }
        }
    }
}

fn apply_reputation_events(
    mut events: MessageReader<ReputationChangeEvent>,
    mut ledger: ResMut<ReputationLedger>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    for evt in events.read() {
        let value = ledger.apply_delta(&evt.target, evt.delta);
        logs.write(TradeLogEvent {
            message: format!(
                "reputation_change target={:?} delta={} now={} reason={}",
                evt.target, evt.delta, value, evt.reason
            ),
        });
    }
}

fn apply_assassination_consequences(
    city: &City,
    crime: &mut PlayerCrimeStatus,
    rep_changes: &mut MessageWriter<ReputationChangeEvent>,
    punish: &mut GlobalPunishmentState,
    base_infamy_delta: u32,
    base_bounty_delta: u32,
    base_rep_delta_abs: i16,
) -> u32 {
    punish.assassinations_total = punish.assassinations_total.saturating_add(1);
    let mult_bps = punishment_multiplier_bps(punish.assassinations_total);

    let infamy_delta = scale_by_bps_u32(base_infamy_delta, mult_bps);
    let bounty_delta = scale_by_bps_u32(base_bounty_delta, mult_bps);

    crime.infamy = crime.infamy.saturating_add(infamy_delta);
    crime.wanted_tier = wanted_tier_from_infamy(crime.infamy, punish.assassinations_total);
    if punish.assassinations_total >= 3 {
        crime.wanted_tier = WantedTier::RealmThreat;
    }

    let bounty = crime
        .bounties_by_clan
        .entry(city.clan_name.clone())
        .or_insert(0);
    *bounty = bounty.saturating_add(bounty_delta);
    if let Some(overlord) = &city.overlord_clan_name {
        let ov = crime.bounties_by_clan.entry(overlord.clone()).or_insert(0);
        *ov = ov.saturating_add(bounty_delta / 2);
    }

    let rep_delta = -scale_by_bps_i16(base_rep_delta_abs.abs(), mult_bps).abs();
    rep_changes.write(ReputationChangeEvent {
        target: ReputationTarget::Governor { city_id: city.id },
        delta: rep_delta,
        reason: "governor_assassination".to_string(),
    });
    rep_changes.write(ReputationChangeEvent {
        target: ReputationTarget::Clan {
            clan_name: city.clan_name.clone(),
        },
        delta: (rep_delta.saturating_mul(5) / 4).min(-1),
        reason: "governor_assassination".to_string(),
    });

    mult_bps
}

fn apply_world_fallout_after_assassination(city: &mut City, mult_bps: u32, is_successor: bool) {
    let severity = ((mult_bps.saturating_sub(10_000)) / 1_200).min(420);
    city.prosperity = city.prosperity.saturating_sub((90 + severity as u16).min(460));
    city.stability = city.stability.saturating_sub((130 + severity as u16).min(520));
    city.security = city.security.saturating_sub((150 + severity as u16).min(560));
    city.garrison_strength = city
        .garrison_strength
        .saturating_sub((80 + (severity / 2) as u16).min(260));

    city.tax_rate_bps = city
        .tax_rate_bps
        .saturating_add((260 + (severity / 2) as u16).min(920));
    city.market_fee_bps = city
        .market_fee_bps
        .saturating_add((180 + (severity / 3) as u16).min(740));
    city.unrest_risk_bps = city
        .unrest_risk_bps
        .saturating_add((220 + (severity / 2) as u16).min(950));

    city.governor_policy.import_tariff_bps = city
        .governor_policy
        .import_tariff_bps
        .saturating_add((180 + (severity / 2) as u16).min(700));
    city.governor_policy.war_levy_bps = city
        .governor_policy
        .war_levy_bps
        .saturating_add((220 + (severity / 2) as u16).min(760));
    city.governor_policy.rationing_bps = city
        .governor_policy
        .rationing_bps
        .saturating_add((260 + (severity / 2) as u16).min(880));

    city.authority.under_siege = true;
    city.authority.siege_pressure_bps = city
        .authority
        .siege_pressure_bps
        .saturating_add((220 + (severity / 2) as u16).min(1_000));

    let item_pressure = if is_successor { 64 } else { 52 };
    city.active_material_market_effects.push(MaterialMarketEffect {
        material: ItemMaterial::IronIngot,
        supply_delta: -(item_pressure as i16),
        demand_delta: item_pressure as i16,
        remaining_ticks: 8,
        reason: "assassination_military_shortage".to_string(),
    });
    city.active_material_market_effects.push(MaterialMarketEffect {
        material: ItemMaterial::Cloth,
        supply_delta: -(item_pressure as i16 / 2),
        demand_delta: (item_pressure as i16 / 2) + 14,
        remaining_ticks: 8,
        reason: "assassination_refugee_strain".to_string(),
    });
    city.active_material_market_effects.push(MaterialMarketEffect {
        material: ItemMaterial::CrystalDust,
        supply_delta: -(item_pressure as i16 / 3),
        demand_delta: (item_pressure as i16 / 2),
        remaining_ticks: 8,
        reason: "assassination_medical_shortage".to_string(),
    });
    city.active_item_market_effects.push(ItemMarketEffect {
        item_id: 5001,
        supply_delta: -(item_pressure as i16 / 2),
        demand_delta: item_pressure as i16,
        remaining_ticks: 8,
        reason: "assassination_weapon_spike".to_string(),
    });

    if city.authority.governor_deaths_total >= 3 {
        city.governor_policy.export_ban_materials = vec![
            ItemMaterial::IronIngot,
            ItemMaterial::SilverSteelIngot,
            ItemMaterial::CrystalDust,
        ];
        city.governor_policy.market_subsidy_bps = city.governor_policy.market_subsidy_bps.max(120);
        city.unrest_risk_bps = city.unrest_risk_bps.saturating_add(140).min(1_000);
    }
    if city.authority.governor_deaths_total >= 4 {
        city.authority.state = CityAuthorityState::CollapsedAuthority;
        city.garrison_strength = city.garrison_strength.saturating_sub(160);
        city.prosperity = city.prosperity.saturating_sub(160);
        city.stability = city.stability.saturating_sub(180);
        city.security = city.security.saturating_sub(180);
        city.tax_rate_bps = city.tax_rate_bps.saturating_add(480).min(2_800);
        city.market_fee_bps = city.market_fee_bps.saturating_add(420).min(2_600);
    }
}

fn apply_neighbor_response(cities: &mut CityCatalog, center_city_id: CityId, mult_bps: u32) {
    let Some(center_city) = cities.0.get(&center_city_id).cloned() else {
        return;
    };
    let boost = ((mult_bps.saturating_sub(10_000)) / 1_800).min(240) as u16;
    let targets: Vec<CityId> = cities
        .0
        .values()
        .filter(|c| c.id != center_city_id)
        .filter(|c| {
            c.trade_route_city_ids.contains(&center_city_id)
                || center_city.trade_route_city_ids.contains(&c.id)
                || c.clan_name == center_city.clan_name
        })
        .map(|c| c.id)
        .collect();
    for target_id in targets {
        let Some(target) = cities.0.get_mut(&target_id) else {
            continue;
        };
        target.governor_policy.import_tariff_bps = target
            .governor_policy
            .import_tariff_bps
            .saturating_add((90 + boost).min(420));
        target.governor_policy.war_levy_bps = target
            .governor_policy
            .war_levy_bps
            .saturating_add((70 + boost / 2).min(380));
        target.market_fee_bps = target.market_fee_bps.saturating_add((45 + boost / 3).min(220));
        if mult_bps >= 22_000 {
            target.authority.under_siege = true;
            target.authority.siege_pressure_bps =
                target.authority.siege_pressure_bps.saturating_add((120 + boost / 2).min(680));
            target.security = target.security.saturating_sub((35 + boost / 3).min(120));
            target.stability = target.stability.saturating_sub((28 + boost / 4).min(100));
        }
    }
}

fn install_next_successor(city: &mut City) -> bool {
    let rule_ticks = u32::from(city.authority.succession_rule.leadership_immunity_ticks)
        .saturating_mul(TIMESTAMP_TICKS_PER_HOUR);
    let immunity_ticks = LEADERSHIP_IMMUNITY_WINDOW_TIMESTAMP
        .max(rule_ticks)
        .min(u32::from(u16::MAX)) as u16;

    for source in &city.authority.succession_rule.source_order {
        match source {
            SuccessionSource::NamedHeir | SuccessionSource::ClanElder => {
                if let Some(candidate) = city
                    .authority
                    .successor_candidates
                    .iter()
                    .filter(|c| c.alive && c.clan_name == city.clan_name)
                    .max_by_key(|c| c.legitimacy)
                    .cloned()
                {
                    city.governor_name = candidate.name.clone();
                    city.governor_title = candidate.title.clone();
                    city.clan_name = candidate.clan_name.clone();
                    city.authority.active_successor = Some(candidate);
                    city.authority.current_governor_alive = true;
                    city.authority.state = CityAuthorityState::SuccessorInstalled;
                    city.authority.leadership_immunity_remaining_ticks = immunity_ticks;
                    return true;
                }
            }
            SuccessionSource::OverlordAppointee => {
                if let Some(overlord) = city.overlord_clan_name.clone() {
                    city.governor_name = format!("{} Appointee", overlord);
                    city.governor_title = "Shugo".to_string();
                    city.clan_name = overlord;
                    city.authority.current_governor_alive = true;
                    city.authority.state = CityAuthorityState::SuccessorInstalled;
                    city.authority.leadership_immunity_remaining_ticks = immunity_ticks;
                    return true;
                }
            }
            SuccessionSource::TempleRegent => {
                city.governor_name = format!("Regent of {}", city.name);
                city.governor_title = "Temple Regent".to_string();
                city.authority.current_governor_alive = true;
                city.authority.state = CityAuthorityState::SuccessorInstalled;
                city.authority.leadership_immunity_remaining_ticks = immunity_ticks;
                return true;
            }
            SuccessionSource::MilitaryCommander => {
                city.governor_name = format!("{} Castellan", city.name);
                city.governor_title = "Castellan".to_string();
                city.authority.current_governor_alive = true;
                city.authority.state = CityAuthorityState::MartialLaw;
                city.authority.leadership_immunity_remaining_ticks = immunity_ticks;
                return true;
            }
        }
    }
    false
}

fn process_city_governance_events(
    mut governor_kills: MessageReader<GovernorAssassinatedEvent>,
    mut successor_kills: MessageReader<SuccessorAssassinatedEvent>,
    mut cities: ResMut<CityCatalog>,
    mut crime: ResMut<PlayerCrimeStatus>,
    mut punish: ResMut<GlobalPunishmentState>,
    mut coup: ResMut<CoupChainState>,
    mut rep_changes: MessageWriter<ReputationChangeEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    for evt in governor_kills.read() {
        let Some(city) = cities.0.get_mut(&evt.city_id) else {
            continue;
        };

        if city.authority.leadership_immunity_remaining_ticks > 0
            && !coup.prepared_by_city.remove(&evt.city_id)
        {
            crime.infamy = crime.infamy.saturating_add(130);
            crime.wanted_tier = wanted_tier_from_infamy(crime.infamy, punish.assassinations_total);
            let bounty = crime.bounties_by_clan.entry(city.clan_name.clone()).or_insert(0);
            *bounty = bounty.saturating_add(18_000);
            city.authority.under_siege = true;
            city.authority.siege_pressure_bps =
                city.authority.siege_pressure_bps.saturating_add(220).min(1_000);
            city.authority.state = CityAuthorityState::MartialLaw;
            city.security = city.security.saturating_sub(50);
            city.stability = city.stability.saturating_sub(36);
            rep_changes.write(ReputationChangeEvent {
                target: ReputationTarget::Governor { city_id: city.id },
                delta: -30,
                reason: "failed_coup_attempt".to_string(),
            });
            rep_changes.write(ReputationChangeEvent {
                target: ReputationTarget::Clan {
                    clan_name: city.clan_name.clone(),
                },
                delta: -36,
                reason: "failed_coup_attempt".to_string(),
            });
            recompute_city_authority_state(city);
            logs.write(TradeLogEvent {
                message: format!(
                    "governor_kill_blocked_immunity city={} immunity_ticks={} need_coup_chain=true",
                    city.name, city.authority.leadership_immunity_remaining_ticks
                ),
            });
            continue;
        }

        city.authority.governor_deaths_total = city.authority.governor_deaths_total.saturating_add(1);
        city.authority.succession_crisis_level = city.authority.succession_crisis_level.saturating_add(1);
        city.authority.current_governor_alive = false;
        city.authority.under_siege = true;
        city.authority.siege_pressure_bps = city.authority.siege_pressure_bps.saturating_add(260).min(1000);
        city.authority.state = CityAuthorityState::Interregnum;
        city.authority.leadership_immunity_remaining_ticks = 0;

        let mult_bps = apply_assassination_consequences(
            city,
            &mut crime,
            &mut rep_changes,
            &mut punish,
            300,
            40_000,
            52,
        );
        apply_world_fallout_after_assassination(city, mult_bps, false);
        let _ = install_next_successor(city);
        recompute_city_authority_state(city);
        let city_name = city.name.clone();
        let city_clan = city.clan_name.clone();
        let authority_state = city.authority.state.label().to_string();
        let crisis_level = city.authority.succession_crisis_level;

        apply_neighbor_response(&mut cities, evt.city_id, mult_bps);
        if punish.assassinations_total >= 2 {
            let target_clans: HashSet<String> =
                cities.0.values().map(|c| c.clan_name.clone()).collect();
            for clan in target_clans {
                if clan == city_clan {
                    continue;
                }
                let entry = crime
                    .bounties_by_clan
                    .entry(clan)
                    .or_insert(0);
                *entry = entry.saturating_add(6_000 + (punish.assassinations_total * 1_500));
            }
        }
        if punish.assassinations_total >= 3 {
            crime.wanted_tier = WantedTier::RealmThreat;
        }

        logs.write(TradeLogEvent {
            message: format!(
                "governor_assassinated city={} mult_bps={} assassinations_total={} state={} crisis={} infamy={} wanted={:?}",
                city_name,
                mult_bps,
                punish.assassinations_total,
                authority_state,
                crisis_level,
                crime.infamy,
                crime.wanted_tier
            ),
        });
    }

    for evt in successor_kills.read() {
        let Some(city) = cities.0.get_mut(&evt.city_id) else {
            continue;
        };

        if city.authority.leadership_immunity_remaining_ticks > 0
            && !coup.prepared_by_city.remove(&evt.city_id)
        {
            crime.infamy = crime.infamy.saturating_add(170);
            crime.wanted_tier = wanted_tier_from_infamy(crime.infamy, punish.assassinations_total);
            let bounty = crime.bounties_by_clan.entry(city.clan_name.clone()).or_insert(0);
            *bounty = bounty.saturating_add(26_000);
            city.authority.under_siege = true;
            city.authority.siege_pressure_bps =
                city.authority.siege_pressure_bps.saturating_add(300).min(1_000);
            city.authority.state = CityAuthorityState::MartialLaw;
            city.security = city.security.saturating_sub(68);
            city.stability = city.stability.saturating_sub(52);
            rep_changes.write(ReputationChangeEvent {
                target: ReputationTarget::Governor { city_id: city.id },
                delta: -36,
                reason: "failed_successor_coup_attempt".to_string(),
            });
            rep_changes.write(ReputationChangeEvent {
                target: ReputationTarget::Clan {
                    clan_name: city.clan_name.clone(),
                },
                delta: -44,
                reason: "failed_successor_coup_attempt".to_string(),
            });
            recompute_city_authority_state(city);
            logs.write(TradeLogEvent {
                message: format!(
                    "successor_kill_blocked_immunity city={} immunity_ticks={} need_coup_chain=true",
                    city.name, city.authority.leadership_immunity_remaining_ticks
                ),
            });
            continue;
        }

        if let Some(active) = &mut city.authority.active_successor {
            if active.id == evt.successor_id {
                active.alive = false;
            }
        }
        for candidate in &mut city.authority.successor_candidates {
            if candidate.id == evt.successor_id {
                candidate.alive = false;
            }
        }

        city.authority.governor_deaths_total = city.authority.governor_deaths_total.saturating_add(1);
        city.authority.succession_crisis_level = city.authority.succession_crisis_level.saturating_add(2);
        city.authority.current_governor_alive = false;
        city.authority.under_siege = true;
        city.authority.siege_pressure_bps = city.authority.siege_pressure_bps.saturating_add(360).min(1000);
        city.authority.state = CityAuthorityState::Interregnum;
        city.authority.leadership_immunity_remaining_ticks = 0;

        let mult_bps = apply_assassination_consequences(
            city,
            &mut crime,
            &mut rep_changes,
            &mut punish,
            380,
            58_000,
            60,
        );
        apply_world_fallout_after_assassination(city, mult_bps.saturating_add(1_200), true);
        let installed = install_next_successor(city);
        if !installed && city.authority.succession_crisis_level >= 3 {
            city.authority.state = CityAuthorityState::CollapsedAuthority;
        }
        recompute_city_authority_state(city);
        let city_name = city.name.clone();
        let city_clan = city.clan_name.clone();
        let authority_state = city.authority.state.label().to_string();
        let crisis_level = city.authority.succession_crisis_level;
        apply_neighbor_response(&mut cities, evt.city_id, mult_bps.saturating_add(1_200));
        if punish.assassinations_total >= 2 {
            let target_clans: HashSet<String> =
                cities.0.values().map(|c| c.clan_name.clone()).collect();
            for clan in target_clans {
                if clan == city_clan {
                    continue;
                }
                let entry = crime
                    .bounties_by_clan
                    .entry(clan)
                    .or_insert(0);
                *entry = entry.saturating_add(7_500 + (punish.assassinations_total * 1_700));
            }
        }
        if punish.assassinations_total >= 3 {
            crime.wanted_tier = WantedTier::RealmThreat;
        }

        logs.write(TradeLogEvent {
            message: format!(
                "successor_assassinated city={} successor_id={} mult_bps={} assassinations_total={} state={} crisis={} infamy={} wanted={:?}",
                city_name,
                evt.successor_id,
                mult_bps,
                punish.assassinations_total,
                authority_state,
                crisis_level,
                crime.infamy,
                crime.wanted_tier
            ),
        });
    }
}

fn castle_assault_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    timestamp: Res<Timestamp>,
    current_area: Res<CurrentArea>,
    map: Res<MapTiles>,
    player_q: Query<&Transform, With<Player>>,
    player_inventory: Res<PlayerInventory>,
    mut cities: ResMut<CityCatalog>,
    mut assault_clock: ResMut<CastleAssaultClock>,
    mut coup: ResMut<CoupChainState>,
    punish: Res<GlobalPunishmentState>,
    mut crime: ResMut<PlayerCrimeStatus>,
    mut governor_kills: MessageWriter<GovernorAssassinatedEvent>,
    mut incidents: MessageWriter<ReputationIncidentEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Exploring || !input.just_pressed(KeyCode::KeyG) {
        return;
    }

    let now = timestamp.0;
    if now < assault_clock.next_allowed_timestamp {
        logs.write(TradeLogEvent {
            message: format!(
                "castle_assault blocked: regroup for {} more ticks",
                assault_clock.next_allowed_timestamp.saturating_sub(now)
            ),
        });
        return;
    }

    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    let Some(city_id) = assault_target_city_id(current_area.0, &cities, &map) else {
        return;
    };
    let Some(anchor) = city_anchor_position(city_id, &cities, &map) else {
        return;
    };

    let distance = player_pos.distance(anchor);
    if distance > CASTLE_ASSAULT_DISTANCE {
        logs.write(TradeLogEvent {
            message: format!(
                "castle_assault blocked: move closer to keep ({:.0}/{:.0})",
                distance, CASTLE_ASSAULT_DISTANCE
            ),
        });
        return;
    }

    let Some(city) = cities.0.get_mut(&city_id) else {
        return;
    };

    city.authority.under_siege = true;
    city.authority.siege_pressure_bps = city.authority.siege_pressure_bps.saturating_add(180).min(1000);
    city.stability = city.stability.saturating_sub(18);
    city.security = city.security.saturating_sub(24);

    let mut rng = rand::rng();
    let mut succeeded = false;
    let casualties_estimate: u16;

    let bypassed_immunity = city.authority.leadership_immunity_remaining_ticks > 0
        && coup.prepared_by_city.remove(&city_id);
    if city.authority.leadership_immunity_remaining_ticks > 0 && !bypassed_immunity {
        casualties_estimate = rng.random_range(12..=32);
        logs.write(TradeLogEvent {
            message: format!(
                "castle_assault repelled [{}]: immunity_ticks={}",
                city.name, city.authority.leadership_immunity_remaining_ticks
            ),
        });
    } else {
        let equipment_count: u32 = player_inventory.0.iter().map(|s| u32::from(s.quantity)).sum();
        let assault_power = 560_u32
            .saturating_add(rng.random_range(0..=420))
            .saturating_add((equipment_count / 2).min(220))
            .saturating_add(u32::from(city.authority.siege_pressure_bps) / 3)
            .saturating_add(u32::from(city.authority.succession_crisis_level) * 60);
        let defense_power = 380_u32
            .saturating_add(u32::from(city.garrison_strength))
            .saturating_add(u32::from(city.security) / 2)
            .saturating_add(u32::from(city.stability) / 3)
            .saturating_add(u32::from(city.governor_policy.war_levy_bps) / 4)
            .saturating_add(if city.authority.state == CityAuthorityState::MartialLaw {
                130
            } else {
                0
            });

        if assault_power >= defense_power {
            succeeded = true;
            casualties_estimate = rng.random_range(40..=110);
            city.garrison_strength = city.garrison_strength.saturating_sub(110);
            city.security = city.security.saturating_sub(170);
            city.stability = city.stability.saturating_sub(130);
            governor_kills.write(GovernorAssassinatedEvent { city_id });
        } else {
            casualties_estimate = rng.random_range(18..=64);
            city.garrison_strength = city.garrison_strength.saturating_sub(35);
            city.security = city.security.saturating_sub(58);
            city.stability = city.stability.saturating_sub(44);
            crime.infamy = crime.infamy.saturating_add(90);
            crime.wanted_tier =
                wanted_tier_from_infamy(crime.infamy, punish.assassinations_total);
            let bounty = crime
                .bounties_by_clan
                .entry(city.clan_name.clone())
                .or_insert(0);
            *bounty = bounty.saturating_add(9_000);
        }
    }

    recompute_city_authority_state(city);
    incidents.write(ReputationIncidentEvent {
        kind: ReputationIncidentKind::CastleAssault {
            city_id,
            succeeded,
            casualties_estimate,
        },
    });

    assault_clock.next_allowed_timestamp = now.saturating_add(CASTLE_ASSAULT_COOLDOWN_TIMESTAMP);
}

fn assault_merchant_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    player_q: Query<&Transform, With<Player>>,
    merchant_q: Query<(&Transform, &MerchantNpc)>,
    merchants: Res<Merchants>,
    mut incidents: MessageWriter<ReputationIncidentEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Exploring || !input.just_pressed(KeyCode::KeyH) {
        return;
    }
    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    let mut nearest: Option<(MerchantId, f32)> = None;
    for (tf, merchant_npc) in merchant_q.iter() {
        let dist = player_pos.distance(tf.translation.truncate());
        if dist > SHOP_OPEN_DISTANCE {
            continue;
        }
        let is_better = nearest.map(|(_, d)| dist < d).unwrap_or(true);
        if is_better {
            nearest = Some((merchant_npc.merchant_id, dist));
        }
    }

    let Some((merchant_id, _)) = nearest else {
        logs.write(TradeLogEvent {
            message: format!(
                "merchant_assault failed: no merchant within {:.0} units",
                SHOP_OPEN_DISTANCE
            ),
        });
        return;
    };
    let Some(merchant) = merchants.0.get(&merchant_id) else {
        return;
    };
    incidents.write(ReputationIncidentEvent {
        kind: ReputationIncidentKind::MerchantAssaulted {
            merchant_id,
            region_id: merchant.region_id,
        },
    });
}
