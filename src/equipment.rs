//! Persistent, interactive party equipment + items.
//!
//! Before this, `Equipment` had stat-application wired in combat
//! (`combat_plugin::apply_equipment_bonuses_system`) but nothing ever spawned or
//! equipped gear, item definitions were hard-coded, and there was no overworld
//! model of who-wears-what. This module closes that:
//!
//! * **Data file** `assets/data/items.ron` overlays the equipment
//!   (`ItemCatalog`) and consumable (`InventoryItemCatalog`) catalogs at
//!   startup — add or override items there by id; hard-coded defaults remain for
//!   any id not listed (so crafting recipes keep their materials).
//! * **`PartyEquipment`** — a persistent, saved map of `CharacterKind` → the
//!   equipment item-ids that member wears. Mirrors `PartyProgression`.
//! * At combat spawn, `apply_party_equipment_system` (gated by
//!   [`EquipmentPending`]) spawns each equipped item as a child of the combatant
//!   and slots it, so the existing bonus system applies its stats. The party's
//!   owned consumables (`PlayerInventory`) are handed to the leader so what you
//!   own is what you can use in battle.
//! * [`equip_item`] / [`unequip_item`] move gear between the owned pool
//!   (`PlayerInventory`) and `PartyEquipment`, driving the character-sheet UI.

use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::characters::CharacterKind;
use crate::combat_plugin::{
    Equipment, EquipmentLoadout, EquipmentType, Inventory, InventoryItemCatalog,
    InventoryItemDefinition, InventoryItemKind, PlayerControlled,
};
use crate::economy::{ItemCatalog, PlayerInventory};

const ITEMS_PATH: &str = "assets/data/items.ron";

/// Persistent record of which equipment item-ids each party member wears.
/// Replayed onto combatants each battle and saved with the run.
#[derive(Resource, Default, Debug, Clone, Serialize, Deserialize)]
pub struct PartyEquipment(pub HashMap<CharacterKind, Vec<u16>>);

/// Marker on a freshly-spawned protagonist combatant: its persisted equipment
/// (and, for the leader, the party's consumables) still need replaying. Removed
/// by [`apply_party_equipment_system`]. Inserted alongside `ProgressionPending`.
#[derive(Component, Debug, Default)]
pub struct EquipmentPending;

pub struct EquipmentPlugin;

impl Plugin for EquipmentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PartyEquipment>()
            .add_systems(Startup, overlay_item_catalogs)
            .add_systems(Update, apply_party_equipment_system);
    }
}

// ---------------------------------------------------------------------------
// Data file: overlay the catalogs
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct ItemFile {
    #[serde(default)]
    equipment: Vec<Equipment>,
    #[serde(default)]
    consumables: Vec<InventoryItemDefinition>,
}

/// Overlay `assets/data/items.ron` onto the hard-coded catalogs at startup.
/// Missing or unparseable file → keep the defaults (logged, never fatal).
fn overlay_item_catalogs(
    mut item_catalog: ResMut<ItemCatalog>,
    mut inv_catalog: ResMut<InventoryItemCatalog>,
) {
    let file = match fs::read_to_string(ITEMS_PATH) {
        Ok(text) => match ron::de::from_str::<ItemFile>(&text) {
            Ok(f) => f,
            Err(e) => {
                warn!("items loader: {ITEMS_PATH} failed to parse ({e}); using defaults");
                return;
            }
        },
        Err(_) => {
            info!("items loader: no {ITEMS_PATH}; using hard-coded item defaults");
            return;
        }
    };
    let (n_eq, n_con) = (file.equipment.len(), file.consumables.len());
    for eq in file.equipment {
        item_catalog.0.insert(eq.id, eq);
    }
    for c in file.consumables {
        inv_catalog.0.insert(c.id, c);
    }
    info!("items loader: overlaid {n_eq} equipment + {n_con} consumable(s) from {ITEMS_PATH}");
}

// ---------------------------------------------------------------------------
// Combat-spawn replay
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn apply_party_equipment_system(
    mut commands: Commands,
    party_equipment: Res<PartyEquipment>,
    item_catalog: Res<ItemCatalog>,
    inv_catalog: Res<InventoryItemCatalog>,
    player_inventory: Res<PlayerInventory>,
    mut q: Query<
        (
            Entity,
            &CharacterKind,
            &mut EquipmentLoadout,
            Option<&PlayerControlled>,
            Option<&mut Inventory>,
        ),
        With<EquipmentPending>,
    >,
) {
    for (entity, kind, mut loadout, is_player, inventory) in q.iter_mut() {
        // Spawn + slot each persisted piece of gear as a CHILD of the combatant
        // so it is despawned with them at battle end (no cross-battle leak), and
        // `apply_equipment_bonuses_system` folds its stats into CombatStats.
        if let Some(ids) = party_equipment.0.get(kind) {
            for &id in ids {
                if let Some(eq) = item_catalog.0.get(&id) {
                    let item_entity = commands.spawn(eq.clone()).id();
                    commands.entity(entity).add_child(item_entity);
                    loadout.equip_in_first_matching_slot(eq.equipment_type, item_entity);
                }
            }
        }

        // The leader carries the party's owned consumables into battle, so the
        // combat item menu reflects what you actually own.
        if is_player.is_some() {
            if let Some(mut inv) = inventory {
                let carried: Vec<u16> = player_inventory
                    .0
                    .iter()
                    .filter(|stack| {
                        matches!(
                            inv_catalog.0.get(&stack.item_id).map(|d| &d.kind),
                            Some(InventoryItemKind::Consumable { .. })
                        )
                    })
                    .flat_map(|stack| {
                        std::iter::repeat(stack.item_id).take(stack.quantity as usize)
                    })
                    .collect();
                if !carried.is_empty() {
                    inv.item_ids = carried;
                }
            }
        }

        commands.entity(entity).remove::<EquipmentPending>();
    }
}

// ---------------------------------------------------------------------------
// Equip / unequip (drives the character-sheet UI)
// ---------------------------------------------------------------------------

/// Does `kind`'s loadout have any slot that accepts this equipment type?
pub fn member_accepts(kind: CharacterKind, eq_type: EquipmentType) -> bool {
    kind.equipment_loadout()
        .slots
        .iter()
        .any(|s| s.allowed_types.contains(&eq_type))
}

/// How many slots of the type that fits `eq_type` does this member have, and
/// how many are already filled by currently-equipped gear of that slot type.
fn slot_capacity(
    party_equipment: &PartyEquipment,
    item_catalog: &ItemCatalog,
    kind: CharacterKind,
    eq_type: EquipmentType,
) -> (usize, usize) {
    let slot_type = eq_type.slot_type();
    let total = kind
        .equipment_loadout()
        .slots
        .iter()
        .filter(|s| s.slot_type == slot_type)
        .count();
    let used = party_equipment
        .0
        .get(&kind)
        .map(|ids| {
            ids.iter()
                .filter(|id| {
                    item_catalog
                        .0
                        .get(id)
                        .map(|e| e.equipment_type.slot_type() == slot_type)
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);
    (total, used)
}

/// Can `kind` equip this item right now (compatible type and a free slot)?
pub fn can_equip(
    party_equipment: &PartyEquipment,
    item_catalog: &ItemCatalog,
    kind: CharacterKind,
    item_id: u16,
) -> bool {
    let Some(eq) = item_catalog.0.get(&item_id) else {
        return false;
    };
    if !member_accepts(kind, eq.equipment_type) {
        return false;
    }
    let (total, used) = slot_capacity(party_equipment, item_catalog, kind, eq.equipment_type);
    used < total
}

/// Move one `item_id` from the owned pool onto `kind`. Returns false if the
/// member can't take it or doesn't own one. The actual stat effect lands next
/// time the member enters battle (via [`apply_party_equipment_system`]).
pub fn equip_item(
    party_equipment: &mut PartyEquipment,
    inventory: &mut PlayerInventory,
    item_catalog: &ItemCatalog,
    kind: CharacterKind,
    item_id: u16,
) -> bool {
    if !can_equip(party_equipment, item_catalog, kind, item_id) {
        return false;
    }
    if !inventory_remove(inventory, item_id) {
        return false;
    }
    party_equipment.0.entry(kind).or_default().push(item_id);
    true
}

/// Take `item_id` off `kind` and return it to the owned pool.
pub fn unequip_item(
    party_equipment: &mut PartyEquipment,
    inventory: &mut PlayerInventory,
    kind: CharacterKind,
    item_id: u16,
) -> bool {
    let list = party_equipment.0.entry(kind).or_default();
    if let Some(pos) = list.iter().position(|&i| i == item_id) {
        list.remove(pos);
        inventory_add(inventory, item_id);
        true
    } else {
        false
    }
}

/// Remove one unit of `item_id` from `PlayerInventory`; false if none owned.
fn inventory_remove(inventory: &mut PlayerInventory, item_id: u16) -> bool {
    if let Some(stack) = inventory.0.iter_mut().find(|s| s.item_id == item_id) {
        if stack.quantity > 0 {
            stack.quantity -= 1;
            inventory.0.retain(|s| s.quantity > 0);
            return true;
        }
    }
    false
}

/// Add one unit of `item_id` to `PlayerInventory`, stacking if present.
fn inventory_add(inventory: &mut PlayerInventory, item_id: u16) {
    if let Some(stack) = inventory.0.iter_mut().find(|s| s.item_id == item_id) {
        stack.quantity += 1;
    } else {
        inventory.0.push(crate::economy::InventoryStack {
            item_id,
            quantity: 1,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::InventoryStack;

    #[test]
    fn items_file_parses() {
        let text = std::fs::read_to_string(ITEMS_PATH).expect("items.ron exists");
        let file: ItemFile = ron::de::from_str(&text).expect("items.ron parses");
        assert!(!file.equipment.is_empty(), "expected equipment entries");
        // Every equipment id should round-trip its declared type.
        for eq in &file.equipment {
            assert_eq!(eq.equipment_type.slot_type(), eq.equipment_type.slot_type());
        }
    }

    #[test]
    fn equip_unequip_round_trips() {
        // Houjou (Samurai) accepts a Sword in his weapon slot.
        let kind = CharacterKind::Houjou;
        let sword_id = 5001u16; // Silversteel Blade (default ItemCatalog)
        let catalog = ItemCatalog::default();
        // Sanity: the member actually accepts this type.
        let eq_type = catalog.0.get(&sword_id).unwrap().equipment_type;
        assert!(member_accepts(kind, eq_type));

        let mut party = PartyEquipment::default();
        let mut inv = PlayerInventory(vec![InventoryStack {
            item_id: sword_id,
            quantity: 1,
        }]);

        assert!(equip_item(&mut party, &mut inv, &catalog, kind, sword_id));
        assert_eq!(party.0.get(&kind).map(|v| v.len()), Some(1));
        assert!(inv.0.iter().all(|s| s.item_id != sword_id)); // consumed from pool

        // Can't equip a second one we don't own.
        assert!(!equip_item(&mut party, &mut inv, &catalog, kind, sword_id));

        assert!(unequip_item(&mut party, &mut inv, kind, sword_id));
        assert!(party.0.get(&kind).map(|v| v.is_empty()).unwrap_or(true));
        assert_eq!(inv.0.iter().find(|s| s.item_id == sword_id).map(|s| s.quantity), Some(1));
    }
}
