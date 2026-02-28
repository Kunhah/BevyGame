use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat_plugin::{Equipment, ItemMaterial, ItemMaterialCost};
use crate::map::CurrentArea;

const BUY_MARKUP_BPS: u32 = 12000; // 120% of dynamic base
const SELL_MARKDOWN_BPS: u32 = 7000; // 70% of dynamic base
const MIN_MARKET_FACTOR_BPS: u32 = 5000; // 50%
const MAX_MARKET_FACTOR_BPS: u32 = 20000; // 200%

pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemCatalog>()
            .init_resource::<RegionMaterialMarkets>()
            .init_resource::<Merchants>()
            .init_resource::<ActiveMerchant>()
            .init_resource::<PlayerInventory>()
            .init_resource::<PlayerWallet>()
            .insert_resource(Messages::<BuyItemEvent>::default())
            .insert_resource(Messages::<SellItemEvent>::default())
            .insert_resource(Messages::<TradeLogEvent>::default())
            .add_systems(Update, sync_active_merchant_by_region)
            .add_systems(Update, debug_trade_hotkeys)
            .add_systems(Update, process_buy_item_events)
            .add_systems(Update, process_sell_item_events)
            .add_systems(Update, log_trade_events);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialMarketEntry {
    pub supply: u32,
    pub demand: u32,
}

impl Default for MaterialMarketEntry {
    fn default() -> Self {
        Self {
            supply: 100,
            demand: 100,
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
pub struct RegionMaterialMarkets(pub HashMap<u32, MaterialMarket>);

impl Default for RegionMaterialMarkets {
    fn default() -> Self {
        let mut by_region = HashMap::new();

        let mut region0 = MaterialMarket::default();
        if let Some(m) = region0.0.get_mut(&ItemMaterial::SilverSteelIngot) {
            m.supply = 130;
            m.demand = 95;
        }
        by_region.insert(0, region0);

        let mut region1 = MaterialMarket::default();
        if let Some(m) = region1.0.get_mut(&ItemMaterial::SilverSteelIngot) {
            m.supply = 80;
            m.demand = 125;
        }
        if let Some(m) = region1.0.get_mut(&ItemMaterial::CrystalDust) {
            m.supply = 70;
            m.demand = 140;
        }
        by_region.insert(1, region1);

        Self(by_region)
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct ItemCatalog(pub HashMap<u32, Equipment>);

impl Default for ItemCatalog {
    fn default() -> Self {
        let mut map = HashMap::new();
        map.insert(
            5001,
            Equipment {
                id: 5001,
                name: "Silversteel Blade".to_string(),
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
        Self(map)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryStack {
    pub item_id: u32,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Merchant {
    pub id: u32,
    pub name: String,
    pub region_id: u32,
    pub coins: u32,
    pub inventory: Vec<InventoryStack>,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct Merchants(pub HashMap<u32, Merchant>);

impl Default for Merchants {
    fn default() -> Self {
        let mut map = HashMap::new();
        map.insert(
            1,
            Merchant {
                id: 1,
                name: "Aster of Greenford".to_string(),
                region_id: 0,
                coins: 220_000,
                inventory: vec![InventoryStack {
                    item_id: 5001,
                    quantity: 5,
                }],
            },
        );
        map.insert(
            2,
            Merchant {
                id: 2,
                name: "Rath of Ironpass".to_string(),
                region_id: 1,
                coins: 280_000,
                inventory: vec![InventoryStack {
                    item_id: 5001,
                    quantity: 8,
                }],
            },
        );
        Self(map)
    }
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActiveMerchant(pub Option<u32>);

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerInventory(pub Vec<InventoryStack>);

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct PlayerWallet {
    pub coins: u32,
}

impl Default for PlayerWallet {
    fn default() -> Self {
        Self { coins: 100_000 }
    }
}

#[derive(Debug, Clone, Message)]
pub struct BuyItemEvent {
    pub item_id: u32,
    pub quantity: u32,
}

#[derive(Debug, Clone, Message)]
pub struct SellItemEvent {
    pub item_id: u32,
    pub quantity: u32,
}

#[derive(Debug, Clone, Message)]
pub struct TradeLogEvent {
    pub message: String,
}

fn price_with_bps(base: u32, bps: u32) -> u32 {
    base.saturating_mul(bps).saturating_add(9_999) / 10_000
}

fn material_dynamic_unit_price(material: ItemMaterial, market: &MaterialMarket) -> u32 {
    let base = material as u32;
    let entry = market.0.get(&material).cloned().unwrap_or_default();
    let supply = entry.supply.max(1);
    let raw_factor_bps = entry.demand.saturating_mul(10_000) / supply;
    let factor_bps = raw_factor_bps.clamp(MIN_MARKET_FACTOR_BPS, MAX_MARKET_FACTOR_BPS);
    price_with_bps(base, factor_bps)
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

fn add_to_inventory(items: &mut Vec<InventoryStack>, item_id: u32, qty: u32) {
    if let Some(stack) = items.iter_mut().find(|s| s.item_id == item_id) {
        stack.quantity = stack.quantity.saturating_add(qty);
    } else {
        items.push(InventoryStack {
            item_id,
            quantity: qty,
        });
    }
}

fn remove_from_inventory(items: &mut Vec<InventoryStack>, item_id: u32, qty: u32) -> bool {
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

fn merchant_id_for_region(merchants: &Merchants, region_id: u32) -> Option<u32> {
    merchants
        .0
        .iter()
        .find_map(|(id, m)| (m.region_id == region_id).then_some(*id))
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
    active_merchant: Res<ActiveMerchant>,
    catalog: Res<ItemCatalog>,
    markets: Res<RegionMaterialMarkets>,
    mut merchants: ResMut<Merchants>,
    mut player_inventory: ResMut<PlayerInventory>,
    mut player_wallet: ResMut<PlayerWallet>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
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

        let Some(market) = markets.0.get(&current_area.0) else {
            logs.write(TradeLogEvent {
                message: format!("buy_item failed: no market for region {}", current_area.0),
            });
            continue;
        };

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
        let unit_price = price_with_bps(dynamic_base, BUY_MARKUP_BPS);
        let total_price = unit_price.saturating_mul(evt.quantity);
        if player_wallet.coins < total_price {
            add_to_inventory(&mut merchant.inventory, evt.item_id, evt.quantity);
            logs.write(TradeLogEvent {
                message: format!(
                    "buy_item failed: need {}, player has {}",
                    total_price, player_wallet.coins
                ),
            });
            continue;
        }

        player_wallet.coins -= total_price;
        merchant.coins = merchant.coins.saturating_add(total_price);
        add_to_inventory(&mut player_inventory.0, evt.item_id, evt.quantity);

        logs.write(TradeLogEvent {
            message: format!(
                "buy_item ok [{} @ region {}]: bought {} x {} for {} (unit {}, dyn_base {})",
                merchant.name,
                current_area.0,
                evt.quantity,
                item.name,
                total_price,
                unit_price,
                dynamic_base
            ),
        });
    }
}

fn process_sell_item_events(
    mut events: MessageReader<SellItemEvent>,
    current_area: Res<CurrentArea>,
    active_merchant: Res<ActiveMerchant>,
    catalog: Res<ItemCatalog>,
    markets: Res<RegionMaterialMarkets>,
    mut merchants: ResMut<Merchants>,
    mut player_inventory: ResMut<PlayerInventory>,
    mut player_wallet: ResMut<PlayerWallet>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
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

        let Some(market) = markets.0.get(&current_area.0) else {
            logs.write(TradeLogEvent {
                message: format!("sell_item failed: no market for region {}", current_area.0),
            });
            continue;
        };

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
        let unit_price = price_with_bps(dynamic_base, SELL_MARKDOWN_BPS);
        let total_price = unit_price.saturating_mul(evt.quantity);
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
        player_wallet.coins = player_wallet.coins.saturating_add(total_price);
        add_to_inventory(&mut merchant.inventory, evt.item_id, evt.quantity);

        logs.write(TradeLogEvent {
            message: format!(
                "sell_item ok [{} @ region {}]: sold {} x {} for {} (unit {}, dyn_base {})",
                merchant.name,
                current_area.0,
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

fn debug_trade_hotkeys(
    input: Res<ButtonInput<KeyCode>>,
    current_area: Res<CurrentArea>,
    active_merchant: Res<ActiveMerchant>,
    catalog: Res<ItemCatalog>,
    mut markets: ResMut<RegionMaterialMarkets>,
    merchants: Res<Merchants>,
    player_inventory: Res<PlayerInventory>,
    player_wallet: Res<PlayerWallet>,
    mut buy: MessageWriter<BuyItemEvent>,
    mut sell: MessageWriter<SellItemEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
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

    let region_market = markets
        .0
        .entry(current_area.0)
        .or_insert_with(MaterialMarket::default);

    if input.just_pressed(KeyCode::F9) {
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
                "shop_status | region:{} | player coins:{} inv:{} | merchant:{} | silversteel(s={}, d={}, unit={})",
                current_area.0,
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
