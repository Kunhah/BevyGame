use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat_plugin::ItemMaterial;
use crate::money::Money;

const ECONOMY_DATA_PATH: &str = "assets/data/economy.ron";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EconomyDataFile {
    #[serde(default)]
    cities: Vec<City>,
    #[serde(default)]
    clans: Vec<ClanProfile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeAccess {
    Open,
    Taxed,
    Blockaded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SettlementTier {
    Hamlet,
    Village,
    Town,
    CastleTown,
    GreatCity,
}

impl SettlementTier {
    pub fn label(self) -> &'static str {
        match self {
            SettlementTier::Hamlet => "Hamlet",
            SettlementTier::Village => "Village",
            SettlementTier::Town => "Town",
            SettlementTier::CastleTown => "Castle Town",
            SettlementTier::GreatCity => "Great City",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GovernanceType {
    ClanEstate,
    TempleDomain,
    ShogunateSeat,
}

impl GovernanceType {
    pub fn label(self) -> &'static str {
        match self {
            GovernanceType::ClanEstate => "Clan Estate",
            GovernanceType::TempleDomain => "Temple Domain",
            GovernanceType::ShogunateSeat => "Shogunate Seat",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CityAuthorityState {
    Stable,
    #[serde(rename = "UnderSiege")]
    LegacyUnderSiege,
    Interregnum,
    SuccessorInstalled,
    MartialLaw,
    CollapsedAuthority,
}

impl CityAuthorityState {
    pub fn label(self) -> &'static str {
        match self {
            CityAuthorityState::Stable => "Stable",
            CityAuthorityState::LegacyUnderSiege => "Under Siege (Legacy)",
            CityAuthorityState::Interregnum => "Interregnum",
            CityAuthorityState::SuccessorInstalled => "Successor Installed",
            CityAuthorityState::MartialLaw => "Martial Law",
            CityAuthorityState::CollapsedAuthority => "Collapsed Authority",
        }
    }
}

impl Default for CityAuthorityState {
    fn default() -> Self {
        Self::Stable
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SuccessionSource {
    NamedHeir,
    ClanElder,
    OverlordAppointee,
    TempleRegent,
    MilitaryCommander,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessorCandidate {
    pub id: u32,
    pub name: String,
    pub title: String,
    pub clan_name: String,
    pub legitimacy: u16, // 0..=1000
    #[serde(default = "default_true")]
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessionRule {
    #[serde(default = "default_succession_order")]
    pub source_order: Vec<SuccessionSource>,
    #[serde(default = "default_interregnum_ticks")]
    pub interregnum_ticks: u16,
    #[serde(default = "default_leadership_immunity_ticks")]
    pub leadership_immunity_ticks: u16,
}

impl Default for SuccessionRule {
    fn default() -> Self {
        Self {
            source_order: default_succession_order(),
            interregnum_ticks: default_interregnum_ticks(),
            leadership_immunity_ticks: default_leadership_immunity_ticks(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CityAuthorityData {
    #[serde(default)]
    pub state: CityAuthorityState,
    #[serde(default)]
    pub under_siege: bool,
    #[serde(default)]
    pub siege_pressure_bps: u16,
    #[serde(default)]
    pub succession_crisis_level: u8,
    #[serde(default)]
    pub governor_deaths_total: u16,
    #[serde(default)]
    pub leadership_immunity_remaining_ticks: u16,
    #[serde(default)]
    pub current_governor_alive: bool,
    #[serde(default)]
    pub active_successor: Option<SuccessorCandidate>,
    #[serde(default)]
    pub successor_candidates: Vec<SuccessorCandidate>,
    #[serde(default)]
    pub succession_rule: SuccessionRule,
}

impl Default for CityAuthorityData {
    fn default() -> Self {
        Self {
            state: CityAuthorityState::Stable,
            under_siege: false,
            siege_pressure_bps: 0,
            succession_crisis_level: 0,
            governor_deaths_total: 0,
            leadership_immunity_remaining_ticks: 0,
            current_governor_alive: true,
            active_successor: None,
            successor_candidates: Vec::new(),
            succession_rule: SuccessionRule::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialMarketEffect {
    pub material: ItemMaterial,
    pub supply_delta: i16,
    pub demand_delta: i16,
    pub remaining_ticks: u16,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemMarketEffect {
    pub item_id: u16,
    pub supply_delta: i16,
    pub demand_delta: i16,
    pub remaining_ticks: u16,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct City {
    pub id: u16,
    pub name: String,
    pub settlement_tier: SettlementTier,
    pub governance_type: GovernanceType,
    pub governor_name: String,
    pub governor_title: String,
    pub clan_name: String,
    #[serde(default)]
    pub overlord_clan_name: Option<String>,
    pub region_ids: Vec<u16>,
    pub trade_route_city_ids: Vec<u16>,
    #[serde(default)]
    pub trade_distance_by_city_id: HashMap<u16, u16>,
    pub population: u32,
    pub prosperity: u16, // 0..=1000
    pub security: u16,   // 0..=1000
    pub stability: u16,  // 0..=1000
    pub tax_rate_bps: u16,
    pub treasury_coins: Money,
    pub garrison_strength: u16, // abstract military strength
    pub primary_material_outputs: HashMap<ItemMaterial, u16>,
    pub crafted_item_outputs: HashMap<u16, u16>,
    pub import_material_priority: HashMap<ItemMaterial, u16>,
    pub material_stockpile: HashMap<ItemMaterial, u32>,
    pub market_fee_bps: u16, // city-level trade friction/fee
    pub unrest_risk_bps: u16,
    #[serde(default)]
    pub governor_policy: GovernorPolicy,
    #[serde(default)]
    pub authority: CityAuthorityData,
    pub notable_landmarks: Vec<String>,
    #[serde(default)]
    pub active_material_market_effects: Vec<MaterialMarketEffect>,
    #[serde(default)]
    pub active_item_market_effects: Vec<ItemMarketEffect>,
}

fn default_true() -> bool {
    true
}

fn default_succession_order() -> Vec<SuccessionSource> {
    vec![
        SuccessionSource::NamedHeir,
        SuccessionSource::ClanElder,
        SuccessionSource::OverlordAppointee,
        SuccessionSource::TempleRegent,
        SuccessionSource::MilitaryCommander,
    ]
}

fn default_interregnum_ticks() -> u16 {
    8
}

fn default_leadership_immunity_ticks() -> u16 {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernorPolicy {
    #[serde(default)]
    pub import_tariff_bps: u16,
    #[serde(default)]
    pub market_subsidy_bps: u16,
    #[serde(default)]
    pub war_levy_bps: u16,
    #[serde(default)]
    pub rationing_bps: u16,
    #[serde(default)]
    pub export_ban_materials: Vec<ItemMaterial>,
}

impl Default for GovernorPolicy {
    fn default() -> Self {
        Self {
            import_tariff_bps: 0,
            market_subsidy_bps: 0,
            war_levy_bps: 0,
            rationing_bps: 0,
            export_ban_materials: Vec::new(),
        }
    }
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct CityCatalog(pub HashMap<u16, City>);

impl Default for CityCatalog {
    fn default() -> Self {
        if let Some(file_data) = load_economy_data_file() {
            let mut cities = HashMap::new();
            for city in file_data.cities {
                cities.insert(city.id, city);
            }
            if !cities.is_empty() {
                info!("Loaded city data from {}", ECONOMY_DATA_PATH);
                return Self(cities);
            }
            warn!(
                "City data file {} had no cities; using built-in defaults",
                ECONOMY_DATA_PATH
            );
        }
        Self(seed_default_cities())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClanRelationStatus {
    Allied,
    Neutral,
    Rival,
    War,
}

impl ClanRelationStatus {
    pub fn trade_access(self) -> TradeAccess {
        match self {
            ClanRelationStatus::Allied | ClanRelationStatus::Neutral => TradeAccess::Open,
            ClanRelationStatus::Rival => TradeAccess::Taxed,
            ClanRelationStatus::War => TradeAccess::Blockaded,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClanProfile {
    pub name: String,
    pub mon: String,
    pub ruler_name: String,
    pub ruler_title: String,
    pub military_strength: u16,
    pub war_weariness: u16, // 0..=1000
    #[serde(default)]
    pub relation_by_clan: HashMap<String, ClanRelationStatus>,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct ClanCatalog(pub HashMap<String, ClanProfile>);

impl Default for ClanCatalog {
    fn default() -> Self {
        if let Some(file_data) = load_economy_data_file() {
            let mut clans = HashMap::new();
            for clan in file_data.clans {
                clans.insert(clan.name.clone(), clan);
            }
            if !clans.is_empty() {
                info!("Loaded clan data from {}", ECONOMY_DATA_PATH);
                return Self(clans);
            }
            warn!(
                "Clan data file {} had no clans; using built-in defaults",
                ECONOMY_DATA_PATH
            );
        }
        Self(seed_default_clans())
    }
}

fn load_economy_data_file() -> Option<EconomyDataFile> {
    let contents = match fs::read_to_string(ECONOMY_DATA_PATH) {
        Ok(s) => s,
        Err(err) => {
            warn!("Failed to open {}: {}", ECONOMY_DATA_PATH, err);
            return None;
        }
    };
    match ron::de::from_str::<EconomyDataFile>(&contents) {
        Ok(data) => Some(data),
        Err(err) => {
            warn!("Failed to parse {}: {}", ECONOMY_DATA_PATH, err);
            None
        }
    }
}

fn seed_default_cities() -> HashMap<u16, City> {
    let mut cities = HashMap::new();

    let mut village_materials = HashMap::new();
    village_materials.insert(ItemMaterial::OakWood, 140);
    village_materials.insert(ItemMaterial::Leather, 90);
    let mut village_crafted = HashMap::new();
    village_crafted.insert(5003, 30);
    village_crafted.insert(5005, 18);
    let mut village_imports = HashMap::new();
    village_imports.insert(ItemMaterial::SilverSteelIngot, 100);
    village_imports.insert(ItemMaterial::CrystalDust, 120);
    let mut village_stockpile = HashMap::new();
    village_stockpile.insert(ItemMaterial::OakWood, 4_800);
    village_stockpile.insert(ItemMaterial::Leather, 3_100);
    let mut village_distances = HashMap::new();
    village_distances.insert(200, 320);
    let village_successors = vec![
        SuccessorCandidate {
            id: 1_001,
            name: "Mizuno Kenta".to_string(),
            title: "Heir Steward".to_string(),
            clan_name: "Mizuno".to_string(),
            legitimacy: 820,
            alive: true,
        },
        SuccessorCandidate {
            id: 1_002,
            name: "Sayo of the Reed Shrine".to_string(),
            title: "Temple Regent".to_string(),
            clan_name: "Mizuno".to_string(),
            legitimacy: 620,
            alive: true,
        },
    ];

    cities.insert(
        100,
        City {
            id: 100,
            name: "Greenford".to_string(),
            settlement_tier: SettlementTier::Village,
            governance_type: GovernanceType::ClanEstate,
            governor_name: "Mizuno Aoi".to_string(),
            governor_title: "Jito".to_string(),
            clan_name: "Mizuno".to_string(),
            overlord_clan_name: Some("Takeda".to_string()),
            region_ids: vec![0],
            trade_route_city_ids: vec![200],
            trade_distance_by_city_id: village_distances,
            population: 2_400,
            prosperity: 540,
            security: 620,
            stability: 710,
            tax_rate_bps: 850,
            treasury_coins: Money(180_000),
            garrison_strength: 160,
            primary_material_outputs: village_materials,
            crafted_item_outputs: village_crafted,
            import_material_priority: village_imports,
            material_stockpile: village_stockpile,
            market_fee_bps: 180,
            unrest_risk_bps: 120,
            governor_policy: GovernorPolicy {
                import_tariff_bps: 60,
                market_subsidy_bps: 0,
                war_levy_bps: 0,
                rationing_bps: 0,
                export_ban_materials: Vec::new(),
            },
            authority: CityAuthorityData {
                state: CityAuthorityState::Stable,
                under_siege: false,
                siege_pressure_bps: 0,
                succession_crisis_level: 0,
                governor_deaths_total: 0,
                leadership_immunity_remaining_ticks: 0,
                current_governor_alive: true,
                active_successor: None,
                successor_candidates: village_successors,
                succession_rule: SuccessionRule::default(),
            },
            notable_landmarks: vec![
                "Moon-Reed Rice Terraces".to_string(),
                "Shrine of the Wind Bell".to_string(),
            ],
            active_material_market_effects: Vec::new(),
            active_item_market_effects: Vec::new(),
        },
    );

    let mut castle_materials = HashMap::new();
    castle_materials.insert(ItemMaterial::SilverSteelIngot, 170);
    castle_materials.insert(ItemMaterial::CrystalDust, 140);
    let mut castle_crafted = HashMap::new();
    castle_crafted.insert(5001, 42);
    castle_crafted.insert(5004, 36);
    let mut castle_imports = HashMap::new();
    castle_imports.insert(ItemMaterial::OakWood, 110);
    castle_imports.insert(ItemMaterial::Cloth, 90);
    let mut castle_stockpile = HashMap::new();
    castle_stockpile.insert(ItemMaterial::SilverSteelIngot, 7_200);
    castle_stockpile.insert(ItemMaterial::CrystalDust, 5_600);
    let mut castle_distances = HashMap::new();
    castle_distances.insert(100, 320);
    let castle_successors = vec![
        SuccessorCandidate {
            id: 2_001,
            name: "Takeda Masaru".to_string(),
            title: "Karo".to_string(),
            clan_name: "Takeda".to_string(),
            legitimacy: 880,
            alive: true,
        },
        SuccessorCandidate {
            id: 2_002,
            name: "Hoshina Reiko".to_string(),
            title: "Castle Castellan".to_string(),
            clan_name: "Takeda".to_string(),
            legitimacy: 700,
            alive: true,
        },
    ];

    cities.insert(
        200,
        City {
            id: 200,
            name: "Ironpass".to_string(),
            settlement_tier: SettlementTier::CastleTown,
            governance_type: GovernanceType::ShogunateSeat,
            governor_name: "Takeda Ren".to_string(),
            governor_title: "Daimyo".to_string(),
            clan_name: "Takeda".to_string(),
            overlord_clan_name: None,
            region_ids: vec![1],
            trade_route_city_ids: vec![100],
            trade_distance_by_city_id: castle_distances,
            population: 11_500,
            prosperity: 760,
            security: 790,
            stability: 680,
            tax_rate_bps: 1120,
            treasury_coins: Money(640_000),
            garrison_strength: 510,
            primary_material_outputs: castle_materials,
            crafted_item_outputs: castle_crafted,
            import_material_priority: castle_imports,
            material_stockpile: castle_stockpile,
            market_fee_bps: 260,
            unrest_risk_bps: 180,
            governor_policy: GovernorPolicy {
                import_tariff_bps: 140,
                market_subsidy_bps: 0,
                war_levy_bps: 90,
                rationing_bps: 80,
                export_ban_materials: vec![ItemMaterial::SilverSteelIngot],
            },
            authority: CityAuthorityData {
                state: CityAuthorityState::Stable,
                under_siege: false,
                siege_pressure_bps: 0,
                succession_crisis_level: 0,
                governor_deaths_total: 0,
                leadership_immunity_remaining_ticks: 0,
                current_governor_alive: true,
                active_successor: None,
                successor_candidates: castle_successors,
                succession_rule: SuccessionRule::default(),
            },
            notable_landmarks: vec![
                "Kurogane Gate Keep".to_string(),
                "Foundry of Five Fires".to_string(),
            ],
            active_material_market_effects: Vec::new(),
            active_item_market_effects: Vec::new(),
        },
    );

    cities
}

fn seed_default_clans() -> HashMap<String, ClanProfile> {
    let mut clans = HashMap::new();
    let mut mizuno_rel = HashMap::new();
    mizuno_rel.insert("Takeda".to_string(), ClanRelationStatus::Rival);
    let mut takeda_rel = HashMap::new();
    takeda_rel.insert("Mizuno".to_string(), ClanRelationStatus::Rival);
    clans.insert(
        "Mizuno".to_string(),
        ClanProfile {
            name: "Mizuno".to_string(),
            mon: "three_streams".to_string(),
            ruler_name: "Mizuno Aoi".to_string(),
            ruler_title: "Jito".to_string(),
            military_strength: 420,
            war_weariness: 210,
            relation_by_clan: mizuno_rel,
        },
    );
    clans.insert(
        "Takeda".to_string(),
        ClanProfile {
            name: "Takeda".to_string(),
            mon: "mountain_hawk".to_string(),
            ruler_name: "Takeda Ren".to_string(),
            ruler_title: "Daimyo".to_string(),
            military_strength: 860,
            war_weariness: 140,
            relation_by_clan: takeda_rel,
        },
    );
    clans
}
