use std::fs;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::characters::{CharacterKind, SelectedParty};
use crate::city_data::{CityCatalog, ClanCatalog};
use crate::core::{GameState, Game_State, Player, PlayerMapPosition, Position, Timestamp};
use crate::economy::{ActiveCaravans, CaravanClock, PlayerInventory, PlayerWallet};
use crate::governance::{
    CastleAssaultClock, CoupChainState, CoupPreparationProgress, GlobalPunishmentState,
    GovernanceClock, GovernorPolicyClock, PlayerCrimeStatus, ReputationLedger,
};
use crate::map::{CurrentArea, MapSelection, MapTiles};
use crate::money::Money;
use crate::quests::{QuestFlags, QuestLog};
use crate::skill_tree::PartyProgression;
use crate::story_flags::StoryFlags;

/// The slice of run state that lives in plain resources (party roster, quest
/// progress, story/quest flags, skill progression, inventory, wallet). Bundled
/// into one [`SystemParam`] so `handle_save_requests` stays under Bevy's 16-arg
/// system limit while still reading/writing all of it.
#[derive(SystemParam)]
pub struct RunStateResources<'w, 's> {
    pub party: ResMut<'w, SelectedParty>,
    pub story_flags: ResMut<'w, StoryFlags>,
    pub quest_flags: ResMut<'w, QuestFlags>,
    pub quest_log: ResMut<'w, QuestLog>,
    pub progression: ResMut<'w, PartyProgression>,
    pub inventory: ResMut<'w, PlayerInventory>,
    pub wallet: ResMut<'w, PlayerWallet>,
    // Party-respawn control: a load despawns the live party and resets these so
    // `world::spawn_party` rebuilds it from the loaded roster at the saved spot.
    pub spawned: ResMut<'w, crate::world::PartySpawned>,
    pub pending_respawn: ResMut<'w, crate::world::PendingPartyRespawn>,
    pub party_entities: Query<'w, 's, Entity, Or<(With<Player>, With<crate::battle::WorldAlly>)>>,
    pub party_equipment: ResMut<'w, crate::equipment::PartyEquipment>,
    pub commands: Commands<'w, 's>,
}

const SAVE_DIR: &str = "saves";

#[derive(Clone, Copy, Debug)]
pub enum SaveSlot {
    Auto,
    Slot1,
    Slot2,
    Slot3,
}

impl SaveSlot {
    fn file_name(self) -> &'static str {
        match self {
            SaveSlot::Auto => "auto.ron",
            SaveSlot::Slot1 => "slot_1.ron",
            SaveSlot::Slot2 => "slot_2.ron",
            SaveSlot::Slot3 => "slot_3.ron",
        }
    }

    fn path(self) -> String {
        format!("{}/{}", SAVE_DIR, self.file_name())
    }
}

/// The most-recently-written save slot that exists on disk, or `None` if there
/// are no saves yet. Drives the title screen's "Continue" button.
pub fn latest_save_slot() -> Option<SaveSlot> {
    [SaveSlot::Auto, SaveSlot::Slot1, SaveSlot::Slot2, SaveSlot::Slot3]
        .into_iter()
        .filter_map(|slot| {
            fs::metadata(slot.path())
                .and_then(|m| m.modified())
                .ok()
                .map(|t| (slot, t))
        })
        .max_by_key(|(_, t)| *t)
        .map(|(slot, _)| slot)
}

#[derive(Clone, Copy, Debug)]
pub enum SaveAction {
    Save,
    Load,
}

#[derive(Clone, Copy, Debug, Message)]
pub struct SaveRequest {
    pub action: SaveAction,
    pub slot: SaveSlot,
}

#[derive(Resource)]
pub struct AutoSaveSettings {
    pub enabled: bool,
    pub interval_seconds: f32,
    pub timer: Timer,
}

impl Default for AutoSaveSettings {
    fn default() -> Self {
        let interval_seconds = 180.0;
        Self {
            enabled: true,
            interval_seconds,
            timer: Timer::from_seconds(interval_seconds, TimerMode::Repeating),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SaveVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<Vec3> for SaveVec3 {
    fn from(v: Vec3) -> Self {
        Self {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}

impl From<SaveVec3> for Vec3 {
    fn from(v: SaveVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveData {
    pub player_world: SaveVec3,
    pub player_tile: Position,
    pub map_selection: Position,
    pub current_area: u16,
    pub timestamp: u32,
    pub map_tiles: MapTiles,
    #[serde(default)]
    pub city_catalog: CityCatalog,
    #[serde(default)]
    pub clan_catalog: ClanCatalog,
    #[serde(default)]
    pub active_caravans: ActiveCaravans,
    #[serde(default)]
    pub caravan_clock: CaravanClock,
    #[serde(default)]
    pub reputation_ledger: ReputationLedger,
    #[serde(default)]
    pub player_crime_status: PlayerCrimeStatus,
    #[serde(default)]
    pub global_punishment_state: GlobalPunishmentState,
    #[serde(default)]
    pub coup_chain_state: CoupChainState,
    #[serde(default)]
    pub governance_clock: GovernanceClock,
    #[serde(default)]
    pub castle_assault_clock: CastleAssaultClock,
    #[serde(default)]
    pub governor_policy_clock: GovernorPolicyClock,
    #[serde(default)]
    pub coup_preparation_progress: CoupPreparationProgress,
    // --- Run progress (party / quests / flags / skills / inventory) ---------
    #[serde(default)]
    pub selected_party: Vec<CharacterKind>,
    #[serde(default)]
    pub story_flags: Vec<String>,
    #[serde(default)]
    pub quest_flags: Vec<String>,
    #[serde(default)]
    pub quest_log: QuestLog,
    #[serde(default)]
    pub party_progression: PartyProgression,
    #[serde(default)]
    pub player_inventory: PlayerInventory,
    #[serde(default)]
    pub wallet_coins: u32,
    #[serde(default)]
    pub party_equipment: crate::equipment::PartyEquipment,
}

pub fn save_game_hotkeys(
    input: Res<ButtonInput<KeyCode>>,
    mut requests: ResMut<Messages<SaveRequest>>,
) {
    if input.just_pressed(KeyCode::F5) {
        requests.write(SaveRequest {
            action: SaveAction::Save,
            slot: SaveSlot::Slot1,
        });
    }
    if input.just_pressed(KeyCode::F9) {
        requests.write(SaveRequest {
            action: SaveAction::Load,
            slot: SaveSlot::Slot1,
        });
    }
}

pub fn handle_save_requests(
    mut requests: ResMut<Messages<SaveRequest>>,
    mut game_state: ResMut<GameState>,
    mut map: ResMut<MapTiles>,
    mut selection: ResMut<MapSelection>,
    mut map_position: ResMut<PlayerMapPosition>,
    mut current_area: ResMut<CurrentArea>,
    mut timestamp: ResMut<Timestamp>,
    mut city_catalog: ResMut<CityCatalog>,
    mut clan_catalog: ResMut<ClanCatalog>,
    mut active_caravans: ResMut<ActiveCaravans>,
    mut caravan_clock: ResMut<CaravanClock>,
    mut reputation_ledger: ResMut<ReputationLedger>,
    mut governance_state: ParamSet<(
        ResMut<PlayerCrimeStatus>,
        ResMut<GlobalPunishmentState>,
        ResMut<CoupChainState>,
        ResMut<GovernanceClock>,
        ResMut<CastleAssaultClock>,
        ResMut<GovernorPolicyClock>,
        ResMut<CoupPreparationProgress>,
    )>,
    mut player_q: Query<&mut Transform, With<Player>>,
    mut camera_q: Query<&mut Transform, (With<crate::core::MainCamera>, Without<Player>)>,
    mut run: RunStateResources,
) {
    for req in requests.drain() {
        match req.action {
            SaveAction::Save => {
                if game_state.0 != Game_State::Exploring && game_state.0 != Game_State::MapOpen {
                    continue;
                }
                let Ok(player_tf) = player_q.single_mut() else {
                    warn!("save_game: player transform not found");
                    continue;
                };
                let player_crime_status = (*governance_state.p0()).clone();
                let global_punishment_state = (*governance_state.p1()).clone();
                let coup_chain_state = (*governance_state.p2()).clone();
                let governance_clock = (*governance_state.p3()).clone();
                let castle_assault_clock = (*governance_state.p4()).clone();
                let governor_policy_clock = (*governance_state.p5()).clone();
                let coup_preparation_progress = (*governance_state.p6()).clone();
                let data = SaveData {
                    player_world: SaveVec3::from(player_tf.translation),
                    player_tile: map_position.0,
                    map_selection: selection.0,
                    current_area: current_area.0,
                    timestamp: timestamp.0,
                    map_tiles: map.clone(),
                    city_catalog: city_catalog.clone(),
                    clan_catalog: clan_catalog.clone(),
                    active_caravans: active_caravans.clone(),
                    caravan_clock: caravan_clock.clone(),
                    reputation_ledger: reputation_ledger.clone(),
                    player_crime_status,
                    global_punishment_state,
                    coup_chain_state,
                    governance_clock,
                    castle_assault_clock,
                    governor_policy_clock,
                    coup_preparation_progress,
                    selected_party: run.party.0.clone(),
                    story_flags: run.story_flags.iter_set_names().map(String::from).collect(),
                    quest_flags: run.quest_flags.0.iter().cloned().collect(),
                    quest_log: run.quest_log.clone(),
                    party_progression: run.progression.clone(),
                    player_inventory: run.inventory.clone(),
                    wallet_coins: run.wallet.coins.0,
                    party_equipment: run.party_equipment.clone(),
                };
                if let Err(e) = write_save(req.slot, &data) {
                    warn!("save_game: {}", e);
                } else {
                    info!("Saved game to {}", req.slot.path());
                }
            }
            SaveAction::Load => {
                let Ok(data) = read_save(req.slot) else {
                    warn!("load_game: save file not found at {}", req.slot.path());
                    continue;
                };
                map.tiles = data.map_tiles.tiles;
                normalize_legacy_tile_image_paths(&mut map);
                selection.0 = data.map_selection;
                map_position.0 = data.player_tile;
                current_area.0 = data.current_area;
                timestamp.0 = data.timestamp;
                *city_catalog = data.city_catalog;
                *clan_catalog = data.clan_catalog;
                *active_caravans = data.active_caravans;
                *caravan_clock = data.caravan_clock;
                *reputation_ledger = data.reputation_ledger;
                *governance_state.p0() = data.player_crime_status;
                *governance_state.p1() = data.global_punishment_state;
                *governance_state.p2() = data.coup_chain_state;
                *governance_state.p3() = data.governance_clock;
                *governance_state.p4() = data.castle_assault_clock;
                *governance_state.p5() = data.governor_policy_clock;
                *governance_state.p6() = data.coup_preparation_progress;

                // Restore run progress. The party roster is only overwritten
                // when the save actually carried one (older saves leave it
                // empty); the already-spawned leader entity stays put — full
                // respawn-from-save is a separate concern (see spawn_party).
                if !data.selected_party.is_empty() {
                    run.party.0 = data.selected_party;
                }
                *run.story_flags = StoryFlags::from_names(data.story_flags);
                run.quest_flags.0 = data.quest_flags.into_iter().collect();
                *run.quest_log = data.quest_log;
                *run.progression = data.party_progression;
                *run.inventory = data.player_inventory;
                run.wallet.coins = Money(data.wallet_coins);
                *run.party_equipment = data.party_equipment;

                // Rebuild the party from the loaded roster: despawn whoever is
                // on the field (the default party from a fresh boot, or the live
                // party mid-game) and have `spawn_party` repopulate it at the
                // saved location next frame. This is what makes "Continue" /
                // loading actually restore the saved roster rather than the
                // default one (the old `Local` spawn-once guard couldn't reset).
                for entity in run.party_entities.iter() {
                    run.commands.entity(entity).despawn();
                }
                run.spawned.0 = false;
                run.pending_respawn.0 = Some(Vec3::from(data.player_world));

                // (Player repositioning is handled by the respawn above; only
                // the camera needs an immediate snap to the loaded location.)
                if let Ok(mut cam_tf) = camera_q.single_mut() {
                    let loaded = Vec3::from(data.player_world);
                    cam_tf.translation =
                        Vec3::new(loaded.x, loaded.y, 0.0) + crate::render3d::iso_camera_offset();
                }

                game_state.0 = Game_State::Exploring;
                info!("Loaded game from {}", req.slot.path());
            }
        }
    }
}

pub fn autosave_tick(
    time: Res<Time>,
    mut settings: ResMut<AutoSaveSettings>,
    game_state: Res<GameState>,
    mut requests: ResMut<Messages<SaveRequest>>,
) {
    if !settings.enabled {
        return;
    }
    if game_state.0 != Game_State::Exploring {
        return;
    }

    settings.timer.tick(time.delta());
    if settings.timer.just_finished() {
        requests.write(SaveRequest {
            action: SaveAction::Save,
            slot: SaveSlot::Auto,
        });
    }
}

fn write_save(slot: SaveSlot, data: &SaveData) -> Result<(), String> {
    if let Err(e) = fs::create_dir_all(SAVE_DIR) {
        return Err(format!("failed to create save directory: {}", e));
    }
    let path = slot.path();
    // Compact RON: smaller files, faster I/O. Saves are large (10s of MB of map tiles)
    // so we skip pretty-printing — savings are significant on disk and parse time.
    let serialized = ron::ser::to_string(data).map_err(|e| e.to_string())?;
    fs::write(&path, serialized).map_err(|e| format!("failed to write save file: {}", e))?;
    Ok(())
}

fn read_save(slot: SaveSlot) -> Result<SaveData, String> {
    let path = slot.path();
    let contents = fs::read_to_string(&path).map_err(|_| "save file not found".to_string())?;
    ron::de::from_str::<SaveData>(&contents).map_err(|e| format!("failed to parse save: {}", e))
}

fn normalize_legacy_tile_image_paths(map: &mut MapTiles) {
    for row in &mut map.tiles {
        for tile in row {
            if tile.image_path == "dot.webp" {
                tile.image_path = "dot.png".to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal-but-non-trivial SaveData so the round-trip exercises the
    /// run-progress fields (party / flags / inventory / equipment) that older
    /// saves on disk don't carry.
    fn sample_save() -> SaveData {
        // A 1x1 map is enough to prove tile (de)serialization round-trips.
        let map_tiles = MapTiles {
            tiles: vec![vec![crate::map::MapTile {
                time: 1,
                location_id: 0,
                type_id: 0,
                event_ids: vec![1000, 2000],
                items_id: None,
                image_path: "dot.png".to_string(),
            }]],
        };
        SaveData {
            player_world: SaveVec3 { x: 1.0, y: 2.0, z: 3.0 },
            player_tile: Position { x: 4, y: 5 },
            map_selection: Position { x: 6, y: 7 },
            current_area: 2,
            timestamp: 42,
            map_tiles,
            city_catalog: CityCatalog::default(),
            clan_catalog: ClanCatalog::default(),
            active_caravans: ActiveCaravans::default(),
            caravan_clock: CaravanClock::default(),
            reputation_ledger: ReputationLedger::default(),
            player_crime_status: PlayerCrimeStatus::default(),
            global_punishment_state: GlobalPunishmentState::default(),
            coup_chain_state: CoupChainState::default(),
            governance_clock: GovernanceClock::default(),
            castle_assault_clock: CastleAssaultClock::default(),
            governor_policy_clock: GovernorPolicyClock::default(),
            coup_preparation_progress: CoupPreparationProgress::default(),
            selected_party: vec![CharacterKind::Rina, CharacterKind::Sayaka],
            story_flags: vec!["met_elder".to_string(), "saw_omen".to_string()],
            quest_flags: vec!["q_started".to_string()],
            quest_log: QuestLog::default(),
            party_progression: PartyProgression::default(),
            player_inventory: PlayerInventory::default(),
            wallet_coins: 1234,
            party_equipment: crate::equipment::PartyEquipment::default(),
        }
    }

    /// Every field that goes into a save must survive a RON write→read cycle.
    /// RON is picky about map key types and floats, so this guards against a
    /// field type silently breaking serialization for the whole save.
    #[test]
    fn savedata_round_trips_through_ron() {
        let data = sample_save();
        let serialized = ron::ser::to_string(&data).expect("SaveData must serialize to RON");
        let restored: SaveData =
            ron::de::from_str(&serialized).expect("SaveData must deserialize from RON");

        assert_eq!(restored.player_world.x, data.player_world.x);
        assert_eq!(restored.player_world.y, data.player_world.y);
        assert_eq!(restored.player_world.z, data.player_world.z);
        assert_eq!(restored.player_tile, data.player_tile);
        assert_eq!(restored.map_selection, data.map_selection);
        assert_eq!(restored.current_area, data.current_area);
        assert_eq!(restored.timestamp, data.timestamp);
        assert_eq!(restored.map_tiles.tiles.len(), data.map_tiles.tiles.len());
        assert_eq!(restored.selected_party, data.selected_party);
        assert_eq!(restored.wallet_coins, data.wallet_coins);
        // Flag ordering isn't guaranteed (HashSet origin), so compare as sets.
        let restored_story: std::collections::HashSet<_> = restored.story_flags.into_iter().collect();
        let expected_story: std::collections::HashSet<_> = data.story_flags.into_iter().collect();
        assert_eq!(restored_story, expected_story);
    }

    /// Fields added after a save was written must fall back to their defaults
    /// rather than failing the whole parse. Mirrors loading a pre-run-progress
    /// save with today's `SaveData`.
    #[test]
    fn old_saves_without_run_progress_still_parse() {
        // A save body that stops at the governance fields — i.e. has none of the
        // run-progress fields that were added later.
        let legacy = "(player_world:(x:0.0,y:0.0,z:0.0),player_tile:(x:0,y:0),\
            map_selection:(x:0,y:0),current_area:0,timestamp:0,map_tiles:(tiles:[]))";
        let restored: SaveData =
            ron::de::from_str(legacy).expect("legacy save (missing new fields) must still parse");
        assert!(restored.selected_party.is_empty());
        assert!(restored.story_flags.is_empty());
        assert_eq!(restored.wallet_coins, 0);
    }

    /// If a real save exists in the working tree, it must parse with the current
    /// schema (this is what "Continue" does on launch). Skips when absent so a
    /// fresh checkout / CI without saves still passes.
    #[test]
    fn on_disk_saves_parse_with_current_schema() {
        for slot in [SaveSlot::Auto, SaveSlot::Slot1, SaveSlot::Slot2, SaveSlot::Slot3] {
            let path = slot.path();
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            ron::de::from_str::<SaveData>(&contents)
                .unwrap_or_else(|e| panic!("on-disk save {path} failed to parse: {e}"));
        }
    }
}
