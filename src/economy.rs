use std::collections::{HashMap, HashSet};
use bevy::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::city_data::{
    City, CityAuthorityState, CityCatalog, ClanCatalog, ClanRelationStatus, ItemMarketEffect,
    MaterialMarketEffect, TradeAccess,
};
use crate::combat_plugin::{
    AccessoryType, ArmorType, Equipment, EquipmentType, ItemMaterial, ItemMaterialCost,
    WeaponType,
};
use crate::constants::TIMESTAMP_TICKS_PER_HOUR;
use crate::core::{GameState, Game_State, Player, Timestamp};
use crate::governance::{
    recompute_city_authority_state, ReputationChangeEvent, ReputationIncidentEvent,
    ReputationIncidentKind, ReputationLedger, ReputationTarget, PlayerCrimeStatus, WantedTier,
};
use crate::map::{tile_center_world, CurrentArea, MapTiles, TILE_WORLD_SIZE};
use crate::money::Money;
use crate::ui_style::{font_size, palette, radius, spacing};

const BUY_MARKUP_BPS: u32 = 12000; // 120% of dynamic base
const SELL_MARKDOWN_BPS: u32 = 7000; // 70% of dynamic base
const MIN_MARKET_FACTOR_BPS: u32 = 5000; // 50%
const MAX_MARKET_FACTOR_BPS: u32 = 20000; // 200%
const MARKET_CENTER: i32 = 2048;
const MARKET_MIN: i32 = 256;
const MARKET_MAX: i32 = 4096;
const SHOP_OPEN_DISTANCE: f32 = 96.0;
const MARKET_FLUCTUATION_INTERVAL_HOURS: u32 = 4; // a few in-game hours
const MARKET_FLUCTUATION_INTERVAL_TIMESTAMP: u32 =
    TIMESTAMP_TICKS_PER_HOUR * MARKET_FLUCTUATION_INTERVAL_HOURS;
const CITY_ECONOMY_INTERVAL_HOURS: u32 = 6;
const CITY_ECONOMY_INTERVAL_TIMESTAMP: u32 =
    TIMESTAMP_TICKS_PER_HOUR * CITY_ECONOMY_INTERVAL_HOURS;
const CARAVAN_INTERVAL_HOURS: u32 = 3;
const CARAVAN_INTERVAL_TIMESTAMP: u32 = TIMESTAMP_TICKS_PER_HOUR * CARAVAN_INTERVAL_HOURS;
const TRADE_ACCESS_TAXED_SUPPLY_BPS: u32 = 6_000;
const TRADE_ACCESS_BLOCKADED_SUPPLY_BPS: u32 = 0;
const TRADE_ACCESS_TAXED_PRICE_MARKUP_BPS: u32 = 420;
const TRADE_ACCESS_BLOCKADED_PRICE_MARKUP_BPS: u32 = 1_450;
const CARAVAN_RAID_DISTANCE: f32 = 84.0;
const CARAVAN_BLOCKADE_LOSS_BPS_PER_STEP: u32 = 2_200;
const CARAVAN_RIVAL_LOSS_BPS_PER_STEP: u32 = 600;

type RegionId = u16;
type MerchantId = u16;
type ItemId = u16;
type StackQty = u16;
type CityId = u16;
type CaravanId = u32;

pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemCatalog>()
            .init_resource::<RegionMaterialMarkets>()
            .init_resource::<CityCatalog>()
            .init_resource::<ClanCatalog>()
            .init_resource::<Merchants>()
            .init_resource::<ActiveMerchant>()
            .init_resource::<PlayerInventory>()
            .init_resource::<PlayerWallet>()
            .init_resource::<ShopUiState>()
            .init_resource::<MarketFluctuationClock>()
            .init_resource::<CityEconomyClock>()
            .init_resource::<CaravanClock>()
            .init_resource::<ActiveCaravans>()
            .insert_resource(Messages::<BuyItemEvent>::default())
            .insert_resource(Messages::<SellItemEvent>::default())
            .insert_resource(Messages::<TradeLogEvent>::default())
            .insert_resource(Messages::<CityMaterialMarketEvent>::default())
            .insert_resource(Messages::<CityItemMarketEvent>::default())
            .add_systems(Update, sync_active_merchant_by_region)
            .add_systems(Update, apply_city_market_events)
            .add_systems(Update, fluctuate_region_markets)
            .add_systems(Update, city_economy_tick)
            .add_systems(Update, caravan_tick)
            .add_systems(Update, sync_caravan_visuals)
            .add_systems(Update, raid_nearby_caravan_input.after(sync_caravan_visuals))
            .add_systems(Update, debug_trade_hotkeys)
            .add_systems(
                Update,
                (
                    toggle_shop_ui_hotkey,
                    ensure_shop_ui_root,
                    handle_shop_ui_input,
                    rob_merchant_store_input,
                    process_buy_item_events,
                    process_sell_item_events,
                    update_shop_ui_text,
                    log_trade_events,
                )
                    .chain(),
            );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialMarketEntry {
    pub supply: u16,
    pub demand: u16,
}

impl Default for MaterialMarketEntry {
    fn default() -> Self {
        Self {
            supply: MARKET_CENTER as u16,
            demand: MARKET_CENTER as u16,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialMarket(pub HashMap<ItemMaterial, MaterialMarketEntry>);

impl Default for MaterialMarket {
    fn default() -> Self {
        let mut map = HashMap::new();
        map.insert(ItemMaterial::IronIngot, MaterialMarketEntry::default());
        map.insert(ItemMaterial::SilverSteelIngot, MaterialMarketEntry::default());
        map.insert(ItemMaterial::OakWood, MaterialMarketEntry::default());
        map.insert(ItemMaterial::Leather, MaterialMarketEntry::default());
        map.insert(ItemMaterial::Cloth, MaterialMarketEntry::default());
        map.insert(ItemMaterial::CrystalDust, MaterialMarketEntry::default());
        Self(map)
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct RegionMaterialMarkets(pub HashMap<RegionId, MaterialMarket>);

impl Default for RegionMaterialMarkets {
    fn default() -> Self {
        let mut by_region = HashMap::new();

        let mut region0 = MaterialMarket::default();
        if let Some(m) = region0.0.get_mut(&ItemMaterial::SilverSteelIngot) {
            m.supply = 2200;
            m.demand = 1960;
        }
        by_region.insert(0, region0);

        let mut region1 = MaterialMarket::default();
        if let Some(m) = region1.0.get_mut(&ItemMaterial::SilverSteelIngot) {
            m.supply = 1900;
            m.demand = 2300;
        }
        if let Some(m) = region1.0.get_mut(&ItemMaterial::CrystalDust) {
            m.supply = 1820;
            m.demand = 2360;
        }
        by_region.insert(1, region1);

        Self(by_region)
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct ItemCatalog(pub HashMap<ItemId, Equipment>);

impl Default for ItemCatalog {
    fn default() -> Self {
        let mut map = HashMap::new();
        map.insert(
            5001,
            Equipment {
                id: 5001,
                name: "Silversteel Blade".to_string(),
                equipment_type: EquipmentType::Weapon(WeaponType::Sword),
                base_price: 32000,
                materials: vec![
                    ItemMaterialCost {
                        material: ItemMaterial::SilverSteelIngot,
                        quantity: 20,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::OakWood,
                        quantity: 4,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::Leather,
                        quantity: 3,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::CrystalDust,
                        quantity: 2,
                    },
                ],
                lethality: 10,
                hit: 5,
                armor: 0,
                agility: 2,
                mind: 0,
                morale: 0,
            },
        );
        map.insert(
            5002,
            Equipment {
                id: 5002,
                name: "Oak Buckler".to_string(),
                equipment_type: EquipmentType::Armor(ArmorType::Shield),
                base_price: 11_500,
                materials: vec![
                    ItemMaterialCost {
                        material: ItemMaterial::IronIngot,
                        quantity: 6,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::OakWood,
                        quantity: 12,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::Leather,
                        quantity: 4,
                    },
                ],
                lethality: 0,
                hit: 1,
                armor: 8,
                agility: -1,
                mind: 0,
                morale: 2,
            },
        );
        map.insert(
            5003,
            Equipment {
                id: 5003,
                name: "Traveler Cloak".to_string(),
                equipment_type: EquipmentType::Armor(ArmorType::LightArmor),
                base_price: 9_800,
                materials: vec![
                    ItemMaterialCost {
                        material: ItemMaterial::Cloth,
                        quantity: 20,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::Leather,
                        quantity: 6,
                    },
                ],
                lethality: 0,
                hit: 0,
                armor: 4,
                agility: 3,
                mind: 2,
                morale: 3,
            },
        );
        map.insert(
            5004,
            Equipment {
                id: 5004,
                name: "Crystal Charm".to_string(),
                equipment_type: EquipmentType::Accessory(AccessoryType::Charm),
                base_price: 12_400,
                materials: vec![
                    ItemMaterialCost {
                        material: ItemMaterial::CrystalDust,
                        quantity: 8,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::SilverSteelIngot,
                        quantity: 4,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::Cloth,
                        quantity: 4,
                    },
                ],
                lethality: 1,
                hit: 2,
                armor: 1,
                agility: 1,
                mind: 6,
                morale: 4,
            },
        );
        map.insert(
            5005,
            Equipment {
                id: 5005,
                name: "Iron Dagger".to_string(),
                equipment_type: EquipmentType::Weapon(WeaponType::Dagger),
                base_price: 7_800,
                materials: vec![
                    ItemMaterialCost {
                        material: ItemMaterial::IronIngot,
                        quantity: 8,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::Leather,
                        quantity: 3,
                    },
                    ItemMaterialCost {
                        material: ItemMaterial::OakWood,
                        quantity: 2,
                    },
                ],
                lethality: 6,
                hit: 7,
                armor: 0,
                agility: 4,
                mind: 0,
                morale: 0,
            },
        );
        Self(map)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryStack {
    pub item_id: ItemId,
    pub quantity: StackQty,
}

/// What currency a merchant trades in. Regular merchants accept the world's
/// gold (the player's `PlayerWallet.coins`); the Merchant from the Contract
/// accepts only Merchant Coins, the spiritual "favor" currency
/// ([`crate::quests::MerchantCoins`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Currency {
    #[default]
    Gold,
    MerchantCoin,
}

/// Read the player's balance in the merchant's currency.
fn player_balance(
    currency: Currency,
    player_wallet: &PlayerWallet,
    merchant_coins: &crate::quests::MerchantCoins,
) -> u32 {
    match currency {
        Currency::Gold => player_wallet.coins.0,
        Currency::MerchantCoin => merchant_coins.0,
    }
}

/// Subtract `amount` from the player's wallet of `currency`. Returns `false`
/// if the player can't afford it (caller is expected to bail without state
/// changes when this returns false).
fn pay_player(
    currency: Currency,
    amount: u32,
    player_wallet: &mut PlayerWallet,
    merchant_coins: &mut crate::quests::MerchantCoins,
) -> bool {
    match currency {
        Currency::Gold => {
            if player_wallet.coins < amount {
                return false;
            }
            player_wallet.coins -= amount;
            true
        }
        Currency::MerchantCoin => {
            if merchant_coins.0 < amount {
                return false;
            }
            merchant_coins.0 -= amount;
            true
        }
    }
}

/// Add `amount` to the player's wallet of `currency` (sale proceeds, refund).
fn refund_player(
    currency: Currency,
    amount: u32,
    player_wallet: &mut PlayerWallet,
    merchant_coins: &mut crate::quests::MerchantCoins,
) {
    match currency {
        Currency::Gold => {
            player_wallet.coins = player_wallet.coins.saturating_add(amount)
        }
        Currency::MerchantCoin => {
            merchant_coins.0 = merchant_coins.0.saturating_add(amount)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Merchant {
    pub id: MerchantId,
    pub name: String,
    pub region_id: RegionId,
    pub coins: Money,
    pub inventory: Vec<InventoryStack>,
    /// Defaults to `Currency::Gold` — only the contract Merchant uses
    /// `MerchantCoin`. `#[serde(default)]` keeps existing RON files
    /// (which don't carry this field) loadable.
    #[serde(default)]
    pub currency: Currency,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct MerchantNpc {
    pub merchant_id: u16,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct Merchants(pub HashMap<MerchantId, Merchant>);

impl Default for Merchants {
    fn default() -> Self {
        let mut map = HashMap::new();
        map.insert(
            1,
            Merchant {
                id: 1,
                name: "Aster of Greenford".to_string(),
                region_id: 0,
                coins: Money(220_000),
                inventory: vec![
                    InventoryStack { item_id: 5001, quantity: 5 },
                    InventoryStack { item_id: 5002, quantity: 7 },
                    InventoryStack { item_id: 5003, quantity: 10 },
                    InventoryStack { item_id: 5004, quantity: 4 },
                    InventoryStack { item_id: 5005, quantity: 12 },
                ],
                currency: Currency::Gold,
            },
        );
        map.insert(
            2,
            Merchant {
                id: 2,
                name: "Rath of Ironpass".to_string(),
                region_id: 1,
                coins: Money(280_000),
                inventory: vec![
                    InventoryStack { item_id: 5001, quantity: 8 },
                    InventoryStack { item_id: 5002, quantity: 3 },
                    InventoryStack { item_id: 5003, quantity: 14 },
                    InventoryStack { item_id: 5004, quantity: 9 },
                    InventoryStack { item_id: 5005, quantity: 16 },
                ],
                currency: Currency::Gold,
            },
        );
        // The Merchant from the Contract — referred to only as "the Merchant"
        // in narrative; older drafts used the placeholder name "Gustav".
        // Trans-regional (id 999, anchored at region 0 for the market lookup
        // until a "no-region" code path is added) and trades exclusively in
        // Merchant Coins. Inventory is a curated list of spiritually
        // significant items rather than mundane gear.
        map.insert(
            999,
            Merchant {
                id: 999,
                name: "The Merchant".to_string(),
                region_id: 0,
                // Stash is in Merchant Coins; the magnitude is the Merchant's
                // willingness to pay for player-sold curios.
                coins: Money(10_000),
                inventory: vec![
                    // Placeholder stock — populate with spiritual items
                    // (talismans, charms, rare reagents) once their item ids
                    // exist.
                    InventoryStack { item_id: 5001, quantity: 2 },
                    InventoryStack { item_id: 5002, quantity: 2 },
                    InventoryStack { item_id: 5003, quantity: 2 },
                ],
                currency: Currency::MerchantCoin,
            },
        );
        Self(map)
    }
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveMerchant(pub Option<MerchantId>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopFocus {
    Buy,
    Sell,
}

#[derive(Resource, Debug, Clone)]
pub struct ShopUiState {
    pub open: bool,
    pub root: Option<Entity>,
    pub focus: ShopFocus,
    pub selected_buy: usize,
    pub selected_sell: usize,
    pub quantity: StackQty,
}

impl Default for ShopUiState {
    fn default() -> Self {
        Self {
            open: false,
            root: None,
            focus: ShopFocus::Buy,
            selected_buy: 0,
            selected_sell: 0,
            quantity: 1,
        }
    }
}

#[derive(Component)]
struct ShopUiRoot;

#[derive(Resource, Debug, Clone)]
pub struct MarketFluctuationClock {
    pub last_timestamp: u32,
}

impl Default for MarketFluctuationClock {
    fn default() -> Self {
        Self { last_timestamp: 0 }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct CityEconomyClock {
    pub last_timestamp: u32,
}

impl Default for CityEconomyClock {
    fn default() -> Self {
        Self { last_timestamp: 0 }
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct CaravanClock {
    pub last_timestamp: u32,
}

impl Default for CaravanClock {
    fn default() -> Self {
        Self { last_timestamp: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Caravan {
    pub id: CaravanId,
    pub material: ItemMaterial,
    pub source_city_id: CityId,
    pub target_city_id: CityId,
    pub quantity: u32,
    #[serde(default)]
    pub departure_timestamp: u32,
    #[serde(default)]
    pub arrival_timestamp: u32,
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveCaravans(pub Vec<Caravan>);

#[derive(Component, Debug, Clone, Copy)]
struct CaravanVisual {
    caravan_id: CaravanId,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInventory(pub Vec<InventoryStack>);

impl Default for PlayerInventory {
    fn default() -> Self {
        Self(vec![
            InventoryStack {
                item_id: 5005,
                quantity: 6,
            },
            InventoryStack {
                item_id: 5003,
                quantity: 3,
            },
            InventoryStack {
                item_id: 5002,
                quantity: 1,
            },
        ])
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct PlayerWallet {
    pub coins: Money,
}

impl Default for PlayerWallet {
    fn default() -> Self {
        Self {
            coins: Money(900_000),
        }
    }
}

#[derive(Debug, Clone, Message)]
pub struct BuyItemEvent {
    pub item_id: ItemId,
    pub quantity: StackQty,
}

#[derive(Debug, Clone, Message)]
pub struct SellItemEvent {
    pub item_id: ItemId,
    pub quantity: StackQty,
}

#[derive(Debug, Clone, Message)]
pub struct TradeLogEvent {
    pub message: String,
}

#[derive(Debug, Clone, Message)]
pub struct CityMaterialMarketEvent {
    pub city_id: CityId,
    pub material: ItemMaterial,
    pub supply_delta: i16,
    pub demand_delta: i16,
    pub stockpile_delta: i32,
    pub duration_ticks: u16,
    pub reason: String,
}

#[derive(Debug, Clone, Message)]
pub struct CityItemMarketEvent {
    pub city_id: CityId,
    pub item_id: ItemId,
    pub supply_delta: i16,
    pub demand_delta: i16,
    pub duration_ticks: u16,
    pub reason: String,
}

fn price_with_bps(base: u32, bps: u32) -> u32 {
    base.saturating_mul(bps).saturating_add(9_999) / 10_000
}

fn material_dynamic_unit_price(material: ItemMaterial, market: &MaterialMarket) -> u32 {
    let base = material as u32;
    let entry = market.0.get(&material).cloned().unwrap_or_default();
    let supply = u32::from(entry.supply).max(1);
    let raw_factor_bps = u32::from(entry.demand).saturating_mul(10_000) / supply;
    let factor_bps = raw_factor_bps.clamp(MIN_MARKET_FACTOR_BPS, MAX_MARKET_FACTOR_BPS);
    price_with_bps(base, factor_bps)
}

fn city_instability_markup_bps(city: &City) -> u32 {
    let stability = u32::from(city.stability.min(1000));
    (1000_u32.saturating_sub(stability)).saturating_mul(3)
}

fn city_siege_markup_bps(city: &City) -> u32 {
    if !city.authority.under_siege {
        return 0;
    }
    let pressure = u32::from(city.authority.siege_pressure_bps.min(1000));
    let base = 280_u32.saturating_add(pressure / 2);
    match city.authority.state {
        CityAuthorityState::CollapsedAuthority => base.saturating_add(520),
        CityAuthorityState::MartialLaw => base.saturating_add(220),
        CityAuthorityState::Interregnum => base.saturating_add(160),
        _ => base,
    }
}

fn city_item_pressure_bps(item_id: ItemId, city: &City) -> i32 {
    let mut demand = 0_i32;
    let mut supply = 0_i32;
    for fx in &city.active_item_market_effects {
        if fx.item_id != item_id {
            continue;
        }
        demand += i32::from(fx.demand_delta);
        supply += i32::from(fx.supply_delta);
    }
    ((demand - supply) * 22).clamp(-2500, 4000)
}

#[derive(Debug, Clone, Copy)]
struct EffectiveCityModifiers {
    tax_rate_bps: u32,
    market_fee_bps: u32,
    instability_markup_bps: u32,
    siege_markup_bps: u32,
    item_pressure_bps: i32,
    policy_tariff_bps: u32,
    policy_subsidy_bps: u32,
    policy_war_levy_bps: u32,
    route_risk_markup_bps: u32,
    reputation_buy_adjust_bps: i32,
    reputation_sell_adjust_bps: i32,
}

#[derive(Debug, Clone, Copy)]
struct PriceBreakdown {
    unit_price: u32,
    dynamic_base: u32,
    base_bps: i32,
    tax_bps: i32,
    market_fee_bps: i32,
    instability_bps: i32,
    siege_bps: i32,
    item_pressure_bps: i32,
    policy_tariff_bps: i32,
    policy_subsidy_bps: i32,
    policy_war_levy_bps: i32,
    route_risk_bps: i32,
    reputation_bps: i32,
    final_bps: u32,
}

fn buy_price_breakdown(
    dynamic_base: u32,
    modifiers: Option<EffectiveCityModifiers>,
) -> PriceBreakdown {
    let Some(mods) = modifiers else {
        let unit_price = price_with_bps(dynamic_base, BUY_MARKUP_BPS);
        return PriceBreakdown {
            unit_price,
            dynamic_base,
            base_bps: BUY_MARKUP_BPS as i32,
            tax_bps: 0,
            market_fee_bps: 0,
            instability_bps: 0,
            siege_bps: 0,
            item_pressure_bps: 0,
            policy_tariff_bps: 0,
            policy_subsidy_bps: 0,
            policy_war_levy_bps: 0,
            route_risk_bps: 0,
            reputation_bps: 0,
            final_bps: BUY_MARKUP_BPS,
        };
    };
    let item_bps = mods.item_pressure_bps;
    let mut bps = BUY_MARKUP_BPS as i32;
    bps += mods.tax_rate_bps as i32;
    bps += mods.market_fee_bps as i32;
    bps += mods.instability_markup_bps as i32;
    bps += mods.siege_markup_bps as i32;
    bps += item_bps;
    bps += mods.policy_tariff_bps as i32;
    bps -= mods.policy_subsidy_bps as i32;
    bps += mods.policy_war_levy_bps as i32;
    bps += mods.route_risk_markup_bps as i32;
    bps += mods.reputation_buy_adjust_bps;
    let bps = bps.clamp(7000, 28000) as u32;
    let unit_price = price_with_bps(dynamic_base, bps);
    PriceBreakdown {
        unit_price,
        dynamic_base,
        base_bps: BUY_MARKUP_BPS as i32,
        tax_bps: mods.tax_rate_bps as i32,
        market_fee_bps: mods.market_fee_bps as i32,
        instability_bps: mods.instability_markup_bps as i32,
        siege_bps: mods.siege_markup_bps as i32,
        item_pressure_bps: item_bps,
        policy_tariff_bps: mods.policy_tariff_bps as i32,
        policy_subsidy_bps: mods.policy_subsidy_bps as i32,
        policy_war_levy_bps: mods.policy_war_levy_bps as i32,
        route_risk_bps: mods.route_risk_markup_bps as i32,
        reputation_bps: mods.reputation_buy_adjust_bps,
        final_bps: bps,
    }
}

fn sell_price_breakdown(
    dynamic_base: u32,
    modifiers: Option<EffectiveCityModifiers>,
) -> PriceBreakdown {
    let Some(mods) = modifiers else {
        let unit_price = price_with_bps(dynamic_base, SELL_MARKDOWN_BPS);
        return PriceBreakdown {
            unit_price,
            dynamic_base,
            base_bps: SELL_MARKDOWN_BPS as i32,
            tax_bps: 0,
            market_fee_bps: 0,
            instability_bps: 0,
            siege_bps: 0,
            item_pressure_bps: 0,
            policy_tariff_bps: 0,
            policy_subsidy_bps: 0,
            policy_war_levy_bps: 0,
            route_risk_bps: 0,
            reputation_bps: 0,
            final_bps: SELL_MARKDOWN_BPS,
        };
    };
    let item_bps = mods.item_pressure_bps;
    let penalty_bps = (mods.tax_rate_bps / 2)
        .saturating_add(mods.market_fee_bps / 2)
        .saturating_add(mods.instability_markup_bps / 2)
        .saturating_add(mods.siege_markup_bps / 2)
        .saturating_add(mods.policy_tariff_bps / 2)
        .saturating_add(mods.policy_war_levy_bps / 2)
        .saturating_add(mods.route_risk_markup_bps / 2);
    let subsidy_relief_bps = mods.policy_subsidy_bps / 2;
    let mut bps = SELL_MARKDOWN_BPS as i32 - penalty_bps as i32;
    bps += subsidy_relief_bps as i32;
    bps += item_bps / 2;
    bps += mods.reputation_sell_adjust_bps;
    let bps = bps.clamp(2500, 14000) as u32;
    let unit_price = price_with_bps(dynamic_base, bps);
    PriceBreakdown {
        unit_price,
        dynamic_base,
        base_bps: SELL_MARKDOWN_BPS as i32,
        tax_bps: -((mods.tax_rate_bps / 2) as i32),
        market_fee_bps: -((mods.market_fee_bps / 2) as i32),
        instability_bps: -((mods.instability_markup_bps / 2) as i32),
        siege_bps: -((mods.siege_markup_bps / 2) as i32),
        item_pressure_bps: item_bps / 2,
        policy_tariff_bps: -((mods.policy_tariff_bps / 2) as i32),
        policy_subsidy_bps: subsidy_relief_bps as i32,
        policy_war_levy_bps: -((mods.policy_war_levy_bps / 2) as i32),
        route_risk_bps: -((mods.route_risk_markup_bps / 2) as i32),
        reputation_bps: mods.reputation_sell_adjust_bps,
        final_bps: bps,
    }
}

fn apply_city_market_events(
    mut material_events: MessageReader<CityMaterialMarketEvent>,
    mut item_events: MessageReader<CityItemMarketEvent>,
    mut cities: ResMut<CityCatalog>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    for evt in material_events.read() {
        let Some(city) = cities.0.get_mut(&evt.city_id) else {
            logs.write(TradeLogEvent {
                message: format!(
                    "city_material_event ignored: unknown city {} ({})",
                    evt.city_id, evt.reason
                ),
            });
            continue;
        };
        if evt.stockpile_delta != 0 {
            let stock = city.material_stockpile.entry(evt.material).or_insert(0);
            if evt.stockpile_delta.is_negative() {
                *stock = stock.saturating_sub(evt.stockpile_delta.unsigned_abs());
            } else {
                *stock = stock.saturating_add(evt.stockpile_delta as u32);
            }
        }
        if evt.supply_delta != 0 || evt.demand_delta != 0 {
            city.active_material_market_effects.push(MaterialMarketEffect {
                material: evt.material,
                supply_delta: evt.supply_delta,
                demand_delta: evt.demand_delta,
                remaining_ticks: evt.duration_ticks.max(1),
                reason: evt.reason.clone(),
            });
        }
        logs.write(TradeLogEvent {
            message: format!(
                "city_material_event [{}]: city={} material={:?} supply_delta={} demand_delta={} stockpile_delta={} ticks={} reason={}",
                city.name,
                evt.city_id,
                evt.material,
                evt.supply_delta,
                evt.demand_delta,
                evt.stockpile_delta,
                evt.duration_ticks.max(1),
                evt.reason
            ),
        });
    }

    for evt in item_events.read() {
        let Some(city) = cities.0.get_mut(&evt.city_id) else {
            logs.write(TradeLogEvent {
                message: format!(
                    "city_item_event ignored: unknown city {} ({})",
                    evt.city_id, evt.reason
                ),
            });
            continue;
        };
        if evt.supply_delta != 0 || evt.demand_delta != 0 {
            city.active_item_market_effects.push(ItemMarketEffect {
                item_id: evt.item_id,
                supply_delta: evt.supply_delta,
                demand_delta: evt.demand_delta,
                remaining_ticks: evt.duration_ticks.max(1),
                reason: evt.reason.clone(),
            });
        }
        logs.write(TradeLogEvent {
            message: format!(
                "city_item_event [{}]: city={} item={} supply_delta={} demand_delta={} ticks={} reason={}",
                city.name,
                evt.city_id,
                evt.item_id,
                evt.supply_delta,
                evt.demand_delta,
                evt.duration_ticks.max(1),
                evt.reason
            ),
        });
    }
}

fn step_market_value(current: u16, rng: &mut impl Rng) -> u16 {
    let current = i32::from(current);
    let random_step = rng.random_range(-3..=3);
    let distance = current - MARKET_CENTER;
    let mut reversion_step = -(distance / 256);
    reversion_step = reversion_step.clamp(-4, 4);
    let next = (current + random_step + reversion_step).clamp(MARKET_MIN, MARKET_MAX);
    next as u16
}

fn fluctuate_region_markets(
    timestamp: Res<Timestamp>,
    mut clock: ResMut<MarketFluctuationClock>,
    mut markets: ResMut<RegionMaterialMarkets>,
) {
    let current = timestamp.0;
    let elapsed = current.saturating_sub(clock.last_timestamp);
    let mut steps = elapsed / MARKET_FLUCTUATION_INTERVAL_TIMESTAMP;
    if steps == 0 {
        return;
    }
    // Avoid a huge catch-up burst if time jumped massively from loading/debug.
    steps = steps.min(12);
    clock.last_timestamp = clock
        .last_timestamp
        .saturating_add(steps.saturating_mul(MARKET_FLUCTUATION_INTERVAL_TIMESTAMP));

    let mut rng = rand::rng();
    for _ in 0..steps {
        for market in markets.0.values_mut() {
            for entry in market.0.values_mut() {
                entry.supply = step_market_value(entry.supply, &mut rng);
                entry.demand = step_market_value(entry.demand, &mut rng);
            }
        }
    }
}

fn city_id_by_region(cities: &CityCatalog) -> HashMap<RegionId, CityId> {
    let mut map = HashMap::new();
    for city in cities.0.values() {
        for &region_id in &city.region_ids {
            map.insert(region_id, city.id);
        }
    }
    map
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

fn import_weight_by_distance(world_distance: f32) -> f32 {
    let tiles = world_distance / TILE_WORLD_SIZE.max(1.0);
    1.0 / (1.0 + (tiles / 2.0).powi(2))
}

fn import_weight_by_route_distance(route_distance_units: f32) -> f32 {
    // Distance constants are abstract travel units; long-haul caravans are strongly discounted.
    let norm = route_distance_units.max(1.0) / 180.0;
    1.0 / (1.0 + norm.powi(2))
}

fn city_distance_units_between(
    from_city_id: CityId,
    to_city_id: CityId,
    cities: &CityCatalog,
    centers: &HashMap<CityId, Vec2>,
) -> Option<f32> {
    let direct = cities
        .0
        .get(&from_city_id)
        .and_then(|c| c.trade_distance_by_city_id.get(&to_city_id).copied())
        .or_else(|| {
            cities
                .0
                .get(&to_city_id)
                .and_then(|c| c.trade_distance_by_city_id.get(&from_city_id).copied())
        });
    if let Some(distance) = direct {
        return Some(distance as f32);
    }
    let from_center = centers.get(&from_city_id).copied()?;
    let to_center = centers.get(&to_city_id).copied()?;
    Some((from_center.distance(to_center) / TILE_WORLD_SIZE.max(1.0)).max(1.0) * 64.0)
}

fn clan_relation_between(city_a: &City, city_b: &City, clans: &ClanCatalog) -> ClanRelationStatus {
    if city_a.clan_name == city_b.clan_name {
        return ClanRelationStatus::Allied;
    }
    let Some(clan_a) = clans.0.get(&city_a.clan_name) else {
        return ClanRelationStatus::Neutral;
    };
    clan_a
        .relation_by_clan
        .get(&city_b.clan_name)
        .copied()
        .unwrap_or(ClanRelationStatus::Neutral)
}

fn trade_access_between(city_a: &City, city_b: &City, clans: &ClanCatalog) -> TradeAccess {
    clan_relation_between(city_a, city_b, clans).trade_access()
}

fn trade_access_supply_weight(access: TradeAccess) -> f32 {
    match access {
        TradeAccess::Open => 1.0,
        TradeAccess::Taxed => TRADE_ACCESS_TAXED_SUPPLY_BPS as f32 / 10_000.0,
        TradeAccess::Blockaded => TRADE_ACCESS_BLOCKADED_SUPPLY_BPS as f32 / 10_000.0,
    }
}

fn trade_access_price_markup_bps(access: TradeAccess) -> u32 {
    match access {
        TradeAccess::Open => 0,
        TradeAccess::Taxed => TRADE_ACCESS_TAXED_PRICE_MARKUP_BPS,
        TradeAccess::Blockaded => TRADE_ACCESS_BLOCKADED_PRICE_MARKUP_BPS,
    }
}

fn city_has_war_trade_route(city: &City, cities: &CityCatalog, clans: &ClanCatalog) -> bool {
    city.trade_route_city_ids.iter().any(|partner_id| {
        let Some(partner) = cities.0.get(partner_id) else {
            return false;
        };
        matches!(trade_access_between(city, partner, clans), TradeAccess::Blockaded)
    })
}

fn caravan_progress(caravan: &Caravan, now: u32) -> f32 {
    if caravan.arrival_timestamp <= caravan.departure_timestamp {
        return 1.0;
    }
    let total = caravan.arrival_timestamp - caravan.departure_timestamp;
    let done = now
        .saturating_sub(caravan.departure_timestamp)
        .min(total);
    done as f32 / total as f32
}

fn step_clamped_u16(current: u16, delta: i32) -> u16 {
    let next = i32::from(current).saturating_add(delta);
    next.clamp(MARKET_MIN, MARKET_MAX) as u16
}

fn city_economy_tick(
    timestamp: Res<Timestamp>,
    map: Res<MapTiles>,
    mut clock: ResMut<CityEconomyClock>,
    mut cities: ResMut<CityCatalog>,
    clans: Res<ClanCatalog>,
    mut markets: ResMut<RegionMaterialMarkets>,
) {
    let current = timestamp.0;
    let elapsed = current.saturating_sub(clock.last_timestamp);
    let mut steps = elapsed / CITY_ECONOMY_INTERVAL_TIMESTAMP;
    if steps == 0 {
        return;
    }
    steps = steps.min(12);
    clock.last_timestamp = clock
        .last_timestamp
        .saturating_add(steps.saturating_mul(CITY_ECONOMY_INTERVAL_TIMESTAMP));

    for _ in 0..steps {
        let war_routed_city_ids: HashSet<CityId> = cities
            .0
            .values()
            .filter(|city| city_has_war_trade_route(city, &cities, &clans))
            .map(|city| city.id)
            .collect();
        for city in cities.0.values_mut() {
            let productivity_bps = 900_u32 + (u32::from(city.prosperity.min(1000)) * 2);
            let mut consumption_bps =
                800_u32 + (u32::from(1000_u16.saturating_sub(city.stability.min(1000))) * 2);
            let rationing_factor_bps =
                1000_u32.saturating_sub(u32::from(city.governor_policy.rationing_bps.min(900)));
            consumption_bps = (consumption_bps * rationing_factor_bps + 999) / 1000;

            for (&material, &output) in &city.primary_material_outputs {
                let produced = (u32::from(output) * productivity_bps + 999) / 1000;
                let stock = city.material_stockpile.entry(material).or_insert(0);
                *stock = stock.saturating_add(produced);
            }

            let baseline_pop_need = (city.population / 850).max(1);
            for (&material, &priority) in &city.import_material_priority {
                let weighted = baseline_pop_need.saturating_add(u32::from(priority) / 3);
                let consume = (weighted * consumption_bps + 999) / 1000;
                let stock = city.material_stockpile.entry(material).or_insert(0);
                *stock = stock.saturating_sub(consume);
            }

            // Basic governor policy automation: react to scarcity and wartime pressure.
            let mut shortage_pressure = 0_u32;
            for (&material, &priority) in &city.import_material_priority {
                let stock = city.material_stockpile.get(&material).copied().unwrap_or(0);
                let target = 2_000_u32.saturating_add(u32::from(priority) * 20);
                if stock < target {
                    shortage_pressure =
                        shortage_pressure.saturating_add((target.saturating_sub(stock)) / 120);
                }
            }
            let scarcity_bps = shortage_pressure.min(600) as u16;
            city.governor_policy.rationing_bps = scarcity_bps;
            city.governor_policy.market_subsidy_bps = (scarcity_bps / 3).min(220);
            if war_routed_city_ids.contains(&city.id) {
                city.governor_policy.war_levy_bps = city.governor_policy.war_levy_bps.max(160);
                city.governor_policy.import_tariff_bps = city.governor_policy.import_tariff_bps.max(120);
            } else {
                city.governor_policy.war_levy_bps =
                    city.governor_policy.war_levy_bps.saturating_sub(20);
                city.governor_policy.import_tariff_bps =
                    city.governor_policy.import_tariff_bps.saturating_sub(10);
            }

            if war_routed_city_ids.contains(&city.id) {
                city.authority.under_siege = true;
                city.authority.siege_pressure_bps =
                    city.authority.siege_pressure_bps.saturating_add(55).min(1000);
                city.security = city.security.saturating_sub(10);
                city.stability = city.stability.saturating_sub(8);
            } else {
                city.authority.siege_pressure_bps =
                    city.authority.siege_pressure_bps.saturating_sub(38);
                if city.authority.siege_pressure_bps == 0 {
                    city.authority.under_siege = false;
                }
            }
            if city.authority.current_governor_alive
                && !city.authority.under_siege
                && city.stability >= 620
                && city.security >= 620
                && city.authority.succession_crisis_level > 0
            {
                city.authority.succession_crisis_level =
                    city.authority.succession_crisis_level.saturating_sub(1);
            }

            // Tick down temporary market effects from world events.
            for fx in &mut city.active_material_market_effects {
                fx.remaining_ticks = fx.remaining_ticks.saturating_sub(1);
            }
            city.active_material_market_effects
                .retain(|fx| fx.remaining_ticks > 0);
            for fx in &mut city.active_item_market_effects {
                fx.remaining_ticks = fx.remaining_ticks.saturating_sub(1);
            }
            city.active_item_market_effects
                .retain(|fx| fx.remaining_ticks > 0);

            recompute_city_authority_state(city);
        }
    }

    let region_to_city = city_id_by_region(&cities);
    let centers = city_centroids(&cities, &map);
    let mut city_stock_by_material = HashMap::new();
    for city in cities.0.values() {
        city_stock_by_material.insert(city.id, city.material_stockpile.clone());
    }

    let region_ids: Vec<RegionId> = markets.0.keys().copied().collect();
    for region_id in region_ids {
        let Some(region_market) = markets.0.get_mut(&region_id) else {
            continue;
        };
        let local_weights = if let Some(home_city_id) = region_to_city.get(&region_id).copied() {
            vec![(home_city_id, 1.0)]
        } else {
            nearest_city_weights_for_region(region_id, &cities, &map, &centers, 3)
        };
        if local_weights.is_empty() {
            continue;
        }
        let region_center = region_centroid_from_map(&map, region_id);

        for (material, entry) in &mut region_market.0 {
            let mut local_stock = 0.0_f32;
            let mut target_stock = 0.0_f32;
            for (city_id, w) in &local_weights {
                let stock = city_stock_by_material
                    .get(city_id)
                    .and_then(|m| m.get(material))
                    .copied()
                    .unwrap_or(0) as f32;
                local_stock += stock * *w;
                let city_target = cities
                    .0
                    .get(city_id)
                    .and_then(|c| c.import_material_priority.get(material).copied())
                    .map(|p| 2_000_u32 + u32::from(p) * 20)
                    .unwrap_or(2_200) as f32;
                target_stock += city_target * *w;
            }
            let local_stock = local_stock.max(0.0) as u32;
            let target_stock = target_stock.max(1.0) as u32;

            let mut import_supply = 0.0_f32;
            for (home_city_id, home_w) in &local_weights {
                let Some(home_city) = cities.0.get(home_city_id) else {
                    continue;
                };
                for (other_city_id, stockpile) in &city_stock_by_material {
                    if home_city_id == other_city_id {
                        continue;
                    }
                    let Some(other_city) = cities.0.get(other_city_id) else {
                        continue;
                    };
                    if other_city
                        .governor_policy
                        .export_ban_materials
                        .contains(material)
                    {
                        continue;
                    }
                    let stock = stockpile.get(material).copied().unwrap_or(0);
                    if stock == 0 {
                        continue;
                    }
                    let route_distance = city_distance_units_between(
                        *home_city_id,
                        *other_city_id,
                        &cities,
                        &centers,
                    )
                    .or_else(|| {
                        let rc = region_center?;
                        let oc = centers.get(other_city_id).copied()?;
                        Some((rc.distance(oc) / TILE_WORLD_SIZE.max(1.0)).max(1.0) * 64.0)
                    });
                    let Some(route_distance) = route_distance else {
                        continue;
                    };
                    let distance_weight = import_weight_by_route_distance(route_distance);
                    let access = trade_access_between(home_city, other_city, &clans);
                    let access_weight = trade_access_supply_weight(access);
                    if access_weight <= 0.0 {
                        continue;
                    }
                    import_supply += stock as f32 * *home_w * distance_weight * access_weight;
                }
            }

            let mut material_fx_supply = 0.0_f32;
            let mut material_fx_demand = 0.0_f32;
            for (city_id, w) in &local_weights {
                let Some(city) = cities.0.get(city_id) else {
                    continue;
                };
                let s: i32 = city
                    .active_material_market_effects
                    .iter()
                    .filter(|fx| fx.material == *material)
                    .map(|fx| i32::from(fx.supply_delta))
                    .sum();
                let d: i32 = city
                    .active_material_market_effects
                    .iter()
                    .filter(|fx| fx.material == *material)
                    .map(|fx| i32::from(fx.demand_delta))
                    .sum();
                material_fx_supply += s as f32 * *w;
                material_fx_demand += d as f32 * *w;
            }

            let local_supply_delta =
                ((local_stock as i32 - target_stock as i32) / 36).clamp(-90, 90);
            let import_supply_delta = (import_supply / 220.0).clamp(0.0, 80.0) as i32;
            let demand_shortage_delta =
                ((target_stock as i32 - local_stock as i32) / 44).clamp(0, 95);
            let remoteness_penalty = if import_supply < 200.0 && local_stock < target_stock / 3 {
                ((target_stock / 3 - local_stock) / 80).min(65) as i32
            } else {
                0
            };

            entry.supply = step_clamped_u16(entry.supply, local_supply_delta + import_supply_delta);
            entry.demand =
                step_clamped_u16(entry.demand, demand_shortage_delta + remoteness_penalty - 18);
            entry.supply = step_clamped_u16(entry.supply, material_fx_supply as i32);
            entry.demand = step_clamped_u16(entry.demand, material_fx_demand as i32);
        }
    }
}

fn caravan_tick(
    timestamp: Res<Timestamp>,
    mut clock: ResMut<CaravanClock>,
    mut caravans: ResMut<ActiveCaravans>,
    mut cities: ResMut<CityCatalog>,
    clans: Res<ClanCatalog>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    let current = timestamp.0;
    let elapsed = current.saturating_sub(clock.last_timestamp);
    let mut steps = elapsed / CARAVAN_INTERVAL_TIMESTAMP;
    if steps == 0 {
        return;
    }
    steps = steps.min(24);
    let step_start = clock.last_timestamp;

    for step_index in 1..=steps {
        let step_timestamp =
            step_start.saturating_add(step_index.saturating_mul(CARAVAN_INTERVAL_TIMESTAMP));

        for caravan in &mut caravans.0 {
            if caravan.quantity == 0 || caravan.arrival_timestamp <= step_timestamp {
                continue;
            }
            let Some(source_city) = cities.0.get(&caravan.source_city_id) else {
                continue;
            };
            let Some(target_city) = cities.0.get(&caravan.target_city_id) else {
                continue;
            };
            let access = trade_access_between(source_city, target_city, &clans);
            let loss_bps = match access {
                TradeAccess::Blockaded => CARAVAN_BLOCKADE_LOSS_BPS_PER_STEP,
                TradeAccess::Taxed => CARAVAN_RIVAL_LOSS_BPS_PER_STEP,
                TradeAccess::Open => 0,
            };
            let source_collapse_bps: u32 = match source_city.authority.state {
                CityAuthorityState::CollapsedAuthority => 2_800,
                CityAuthorityState::Interregnum => 1_500,
                CityAuthorityState::MartialLaw => 850,
                _ => 0,
            };
            let target_collapse_bps: u32 = match target_city.authority.state {
                CityAuthorityState::CollapsedAuthority => 2_800,
                CityAuthorityState::Interregnum => 1_500,
                CityAuthorityState::MartialLaw => 850,
                _ => 0,
            };
            let turmoil_bps = source_collapse_bps.saturating_add(target_collapse_bps);
            let total_loss_bps = loss_bps.saturating_add(turmoil_bps);
            if total_loss_bps == 0 {
                continue;
            }
            let raw_loss = caravan.quantity.saturating_mul(total_loss_bps) / 10_000;
            let loss = raw_loss.max(1).min(caravan.quantity);
            caravan.quantity = caravan.quantity.saturating_sub(loss);
            logs.write(TradeLogEvent {
                message: format!(
                    "caravan_attrition id={} access={:?} turmoil_bps={} loss={} remaining={}",
                    caravan.id, access, turmoil_bps, loss, caravan.quantity
                ),
            });
        }

        let mut deliveries: Vec<Caravan> = Vec::new();
        let mut survivors: Vec<Caravan> = Vec::new();
        for caravan in caravans.0.drain(..) {
            if caravan.quantity == 0 {
                logs.write(TradeLogEvent {
                    message: format!(
                        "caravan_lost id={} {} -> {}",
                        caravan.id, caravan.source_city_id, caravan.target_city_id
                    ),
                });
                continue;
            }
            if caravan.arrival_timestamp <= step_timestamp {
                deliveries.push(caravan);
            } else {
                survivors.push(caravan);
            }
        }
        caravans.0 = survivors;

        for delivered in deliveries {
            if let Some(target) = cities.0.get_mut(&delivered.target_city_id) {
                let looted_bps = match target.authority.state {
                    CityAuthorityState::CollapsedAuthority => 6_000,
                    CityAuthorityState::Interregnum => 2_500,
                    CityAuthorityState::MartialLaw => 1_300,
                    _ => 0,
                };
                let delivered_qty = delivered
                    .quantity
                    .saturating_sub(delivered.quantity.saturating_mul(looted_bps) / 10_000);
                let stock = target.material_stockpile.entry(delivered.material).or_insert(0);
                *stock = stock.saturating_add(delivered_qty);
            }
            logs.write(TradeLogEvent {
                message: format!(
                    "caravan_arrived id={} {} -> {} material={:?} qty={}",
                    delivered.id,
                    delivered.source_city_id,
                    delivered.target_city_id,
                    delivered.material,
                    delivered.quantity
                ),
            });
        }

        if caravans.0.len() >= 8 {
            continue;
        }

        let mut ids: Vec<CityId> = cities.0.keys().copied().collect();
        ids.sort_unstable();
        let mut next_caravan_id = caravans
            .0
            .iter()
            .map(|c| c.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        for &source_id in &ids {
            if caravans.0.len() >= 8 {
                break;
            }
            let Some(source) = cities.0.get(&source_id).cloned() else {
                continue;
            };
            if matches!(
                source.authority.state,
                CityAuthorityState::CollapsedAuthority | CityAuthorityState::Interregnum
            ) {
                continue;
            }

            let mut best: Option<(CityId, ItemMaterial, u32, u16)> = None;
            for (&material, &source_stock) in &source.material_stockpile {
                if source_stock < 1_200 {
                    continue;
                }
                if source
                    .governor_policy
                    .export_ban_materials
                    .contains(&material)
                {
                    continue;
                }
                for &target_id in &ids {
                    if source_id == target_id {
                        continue;
                    }
                    let Some(target) = cities.0.get(&target_id) else {
                        continue;
                    };
                    if matches!(
                        target.authority.state,
                        CityAuthorityState::CollapsedAuthority
                    ) {
                        continue;
                    }
                    let access = trade_access_between(&source, target, &clans);
                    if matches!(access, TradeAccess::Blockaded) {
                        continue;
                    }
                    let target_stock = target.material_stockpile.get(&material).copied().unwrap_or(0);
                    if source_stock <= target_stock.saturating_add(600) {
                        continue;
                    }
                    let Some(distance) =
                        source.trade_distance_by_city_id.get(&target_id).copied().or_else(|| {
                            target.trade_distance_by_city_id.get(&source_id).copied()
                        })
                    else {
                        continue;
                    };
                    let access_factor = trade_access_supply_weight(access).max(0.05);
                    let transferable = source_stock
                        .saturating_sub(target_stock)
                        .saturating_div(10)
                        .min(420);
                    if transferable < 40 {
                        continue;
                    }
                    let score = (transferable as f32 * access_factor) / (distance.max(1) as f32);
                    let current_best = best
                        .map(|(_, _, qty, dist)| qty as f32 / (dist.max(1) as f32))
                        .unwrap_or(0.0);
                    if score > current_best {
                        best = Some((target_id, material, transferable, distance));
                    }
                }
            }

            let Some((target_id, material, qty, distance)) = best else {
                continue;
            };
            let Some(source_city_mut) = cities.0.get_mut(&source_id) else {
                continue;
            };
            let source_stock = source_city_mut.material_stockpile.entry(material).or_insert(0);
            if *source_stock < qty {
                continue;
            }
            *source_stock = source_stock.saturating_sub(qty);

            let travel_ticks = ((distance as u32 + 119) / 120).clamp(1, 12) as u16;
            let travel_duration = u32::from(travel_ticks).saturating_mul(CARAVAN_INTERVAL_TIMESTAMP);
            caravans.0.push(Caravan {
                id: next_caravan_id,
                material,
                source_city_id: source_id,
                target_city_id: target_id,
                quantity: qty,
                departure_timestamp: step_timestamp,
                arrival_timestamp: step_timestamp.saturating_add(travel_duration),
            });
            logs.write(TradeLogEvent {
                message: format!(
                    "caravan_departed id={} {} -> {} material={:?} qty={} dep={} arr={}",
                    next_caravan_id,
                    source_id,
                    target_id,
                    material,
                    qty,
                    step_timestamp,
                    step_timestamp.saturating_add(travel_duration)
                ),
            });
            next_caravan_id = next_caravan_id.saturating_add(1);
        }
    }

    clock.last_timestamp = step_start.saturating_add(steps.saturating_mul(CARAVAN_INTERVAL_TIMESTAMP));
}

fn sync_caravan_visuals(
    mut commands: Commands,
    timestamp: Res<Timestamp>,
    map: Res<MapTiles>,
    cities: Res<CityCatalog>,
    caravans: Res<ActiveCaravans>,
    existing: Query<(Entity, &CaravanVisual)>,
) {
    let mut existing_by_id: HashMap<CaravanId, Entity> = HashMap::new();
    for (entity, visual) in existing.iter() {
        existing_by_id.insert(visual.caravan_id, entity);
    }

    let centers = city_centroids(&cities, &map);
    let mut active_ids: HashSet<CaravanId> = HashSet::new();
    for caravan in &caravans.0 {
        let Some(source) = centers.get(&caravan.source_city_id).copied() else {
            continue;
        };
        let Some(target) = centers.get(&caravan.target_city_id).copied() else {
            continue;
        };
        let progress = caravan_progress(caravan, timestamp.0).clamp(0.0, 1.0);
        let pos = source.lerp(target, progress);
        active_ids.insert(caravan.id);

        if let Some(entity) = existing_by_id.get(&caravan.id).copied() {
            commands.entity(entity).insert(Transform::from_xyz(pos.x, pos.y, 9.0));
        } else {
            commands.spawn((
                crate::render3d::PlaceholderVisual::prop(
                    Color::srgb(0.92, 0.78, 0.34),
                    Vec2::splat(22.0),
                    22.0,
                ),
                Transform::from_xyz(pos.x, pos.y, 9.0),
                CaravanVisual {
                    caravan_id: caravan.id,
                },
                Name::new(format!("Caravan{}", caravan.id)),
            ));
        }
    }

    for (entity, visual) in existing.iter() {
        if !active_ids.contains(&visual.caravan_id) {
            commands.entity(entity).despawn();
        }
    }
}

fn raid_nearby_caravan_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    player_q: Query<&Transform, With<Player>>,
    caravan_q: Query<(&Transform, &CaravanVisual)>,
    mut caravans: ResMut<ActiveCaravans>,
    mut incidents: MessageWriter<ReputationIncidentEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Exploring || !input.just_pressed(KeyCode::KeyR) {
        return;
    }
    let Ok(player_tf) = player_q.single() else {
        return;
    };
    let player_pos = player_tf.translation.truncate();

    let mut best: Option<(CaravanId, f32)> = None;
    for (tf, visual) in caravan_q.iter() {
        let dist = player_pos.distance(tf.translation.truncate());
        if dist > CARAVAN_RAID_DISTANCE {
            continue;
        }
        let is_better = best.map(|(_, d)| dist < d).unwrap_or(true);
        if is_better {
            best = Some((visual.caravan_id, dist));
        }
    }

    let Some((caravan_id, _)) = best else {
        logs.write(TradeLogEvent {
            message: format!(
                "caravan_raid failed: no caravan within {:.0} units",
                CARAVAN_RAID_DISTANCE
            ),
        });
        return;
    };

    let Some(caravan) = caravans.0.iter_mut().find(|c| c.id == caravan_id) else {
        return;
    };
    let stolen = (caravan.quantity / 3).max(20).min(caravan.quantity);
    caravan.quantity = caravan.quantity.saturating_sub(stolen);
    incidents.write(ReputationIncidentEvent {
        kind: ReputationIncidentKind::CaravanRaided {
            source_city_id: caravan.source_city_id,
            target_city_id: caravan.target_city_id,
            stolen_quantity: stolen,
        },
    });

    logs.write(TradeLogEvent {
        message: format!(
            "caravan_raid success: id={} stolen={} remaining={}",
            caravan_id, stolen, caravan.quantity
        ),
    });
}

fn item_material_total(item: &Equipment, market: &MaterialMarket, use_market: bool) -> u32 {
    item.materials
        .iter()
        .map(|m| {
            let unit = if use_market {
                material_dynamic_unit_price(m.material, market)
            } else {
                m.material as u32
            };
            unit.saturating_mul(m.quantity)
        })
        .sum()
}

fn item_dynamic_base_price(item: &Equipment, market: &MaterialMarket) -> u32 {
    let baseline_material_total = item_material_total(item, market, false);
    if baseline_material_total == 0 {
        return item.base_price;
    }
    let current_material_total = item_material_total(item, market, true);
    let scaled = (item.base_price as u128)
        .saturating_mul(current_material_total as u128)
        .saturating_add((baseline_material_total / 2) as u128)
        / (baseline_material_total as u128);
    (scaled.min(u32::MAX as u128)) as u32
}

fn add_to_inventory(items: &mut Vec<InventoryStack>, item_id: ItemId, qty: StackQty) {
    if let Some(stack) = items.iter_mut().find(|s| s.item_id == item_id) {
        stack.quantity = stack.quantity.saturating_add(qty);
    } else {
        items.push(InventoryStack {
            item_id,
            quantity: qty,
        });
    }
}

fn remove_from_inventory(items: &mut Vec<InventoryStack>, item_id: ItemId, qty: StackQty) -> bool {
    if let Some(index) = items.iter().position(|s| s.item_id == item_id) {
        if items[index].quantity < qty {
            return false;
        }
        items[index].quantity -= qty;
        if items[index].quantity == 0 {
            items.remove(index);
        }
        true
    } else {
        false
    }
}

fn merchant_id_for_region(merchants: &Merchants, region_id: RegionId) -> Option<MerchantId> {
    merchants
        .0
        .iter()
        .find_map(|(id, m)| (m.region_id == region_id).then_some(*id))
}

fn city_for_region(cities: &CityCatalog, region_id: RegionId) -> Option<&City> {
    cities
        .0
        .values()
        .find(|city| city.region_ids.contains(&region_id))
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

fn service_lock_reason_for_region(
    region_id: RegionId,
    cities: &CityCatalog,
    reputation: &ReputationLedger,
    crime: &PlayerCrimeStatus,
    map: &MapTiles,
    centers: &HashMap<CityId, Vec2>,
) -> Option<String> {
    let weights = nearest_city_weights_for_region(region_id, cities, map, centers, 3);
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
        WantedTier::RealmThreat => Some("services blocked: realm threat status".to_string()),
        WantedTier::Outlaw if governor_rep <= -30 || clan_rep <= -35 => Some(format!(
            "services blocked: outlaw with hostile local rule (gov_rep={} clan_rep={})",
            governor_rep, clan_rep
        )),
        WantedTier::Suspect if governor_rep <= -75 && clan_rep <= -80 => Some(format!(
            "services blocked: suspect with severe local hostility (gov_rep={} clan_rep={})",
            governor_rep, clan_rep
        )),
        _ => None,
    }
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
        .map(|(city_id, center)| (*city_id, import_weight_by_distance(region_center.distance(*center))))
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

fn effective_city_modifiers_for_region(
    region_id: RegionId,
    item_id: ItemId,
    merchant_id: Option<MerchantId>,
    cities: &CityCatalog,
    clans: &ClanCatalog,
    reputation: &ReputationLedger,
    map: &MapTiles,
    centers: &HashMap<CityId, Vec2>,
) -> Option<EffectiveCityModifiers> {
    let weights = nearest_city_weights_for_region(region_id, cities, map, centers, 3);
    if weights.is_empty() {
        return None;
    }
    let mut tax = 0.0_f32;
    let mut fee = 0.0_f32;
    let mut instability = 0.0_f32;
    let mut siege = 0.0_f32;
    let mut item_pressure = 0.0_f32;
    let mut policy_tariff = 0.0_f32;
    let mut policy_subsidy = 0.0_f32;
    let mut policy_war_levy = 0.0_f32;
    let mut route_risk = 0.0_f32;
    let mut governor_rep = 0.0_f32;
    let mut clan_rep = 0.0_f32;
    for (city_id, w) in weights {
        let Some(city) = cities.0.get(&city_id) else {
            continue;
        };
        tax += city.tax_rate_bps as f32 * w;
        fee += city.market_fee_bps as f32 * w;
        instability += city_instability_markup_bps(city) as f32 * w;
        siege += city_siege_markup_bps(city) as f32 * w;
        policy_tariff += city.governor_policy.import_tariff_bps as f32 * w;
        policy_subsidy += city.governor_policy.market_subsidy_bps as f32 * w;
        policy_war_levy += city.governor_policy.war_levy_bps as f32 * w;
        item_pressure += city_item_pressure_bps(item_id, city) as f32 * w;
        governor_rep += reputation.get_governor(city.id) as f32 * w;
        clan_rep += reputation.get_clan(&city.clan_name) as f32 * w;
        if !city.trade_route_city_ids.is_empty() {
            let mut local_route_markup = 0.0_f32;
            let mut local_route_count = 0_u32;
            for partner_city_id in &city.trade_route_city_ids {
                let Some(partner_city) = cities.0.get(partner_city_id) else {
                    continue;
                };
                let access = trade_access_between(city, partner_city, clans);
                local_route_markup += trade_access_price_markup_bps(access) as f32;
                local_route_count = local_route_count.saturating_add(1);
            }
            if local_route_count > 0 {
                route_risk += (local_route_markup / local_route_count as f32) * w;
            }
        }
    }
    let merchant_rep = merchant_id.map(|id| reputation.get_merchant(id)).unwrap_or(0);
    let reputation_buy_adjust_bps =
        (-(merchant_rep * 22) - (governor_rep as i32 * 12) - (clan_rep as i32 * 8))
            .clamp(-3200, 3200);
    let reputation_sell_adjust_bps =
        ((merchant_rep * 14) + (governor_rep as i32 * 8) + (clan_rep as i32 * 6)).clamp(-2200, 2200);
    Some(EffectiveCityModifiers {
        tax_rate_bps: tax.max(0.0) as u32,
        market_fee_bps: fee.max(0.0) as u32,
        instability_markup_bps: instability.max(0.0) as u32,
        siege_markup_bps: siege.max(0.0) as u32,
        item_pressure_bps: item_pressure as i32,
        policy_tariff_bps: policy_tariff.max(0.0) as u32,
        policy_subsidy_bps: policy_subsidy.max(0.0) as u32,
        policy_war_levy_bps: policy_war_levy.max(0.0) as u32,
        route_risk_markup_bps: route_risk.max(0.0) as u32,
        reputation_buy_adjust_bps,
        reputation_sell_adjust_bps,
    })
}

fn sync_active_merchant_by_region(
    current_area: Res<CurrentArea>,
    merchants: Res<Merchants>,
    mut active: ResMut<ActiveMerchant>,
) {
    let wanted = merchant_id_for_region(&merchants, current_area.0);
    if active.0 != wanted {
        active.0 = wanted;
    }
}

fn process_buy_item_events(
    mut events: MessageReader<BuyItemEvent>,
    current_area: Res<CurrentArea>,
    cities: Res<CityCatalog>,
    clans: Res<ClanCatalog>,
    reputation: Res<ReputationLedger>,
    crime: Res<PlayerCrimeStatus>,
    map: Res<MapTiles>,
    active_merchant: Res<ActiveMerchant>,
    catalog: Res<ItemCatalog>,
    markets: Res<RegionMaterialMarkets>,
    mut merchants: ResMut<Merchants>,
    mut player_inventory: ResMut<PlayerInventory>,
    mut player_wallet: ResMut<PlayerWallet>,
    mut merchant_coins: ResMut<crate::quests::MerchantCoins>,
    mut rep_events: MessageWriter<ReputationChangeEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if events.is_empty() {
        return;
    }
    // City centroids depend only on the static map + catalog; compute once per
    // batch instead of rescanning the whole map inside every pricing call.
    let centers = city_centroids(&cities, &map);
    for evt in events.read() {
        if evt.quantity == 0 {
            logs.write(TradeLogEvent {
                message: "buy_item failed: quantity must be > 0".to_string(),
            });
            continue;
        }

        let Some(merchant_id) = active_merchant.0 else {
            logs.write(TradeLogEvent {
                message: format!(
                    "buy_item failed: no merchant available in region {}",
                    current_area.0
                ),
            });
            continue;
        };

        let Some(item) = catalog.0.get(&evt.item_id) else {
            logs.write(TradeLogEvent {
                message: format!("buy_item failed: unknown item id {}", evt.item_id),
            });
            continue;
        };

        let Some(merchant_region_id) = merchants.0.get(&merchant_id).map(|m| m.region_id) else {
            logs.write(TradeLogEvent {
                message: format!("buy_item failed: merchant {} not found", merchant_id),
            });
            continue;
        };
        if let Some(reason) = service_lock_reason_for_region(
            merchant_region_id,
            &cities,
            &reputation,
            &crime,
            &map,
            &centers,
        ) {
            logs.write(TradeLogEvent {
                message: format!("buy_item failed: {}", reason),
            });
            continue;
        }

        let Some(market) = markets.0.get(&merchant_region_id) else {
            logs.write(TradeLogEvent {
                message: format!(
                    "buy_item failed: no market for merchant region {}",
                    merchant_region_id
                ),
            });
            continue;
        };

        let modifiers = effective_city_modifiers_for_region(
            merchant_region_id,
            evt.item_id,
            Some(merchant_id),
            &cities,
            &clans,
            &reputation,
            &map,
            &centers,
        );

        let Some(merchant) = merchants.0.get_mut(&merchant_id) else {
            logs.write(TradeLogEvent {
                message: format!("buy_item failed: merchant {} not found", merchant_id),
            });
            continue;
        };

        if !remove_from_inventory(&mut merchant.inventory, evt.item_id, evt.quantity) {
            logs.write(TradeLogEvent {
                message: format!(
                    "buy_item failed: merchant {} lacks {} x {}",
                    merchant.name, evt.quantity, item.name
                ),
            });
            continue;
        }

        let dynamic_base = item_dynamic_base_price(item, market);
        let pricing = buy_price_breakdown(dynamic_base, modifiers);
        let unit_price = pricing.unit_price;
        let total_price = unit_price.saturating_mul(u32::from(evt.quantity));
        let merchant_currency = merchant.currency;
        let player_funds = player_balance(merchant_currency, &player_wallet, &merchant_coins);
        if player_funds < total_price {
            add_to_inventory(&mut merchant.inventory, evt.item_id, evt.quantity);
            logs.write(TradeLogEvent {
                message: format!(
                    "buy_item failed: need {} ({:?}), player has {}",
                    total_price, merchant_currency, player_funds
                ),
            });
            continue;
        }

        // pay_player guarantees true here because we just checked the balance.
        let _ = pay_player(
            merchant_currency,
            total_price,
            &mut player_wallet,
            &mut merchant_coins,
        );
        merchant.coins = merchant.coins.saturating_add(total_price);
        add_to_inventory(&mut player_inventory.0, evt.item_id, evt.quantity);
        rep_events.write(ReputationChangeEvent {
            target: ReputationTarget::Merchant { merchant_id },
            delta: 1,
            reason: "fair_purchase".to_string(),
        });
        if let Some(city) = city_for_region(&cities, merchant.region_id) {
            rep_events.write(ReputationChangeEvent {
                target: ReputationTarget::Governor { city_id: city.id },
                delta: 1,
                reason: "local_trade_activity".to_string(),
            });
            rep_events.write(ReputationChangeEvent {
                target: ReputationTarget::Clan {
                    clan_name: city.clan_name.clone(),
                },
                delta: 1,
                reason: "local_trade_activity".to_string(),
            });
        }

        logs.write(TradeLogEvent {
            message: format!(
                "buy_item ok [{} @ merchant_region {}]: bought {} x {} for {} (unit {}, dyn_base {})",
                merchant.name,
                merchant.region_id,
                evt.quantity,
                item.name,
                total_price,
                unit_price,
                dynamic_base,
            ),
        });
    }
}

fn process_sell_item_events(
    mut events: MessageReader<SellItemEvent>,
    current_area: Res<CurrentArea>,
    cities: Res<CityCatalog>,
    clans: Res<ClanCatalog>,
    reputation: Res<ReputationLedger>,
    crime: Res<PlayerCrimeStatus>,
    map: Res<MapTiles>,
    active_merchant: Res<ActiveMerchant>,
    catalog: Res<ItemCatalog>,
    markets: Res<RegionMaterialMarkets>,
    mut merchants: ResMut<Merchants>,
    mut player_inventory: ResMut<PlayerInventory>,
    mut player_wallet: ResMut<PlayerWallet>,
    mut merchant_coins: ResMut<crate::quests::MerchantCoins>,
    mut rep_events: MessageWriter<ReputationChangeEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if events.is_empty() {
        return;
    }
    // City centroids depend only on the static map + catalog; compute once per
    // batch instead of rescanning the whole map inside every pricing call.
    let centers = city_centroids(&cities, &map);
    for evt in events.read() {
        if evt.quantity == 0 {
            logs.write(TradeLogEvent {
                message: "sell_item failed: quantity must be > 0".to_string(),
            });
            continue;
        }

        let Some(merchant_id) = active_merchant.0 else {
            logs.write(TradeLogEvent {
                message: format!(
                    "sell_item failed: no merchant available in region {}",
                    current_area.0
                ),
            });
            continue;
        };

        let Some(item) = catalog.0.get(&evt.item_id) else {
            logs.write(TradeLogEvent {
                message: format!("sell_item failed: unknown item id {}", evt.item_id),
            });
            continue;
        };

        let Some(merchant_region_id) = merchants.0.get(&merchant_id).map(|m| m.region_id) else {
            logs.write(TradeLogEvent {
                message: format!("sell_item failed: merchant {} not found", merchant_id),
            });
            continue;
        };
        if let Some(reason) = service_lock_reason_for_region(
            merchant_region_id,
            &cities,
            &reputation,
            &crime,
            &map,
            &centers,
        ) {
            logs.write(TradeLogEvent {
                message: format!("sell_item failed: {}", reason),
            });
            continue;
        }

        let Some(market) = markets.0.get(&merchant_region_id) else {
            logs.write(TradeLogEvent {
                message: format!(
                    "sell_item failed: no market for merchant region {}",
                    merchant_region_id
                ),
            });
            continue;
        };

        let modifiers = effective_city_modifiers_for_region(
            merchant_region_id,
            evt.item_id,
            Some(merchant_id),
            &cities,
            &clans,
            &reputation,
            &map,
            &centers,
        );

        let Some(merchant) = merchants.0.get_mut(&merchant_id) else {
            logs.write(TradeLogEvent {
                message: format!("sell_item failed: merchant {} not found", merchant_id),
            });
            continue;
        };

        if !remove_from_inventory(&mut player_inventory.0, evt.item_id, evt.quantity) {
            logs.write(TradeLogEvent {
                message: format!(
                    "sell_item failed: player lacks {} x {}",
                    evt.quantity, item.name
                ),
            });
            continue;
        }

        let dynamic_base = item_dynamic_base_price(item, market);
        let pricing = sell_price_breakdown(dynamic_base, modifiers);
        let unit_price = pricing.unit_price;
        let total_price = unit_price.saturating_mul(u32::from(evt.quantity));
        if merchant.coins < total_price {
            add_to_inventory(&mut player_inventory.0, evt.item_id, evt.quantity);
            logs.write(TradeLogEvent {
                message: format!(
                    "sell_item failed: merchant {} cannot pay {}, has {}",
                    merchant.name, total_price, merchant.coins
                ),
            });
            continue;
        }

        merchant.coins -= total_price;
        let merchant_currency = merchant.currency;
        refund_player(
            merchant_currency,
            total_price,
            &mut player_wallet,
            &mut merchant_coins,
        );
        add_to_inventory(&mut merchant.inventory, evt.item_id, evt.quantity);
        rep_events.write(ReputationChangeEvent {
            target: ReputationTarget::Merchant { merchant_id },
            delta: 1,
            reason: "profitable_sale".to_string(),
        });
        if let Some(city) = city_for_region(&cities, merchant.region_id) {
            rep_events.write(ReputationChangeEvent {
                target: ReputationTarget::Governor { city_id: city.id },
                delta: 1,
                reason: "market_turnover".to_string(),
            });
        }

        logs.write(TradeLogEvent {
            message: format!(
                "sell_item ok [{} @ merchant_region {}]: sold {} x {} for {} (unit {}, dyn_base {})",
                merchant.name,
                merchant.region_id,
                evt.quantity,
                item.name,
                total_price,
                unit_price,
                dynamic_base
            ),
        });
    }
}

fn log_trade_events(mut events: MessageReader<TradeLogEvent>) {
    for evt in events.read() {
        info!("{}", evt.message);
    }
}

fn toggle_shop_ui_hotkey(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    current_area: Res<CurrentArea>,
    cities: Res<CityCatalog>,
    reputation: Res<ReputationLedger>,
    crime: Res<PlayerCrimeStatus>,
    map: Res<MapTiles>,
    active_merchant: Res<ActiveMerchant>,
    player_q: Query<&Transform, With<Player>>,
    merchant_q: Query<(&Transform, &MerchantNpc)>,
    mut shop_ui: ResMut<ShopUiState>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if !input.just_pressed(KeyCode::KeyF) {
        return;
    }

    if shop_ui.open || game_state.0 == Game_State::Shopping {
        shop_ui.open = false;
        if game_state.0 == Game_State::Shopping {
            game_state.0 = Game_State::Exploring;
        }
        return;
    }

    if game_state.0 != Game_State::Exploring {
        return;
    }

    if active_merchant.0.is_none() {
        logs.write(TradeLogEvent {
            message: "shop_ui: no merchant available in current region".to_string(),
        });
        return;
    }
    let centers = city_centroids(&cities, &map);
    if let Some(reason) = service_lock_reason_for_region(
        current_area.0,
        &cities,
        &reputation,
        &crime,
        &map,
        &centers,
    ) {
        logs.write(TradeLogEvent {
            message: format!("shop_ui: {}", reason),
        });
        return;
    }

    let Some(active_id) = active_merchant.0 else {
        return;
    };
    let Ok(player_tf) = player_q.single() else {
        logs.write(TradeLogEvent {
            message: "shop_ui: player transform unavailable".to_string(),
        });
        return;
    };
    let player_pos = player_tf.translation.truncate();
    let mut can_open = false;
    for (merchant_tf, merchant_npc) in merchant_q.iter() {
        if merchant_npc.merchant_id != active_id {
            continue;
        }
        if player_pos.distance(merchant_tf.translation.truncate()) <= SHOP_OPEN_DISTANCE {
            can_open = true;
            break;
        }
    }
    if !can_open {
        logs.write(TradeLogEvent {
            message: format!(
                "shop_ui: get closer to merchant (max {:.0} units)",
                SHOP_OPEN_DISTANCE
            ),
        });
        return;
    }

    shop_ui.open = true;
    game_state.0 = Game_State::Shopping;
}

fn ensure_shop_ui_root(
    mut commands: Commands,
    game_state: Res<GameState>,
    mut shop_ui: ResMut<ShopUiState>,
    roots: Query<Entity, With<ShopUiRoot>>,
) {
    if !shop_ui.open || game_state.0 != Game_State::Shopping {
        if let Some(entity) = shop_ui.root.take() {
            commands.entity(entity).despawn();
        }
        return;
    }

    if let Some(root) = shop_ui.root {
        if roots.get(root).is_ok() {
            return;
        }
        shop_ui.root = None;
    }

    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(8.0),
                right: Val::Percent(8.0),
                top: Val::Percent(8.0),
                bottom: Val::Percent(8.0),
                padding: UiRect::all(Val::Px(spacing::LG)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::LG)),
                ..default()
            },
            BackgroundColor(palette::BG_PANEL),
            BorderColor::all(palette::BORDER_ACCENT),
            Text::new("Shop UI"),
            TextFont {
                font_size: font_size::BODY_LG,
                ..default()
            },
            TextColor(palette::TEXT_PRIMARY),
            ShopUiRoot,
        ))
        .id();
    shop_ui.root = Some(root);
}

fn stack_entries_with_items<'a>(
    stacks: &'a [InventoryStack],
    catalog: &'a ItemCatalog,
) -> Vec<(&'a InventoryStack, Option<&'a Equipment>)> {
    stacks
        .iter()
        .filter(|s| s.quantity > 0)
        .map(|s| (s, catalog.0.get(&s.item_id)))
        .collect()
}

fn handle_shop_ui_input(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    mut shop_ui: ResMut<ShopUiState>,
    active_merchant: Res<ActiveMerchant>,
    merchants: Res<Merchants>,
    player_inventory: Res<PlayerInventory>,
    catalog: Res<ItemCatalog>,
    mut buy: MessageWriter<BuyItemEvent>,
    mut sell: MessageWriter<SellItemEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if !shop_ui.open || game_state.0 != Game_State::Shopping {
        return;
    }

    if input.just_pressed(KeyCode::Escape) {
        shop_ui.open = false;
        game_state.0 = Game_State::Exploring;
        return;
    }

    if input.just_pressed(KeyCode::KeyA) || input.just_pressed(KeyCode::ArrowLeft) {
        shop_ui.focus = ShopFocus::Buy;
    }
    if input.just_pressed(KeyCode::KeyD) || input.just_pressed(KeyCode::ArrowRight) {
        shop_ui.focus = ShopFocus::Sell;
    }

    if input.just_pressed(KeyCode::KeyQ) && shop_ui.quantity > 1 {
        shop_ui.quantity -= 1;
    }
    if input.just_pressed(KeyCode::KeyE) {
        shop_ui.quantity = shop_ui.quantity.saturating_add(1).min(999);
    }

    let Some(merchant_id) = active_merchant.0 else {
        shop_ui.open = false;
        return;
    };
    let Some(merchant) = merchants.0.get(&merchant_id) else {
        shop_ui.open = false;
        return;
    };

    let buy_entries = stack_entries_with_items(&merchant.inventory, &catalog);
    let sell_entries = stack_entries_with_items(&player_inventory.0, &catalog);

    if !buy_entries.is_empty() && shop_ui.selected_buy >= buy_entries.len() {
        shop_ui.selected_buy = buy_entries.len() - 1;
    }
    if !sell_entries.is_empty() && shop_ui.selected_sell >= sell_entries.len() {
        shop_ui.selected_sell = sell_entries.len() - 1;
    }

    if input.just_pressed(KeyCode::ArrowDown) || input.just_pressed(KeyCode::KeyS) {
        match shop_ui.focus {
            ShopFocus::Buy => {
                if !buy_entries.is_empty() {
                    shop_ui.selected_buy = (shop_ui.selected_buy + 1) % buy_entries.len();
                }
            }
            ShopFocus::Sell => {
                if !sell_entries.is_empty() {
                    shop_ui.selected_sell = (shop_ui.selected_sell + 1) % sell_entries.len();
                }
            }
        }
    }

    if input.just_pressed(KeyCode::ArrowUp) || input.just_pressed(KeyCode::KeyW) {
        match shop_ui.focus {
            ShopFocus::Buy => {
                if !buy_entries.is_empty() {
                    shop_ui.selected_buy = if shop_ui.selected_buy == 0 {
                        buy_entries.len() - 1
                    } else {
                        shop_ui.selected_buy - 1
                    };
                }
            }
            ShopFocus::Sell => {
                if !sell_entries.is_empty() {
                    shop_ui.selected_sell = if shop_ui.selected_sell == 0 {
                        sell_entries.len() - 1
                    } else {
                        shop_ui.selected_sell - 1
                    };
                }
            }
        }
    }

    let trigger_buy = input.just_pressed(KeyCode::Enter)
        && shop_ui.focus == ShopFocus::Buy
        || input.just_pressed(KeyCode::KeyB);
    let trigger_sell = input.just_pressed(KeyCode::Enter)
        && shop_ui.focus == ShopFocus::Sell
        || input.just_pressed(KeyCode::KeyN);

    if trigger_buy {
        if let Some((stack, _)) = buy_entries.get(shop_ui.selected_buy) {
            let qty = shop_ui.quantity.min(stack.quantity).max(1);
            buy.write(BuyItemEvent {
                item_id: stack.item_id,
                quantity: qty,
            });
        } else {
            logs.write(TradeLogEvent {
                message: "shop_ui: no buyable item selected".to_string(),
            });
        }
    }

    if trigger_sell {
        if let Some((stack, _)) = sell_entries.get(shop_ui.selected_sell) {
            let qty = shop_ui.quantity.min(stack.quantity).max(1);
            sell.write(SellItemEvent {
                item_id: stack.item_id,
                quantity: qty,
            });
        } else {
            logs.write(TradeLogEvent {
                message: "shop_ui: no sellable item selected".to_string(),
            });
        }
    }
}

fn rob_merchant_store_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    shop_ui: Res<ShopUiState>,
    active_merchant: Res<ActiveMerchant>,
    mut merchants: ResMut<Merchants>,
    mut player_inventory: ResMut<PlayerInventory>,
    mut player_wallet: ResMut<PlayerWallet>,
    mut incidents: MessageWriter<ReputationIncidentEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Shopping || !shop_ui.open || !input.just_pressed(KeyCode::KeyT) {
        return;
    }
    let Some(merchant_id) = active_merchant.0 else {
        return;
    };
    let Some(merchant) = merchants.0.get_mut(&merchant_id) else {
        return;
    };

    let stolen_coins = (merchant.coins.0 / 18).clamp(120, 3_500);
    merchant.coins = merchant.coins.saturating_sub(stolen_coins);
    player_wallet.coins = player_wallet.coins.saturating_add(stolen_coins);

    let mut stolen_item = None;
    if let Some(stack) = merchant.inventory.iter_mut().find(|s| s.quantity > 0) {
        stack.quantity = stack.quantity.saturating_sub(1);
        add_to_inventory(&mut player_inventory.0, stack.item_id, 1);
        stolen_item = Some(stack.item_id);
    }
    merchant.inventory.retain(|s| s.quantity > 0);

    incidents.write(ReputationIncidentEvent {
        kind: ReputationIncidentKind::MerchantStoreRobbed {
            merchant_id,
            region_id: merchant.region_id,
            stolen_value: stolen_coins.saturating_add(1_000),
        },
    });

    logs.write(TradeLogEvent {
        message: format!(
            "merchant_store_robbed merchant={} stolen_coins={} stolen_item={:?}",
            merchant.name, stolen_coins, stolen_item
        ),
    });
}

fn update_shop_ui_text(
    shop_ui: Res<ShopUiState>,
    game_state: Res<GameState>,
    current_area: Res<CurrentArea>,
    cities: Res<CityCatalog>,
    clans: Res<ClanCatalog>,
    reputation: Res<ReputationLedger>,
    map: Res<MapTiles>,
    active_merchant: Res<ActiveMerchant>,
    merchants: Res<Merchants>,
    player_inventory: Res<PlayerInventory>,
    player_wallet: Res<PlayerWallet>,
    catalog: Res<ItemCatalog>,
    markets: Res<RegionMaterialMarkets>,
    mut roots: Query<&mut Text, With<ShopUiRoot>>,
) {
    if !shop_ui.open || game_state.0 != Game_State::Shopping {
        return;
    }

    let Some(root) = shop_ui.root else {
        return;
    };
    let Ok(mut text) = roots.get_mut(root) else {
        return;
    };

    let mut out = String::new();
    out.push_str("=== SHOP === (F or ESC close)\n");
    out.push_str("Controls: W/S=select  A/D=buy-sell panel  Q/E=qty-+  ENTER=confirm  B=buy  N=sell  T=rob store  G=castle assault\n");
    let Some(merchant_id) = active_merchant.0 else {
        out.push_str(&format!(
            "Region: {} | Player coins: {} | Qty: {} | Focus: {}\n",
            current_area.0,
            player_wallet.coins,
            shop_ui.quantity,
            match shop_ui.focus {
                ShopFocus::Buy => "Buy",
                ShopFocus::Sell => "Sell",
            }
        ));
        out.push_str("City: Frontier region (no city authority assigned)\n");
        out.push_str("------------------------------------------------------------\n");
        out.push_str("No merchant in this region.\n");
        text.0 = out;
        return;
    };
    let Some(merchant) = merchants.0.get(&merchant_id) else {
        out.push_str("Active merchant missing.\n");
        text.0 = out;
        return;
    };

    let market = markets.0.get(&merchant.region_id);
    let city = city_for_region(&cities, merchant.region_id);
    // Computed once for the whole panel; every per-item pricing call below reuses it.
    let centers = city_centroids(&cities, &map);
    out.push_str(&format!(
        "Area region: {} | Merchant region: {} | Player coins: {} | Qty: {} | Focus: {}\n",
        current_area.0,
        merchant.region_id,
        player_wallet.coins,
        shop_ui.quantity,
        match shop_ui.focus {
            ShopFocus::Buy => "Buy",
            ShopFocus::Sell => "Sell",
        }
    ));
    if let Some(city) = city {
        out.push_str(&format!(
            "City: {} ({}, {}) | Governor: {} {} of Clan {}\n",
            city.name,
            city.settlement_tier.label(),
            city.governance_type.label(),
            city.governor_title,
            city.governor_name,
            city.clan_name
        ));
        out.push_str(&format!(
            "Authority: {} | under_siege {} (pressure {})\n",
            city.authority.state.label(),
            city.authority.under_siege,
            city.authority.siege_pressure_bps
        ));
        out.push_str(&format!(
            "Reputation: governor {} | clan {}\n",
            reputation.get_governor(city.id),
            reputation.get_clan(&city.clan_name)
        ));
    } else {
        out.push_str("City: Frontier region (no city authority assigned)\n");
    }
    out.push_str("------------------------------------------------------------\n");
    out.push_str(&format!(
        "Merchant: {} (coins {}, region {})\n",
        merchant.name, merchant.coins, merchant.region_id
    ));
    out.push_str(&format!(
        "Reputation: merchant {}\n",
        reputation.get_merchant(merchant.id)
    ));
    out.push_str("------------------------------------------------------------\n");
    let ui_region_id = merchant.region_id;

    let buy_entries = stack_entries_with_items(&merchant.inventory, &catalog);
    out.push_str("[BUY FROM MERCHANT]\n");
    if buy_entries.is_empty() {
        out.push_str("  (empty)\n");
    } else {
        for (idx, (stack, item_opt)) in buy_entries.iter().enumerate() {
            let marker = if shop_ui.focus == ShopFocus::Buy && idx == shop_ui.selected_buy {
                ">"
            } else {
                " "
            };
            let (name, unit) = if let Some(item) = item_opt {
                let dyn_base = market
                    .map(|m| item_dynamic_base_price(item, m))
                    .unwrap_or(item.base_price);
                let modifiers =
                    effective_city_modifiers_for_region(
                        ui_region_id,
                        stack.item_id,
                        Some(merchant.id),
                        &cities,
                        &clans,
                        &reputation,
                        &map,
                        &centers,
                    );
                (
                    item.name.as_str(),
                    buy_price_breakdown(dyn_base, modifiers).unit_price,
                )
            } else {
                ("UnknownItem", 0)
            };
            out.push_str(&format!(
                "{} [{}] {:20} stock {:3} | price {:6}\n",
                marker, stack.item_id, name, stack.quantity, unit
            ));
        }
    }

    let sell_entries = stack_entries_with_items(&player_inventory.0, &catalog);
    out.push_str("\n[SELL TO MERCHANT]\n");
    if sell_entries.is_empty() {
        out.push_str("  (empty)\n");
    } else {
        for (idx, (stack, item_opt)) in sell_entries.iter().enumerate() {
            let marker = if shop_ui.focus == ShopFocus::Sell && idx == shop_ui.selected_sell {
                ">"
            } else {
                " "
            };
            let (name, unit) = if let Some(item) = item_opt {
                let dyn_base = market
                    .map(|m| item_dynamic_base_price(item, m))
                    .unwrap_or(item.base_price);
                let modifiers =
                    effective_city_modifiers_for_region(
                        ui_region_id,
                        stack.item_id,
                        Some(merchant.id),
                        &cities,
                        &clans,
                        &reputation,
                        &map,
                        &centers,
                    );
                (
                    item.name.as_str(),
                    sell_price_breakdown(dyn_base, modifiers).unit_price,
                )
            } else {
                ("UnknownItem", 0)
            };
            out.push_str(&format!(
                "{} [{}] {:20} stock {:3} | price {:6}\n",
                marker, stack.item_id, name, stack.quantity, unit
            ));
        }
    }

    out.push_str("\n------------------------------------------------------------\n");
    match shop_ui.focus {
        ShopFocus::Buy => {
            if let Some((stack, item_opt)) = buy_entries.get(shop_ui.selected_buy) {
                if let Some(item) = item_opt {
                    let dyn_base = market
                        .map(|m| item_dynamic_base_price(item, m))
                        .unwrap_or(item.base_price);
                    let modifiers = effective_city_modifiers_for_region(
                        ui_region_id,
                        stack.item_id,
                        Some(merchant.id),
                        &cities,
                        &clans,
                        &reputation,
                        &map,
                        &centers,
                    );
                    let pricing = buy_price_breakdown(dyn_base, modifiers);
                    let unit = pricing.unit_price;
                    let qty = shop_ui.quantity.min(stack.quantity).max(1);
                    let total = unit.saturating_mul(u32::from(qty));
                    let can_pay = player_wallet.coins >= total;
                    out.push_str(&format!(
                        "Selected BUY: {} x{} | price {} | total {} | can_pay={}\n",
                        item.name, qty, unit, total, can_pay
                    ));
                    out.push_str(&format!(
                        "  Breakdown(bps): base {} +tax {} +fee {} +instability {} +item {} +tariff {} -subsidy {} +war_levy {} +route_risk {} +rep {} = {} | dyn_base {}\n",
                        pricing.base_bps,
                        pricing.tax_bps,
                        pricing.market_fee_bps,
                        pricing.instability_bps,
                        pricing.item_pressure_bps,
                        pricing.policy_tariff_bps,
                        pricing.policy_subsidy_bps,
                        pricing.policy_war_levy_bps,
                        pricing.route_risk_bps,
                        pricing.reputation_bps,
                        pricing.final_bps,
                        pricing.dynamic_base
                    ));
                }
            }
        }
        ShopFocus::Sell => {
            if let Some((stack, item_opt)) = sell_entries.get(shop_ui.selected_sell) {
                if let Some(item) = item_opt {
                    let dyn_base = market
                        .map(|m| item_dynamic_base_price(item, m))
                        .unwrap_or(item.base_price);
                    let modifiers = effective_city_modifiers_for_region(
                        ui_region_id,
                        stack.item_id,
                        Some(merchant.id),
                        &cities,
                        &clans,
                        &reputation,
                        &map,
                        &centers,
                    );
                    let pricing = sell_price_breakdown(dyn_base, modifiers);
                    let unit = pricing.unit_price;
                    let qty = shop_ui.quantity.min(stack.quantity).max(1);
                    let total = unit.saturating_mul(u32::from(qty));
                    let merchant_can_pay = merchant.coins >= total;
                    out.push_str(&format!(
                        "Selected SELL: {} x{} | price {} | total {} | merchant_can_pay={}\n",
                        item.name, qty, unit, total, merchant_can_pay
                    ));
                    out.push_str(&format!(
                        "  Breakdown(bps): base {} tax {} fee {} instability {} siege {} item {} tariff {} subsidy {} war_levy {} route_risk {} rep {} = {} | dyn_base {}\n",
                        pricing.base_bps,
                        pricing.tax_bps,
                        pricing.market_fee_bps,
                        pricing.instability_bps,
                        pricing.siege_bps,
                        pricing.item_pressure_bps,
                        pricing.policy_tariff_bps,
                        pricing.policy_subsidy_bps,
                        pricing.policy_war_levy_bps,
                        pricing.route_risk_bps,
                        pricing.reputation_bps,
                        pricing.final_bps,
                        pricing.dynamic_base
                    ));
                }
            }
        }
    }

    // Avoid marking the Text dirty (and triggering UI re-layout) every frame
    // when the rendered contents are unchanged.
    if text.0 != out {
        text.0 = out;
    }
}

fn debug_trade_hotkeys(
    input: Res<ButtonInput<KeyCode>>,
    current_area: Res<CurrentArea>,
    cities: Res<CityCatalog>,
    active_merchant: Res<ActiveMerchant>,
    catalog: Res<ItemCatalog>,
    mut markets: ResMut<RegionMaterialMarkets>,
    merchants: Res<Merchants>,
    player_inventory: Res<PlayerInventory>,
    player_wallet: Res<PlayerWallet>,
    mut buy: MessageWriter<BuyItemEvent>,
    mut sell: MessageWriter<SellItemEvent>,
    mut rep_events: MessageWriter<ReputationChangeEvent>,
    mut city_material_events: MessageWriter<CityMaterialMarketEvent>,
    mut city_item_events: MessageWriter<CityItemMarketEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if input.just_pressed(KeyCode::F6) {
        if let Some(city) = city_for_region(&cities, current_area.0) {
            city_material_events.write(CityMaterialMarketEvent {
                city_id: city.id,
                material: ItemMaterial::SilverSteelIngot,
                supply_delta: -35,
                demand_delta: 40,
                stockpile_delta: -420,
                duration_ticks: 4,
                reason: "war_draft".to_string(),
            });
            city_item_events.write(CityItemMarketEvent {
                city_id: city.id,
                item_id: 5001,
                supply_delta: -28,
                demand_delta: 46,
                duration_ticks: 4,
                reason: "officer_rearmament".to_string(),
            });
        }
    }
    if input.just_pressed(KeyCode::F7) {
        buy.write(BuyItemEvent {
            item_id: 5001,
            quantity: 1,
        });
    }
    if input.just_pressed(KeyCode::F8) {
        sell.write(SellItemEvent {
            item_id: 5001,
            quantity: 1,
        });
    }
    if input.just_pressed(KeyCode::F10) {
        if let Some(city) = city_for_region(&cities, current_area.0) {
            rep_events.write(ReputationChangeEvent {
                target: ReputationTarget::Governor { city_id: city.id },
                delta: 10,
                reason: "debug_helped_city".to_string(),
            });
            rep_events.write(ReputationChangeEvent {
                target: ReputationTarget::Clan {
                    clan_name: city.clan_name.clone(),
                },
                delta: 6,
                reason: "debug_helped_city".to_string(),
            });
        }
    }
    if input.just_pressed(KeyCode::F11) {
        if let Some(mid) = active_merchant.0 {
            rep_events.write(ReputationChangeEvent {
                target: ReputationTarget::Merchant { merchant_id: mid },
                delta: -8,
                reason: "debug_bad_deal".to_string(),
            });
        }
    }

    let region_id = current_area.0;

    let region_market = markets
        .0
        .entry(region_id)
        .or_insert_with(MaterialMarket::default);

    if input.just_pressed(KeyCode::F9) {
        let city_summary = if let Some(city) = city_for_region(&cities, current_area.0) {
            format!(
                "{} [{} | {}] gov:{} {} pop:{} tax:{}bps sec:{} stab:{} state:{} siege:{}({})",
                city.name,
                city.settlement_tier.label(),
                city.governance_type.label(),
                city.governor_title,
                city.governor_name,
                city.population,
                city.tax_rate_bps,
                city.security,
                city.stability,
                city.authority.state.label(),
                city.authority.under_siege,
                city.authority.siege_pressure_bps
            )
        } else {
            "none".to_string()
        };

        let merchant_summary = if let Some(mid) = active_merchant.0 {
            if let Some(m) = merchants.0.get(&mid) {
                let inv = if m.inventory.is_empty() {
                    "(empty)".to_string()
                } else {
                    m.inventory
                        .iter()
                        .map(|s| {
                            let name = catalog
                                .0
                                .get(&s.item_id)
                                .map(|i| i.name.as_str())
                                .unwrap_or("UnknownItem");
                            format!("{}x {}(id:{})", s.quantity, name, s.item_id)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                format!(
                    "{}(id:{}) coins:{} inv:{}",
                    m.name, m.id, m.coins, inv
                )
            } else {
                "missing".to_string()
            }
        } else {
            "none".to_string()
        };

        let player_inv = if player_inventory.0.is_empty() {
            "(empty)".to_string()
        } else {
            player_inventory
                .0
                .iter()
                .map(|s| {
                    let name = catalog
                        .0
                        .get(&s.item_id)
                        .map(|i| i.name.as_str())
                        .unwrap_or("UnknownItem");
                    format!("{}x {}(id:{})", s.quantity, name, s.item_id)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        let silver = region_market
            .0
            .get(&ItemMaterial::SilverSteelIngot)
            .cloned()
            .unwrap_or_default();
        logs.write(TradeLogEvent {
            message: format!(
                "shop_status | area_region:{} | city:{} | player coins:{} inv:{} | merchant:{} | silversteel(s={}, d={}, unit={})",
                current_area.0,
                city_summary,
                player_wallet.coins,
                player_inv,
                merchant_summary,
                silver.supply,
                silver.demand,
                material_dynamic_unit_price(ItemMaterial::SilverSteelIngot, region_market)
            ),
        });
    }

    if input.just_pressed(KeyCode::F10) {
        if let Some(entry) = region_market.0.get_mut(&ItemMaterial::SilverSteelIngot) {
            entry.demand = entry.demand.saturating_add(10);
            let demand_now = entry.demand;
            let unit =
                material_dynamic_unit_price(ItemMaterial::SilverSteelIngot, region_market);
            logs.write(TradeLogEvent {
                message: format!(
                    "market tweak [region {}]: SilverSteel demand -> {} (unit {})",
                    current_area.0, demand_now, unit
                ),
            });
        }
    }
    if input.just_pressed(KeyCode::F11) {
        if let Some(entry) = region_market.0.get_mut(&ItemMaterial::SilverSteelIngot) {
            entry.supply = entry.supply.saturating_add(10);
            let supply_now = entry.supply;
            let unit =
                material_dynamic_unit_price(ItemMaterial::SilverSteelIngot, region_market);
            logs.write(TradeLogEvent {
                message: format!(
                    "market tweak [region {}]: SilverSteel supply -> {} (unit {})",
                    current_area.0, supply_now, unit
                ),
            });
        }
    }
}
