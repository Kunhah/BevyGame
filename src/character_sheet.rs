//! Party / character-sheet overlay, opened with `C`.
//!
//! Now interactive: pick a party member (tabs), see their stats with equipped
//! bonuses, and **equip / unequip gear**. Owned equipment comes from the shared
//! [`PlayerInventory`]; equipping moves it into the persistent [`PartyEquipment`]
//! (which is replayed onto the member each battle and saved with the run).
//! Stats, magic affinities, abilities and learned skills are read straight off
//! [`CharacterKind`] / [`PartyProgression`], so the sheet is accurate on the
//! overworld where no combatants exist. Consumables are listed (they're used
//! from the in-battle item menu); skill points are still spent on `K`.

use bevy::prelude::*;

use crate::characters::{CharacterKind, SelectedParty};
use crate::combat_ability::Ability_Tree;
use crate::combat_plugin::{InventoryItemCatalog, InventoryItemKind};
use crate::core::{GameState, Game_State};
use crate::economy::{ItemCatalog, PlayerInventory, PlayerWallet};
use crate::equipment::{can_equip, equip_item, member_accepts, unequip_item, PartyEquipment};
use crate::skill_tree::PartyProgression;
use crate::ui_style::{
    button_node, button_visual, font_size, overlay_root, palette, panel, spacing,
};

pub struct CharacterSheetPlugin;

impl Plugin for CharacterSheetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SheetState>()
            .add_systems(Update, toggle_character_sheet)
            .add_systems(Update, handle_sheet_actions)
            .add_systems(Update, sync_character_sheet.after(handle_sheet_actions));
    }
}

/// Which member is selected, and whether the panel needs rebuilding (set by any
/// click so the next `sync` repaints).
#[derive(Resource, Default)]
struct SheetState {
    selected: usize,
    dirty: bool,
}

#[derive(Component, Clone)]
enum SheetAction {
    SelectMember(usize),
    Equip { kind: CharacterKind, item_id: u16 },
    Unequip { kind: CharacterKind, item_id: u16 },
}

#[derive(Component)]
struct CharacterSheetRoot;

fn toggle_character_sheet(input: Res<ButtonInput<KeyCode>>, mut game_state: ResMut<GameState>) {
    if !input.just_pressed(KeyCode::KeyC) {
        return;
    }
    game_state.0 = match game_state.0 {
        Game_State::Exploring => Game_State::CharacterSheet,
        Game_State::CharacterSheet => Game_State::Exploring,
        other => other,
    };
}

fn handle_sheet_actions(
    game_state: Res<GameState>,
    mut sheet: ResMut<SheetState>,
    mut party_equipment: ResMut<PartyEquipment>,
    mut inventory: ResMut<PlayerInventory>,
    item_catalog: Res<ItemCatalog>,
    interactions: Query<(&Interaction, &SheetAction), Changed<Interaction>>,
) {
    if game_state.0 != Game_State::CharacterSheet {
        return;
    }
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            SheetAction::SelectMember(i) => {
                sheet.selected = *i;
                sheet.dirty = true;
            }
            SheetAction::Equip { kind, item_id } => {
                if equip_item(
                    &mut party_equipment,
                    &mut inventory,
                    &item_catalog,
                    *kind,
                    *item_id,
                ) {
                    sheet.dirty = true;
                }
            }
            SheetAction::Unequip { kind, item_id } => {
                if unequip_item(&mut party_equipment, &mut inventory, *kind, *item_id) {
                    sheet.dirty = true;
                }
            }
        }
    }
}

/// Sum of the stat bonuses from everything `kind` currently has equipped.
#[derive(Default)]
struct EquipBonus {
    lethality: i32,
    hit: i32,
    armor: i32,
    agility: i32,
    mind: i32,
    morale: i32,
}

fn equipped_bonus(
    party_equipment: &PartyEquipment,
    item_catalog: &ItemCatalog,
    kind: CharacterKind,
) -> EquipBonus {
    let mut b = EquipBonus::default();
    if let Some(ids) = party_equipment.0.get(&kind) {
        for id in ids {
            if let Some(eq) = item_catalog.0.get(id) {
                b.lethality += eq.lethality;
                b.hit += eq.hit;
                b.armor += eq.armor;
                b.agility += eq.agility;
                b.mind += eq.mind;
                b.morale += eq.morale;
            }
        }
    }
    b
}

#[allow(clippy::too_many_arguments)]
fn sync_character_sheet(
    mut commands: Commands,
    game_state: Res<GameState>,
    mut sheet: ResMut<SheetState>,
    party: Res<SelectedParty>,
    party_equipment: Res<PartyEquipment>,
    item_catalog: Res<ItemCatalog>,
    inv_catalog: Res<InventoryItemCatalog>,
    inventory: Res<PlayerInventory>,
    wallet: Res<PlayerWallet>,
    progression: Res<PartyProgression>,
    ability_tree: Option<Res<Ability_Tree>>,
    existing: Query<Entity, With<CharacterSheetRoot>>,
) {
    if game_state.0 != Game_State::CharacterSheet {
        for e in existing.iter() {
            commands.entity(e).despawn();
        }
        return;
    }

    let exists = !existing.is_empty();
    if exists && !sheet.dirty {
        return;
    }
    for e in existing.iter() {
        commands.entity(e).despawn();
    }
    sheet.dirty = false;

    if party.0.is_empty() {
        return;
    }
    let selected = sheet.selected.min(party.0.len() - 1);
    let kind = party.0[selected];
    let leader = party.0.first().copied();

    commands
        .spawn((overlay_root(), CharacterSheetRoot))
        .with_children(|root| {
            root.spawn(panel(820.0)).with_children(|col| {
                col.spawn((
                    Text::new("Party  ·  Equipment"),
                    TextFont {
                        font_size: font_size::HEADING,
                        ..default()
                    },
                    TextColor(palette::TEXT_HEADING),
                ));
                col.spawn((
                    Text::new(format!("Purse: {}", wallet.coins.format_short())),
                    TextFont {
                        font_size: font_size::LABEL,
                        ..default()
                    },
                    TextColor(palette::ACCENT_WARNING),
                    Node {
                        margin: UiRect::bottom(Val::Px(spacing::SM)),
                        ..default()
                    },
                ));

                // --- Member tabs ---
                col.spawn(Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(spacing::SM),
                    flex_wrap: FlexWrap::Wrap,
                    margin: UiRect::bottom(Val::Px(spacing::SM)),
                    ..default()
                })
                .with_children(|tabs| {
                    for (i, member) in party.0.iter().enumerate() {
                        let is_sel = i == selected;
                        tabs.spawn((
                            Button,
                            button_node(34.0),
                            BackgroundColor(palette::BG_BUTTON),
                            BorderColor::all(if is_sel {
                                palette::BRAND
                            } else {
                                palette::BORDER_SUBTLE
                            }),
                            SheetAction::SelectMember(i),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new(member.display_name().to_string()),
                                TextFont {
                                    font_size: font_size::LABEL,
                                    ..default()
                                },
                                TextColor(if is_sel {
                                    palette::BRAND
                                } else {
                                    palette::TEXT_SECONDARY
                                }),
                            ));
                        });
                    }
                });

                // --- Selected member ---
                let head = if Some(kind) == leader {
                    format!("{}  —  {}  (Leader)", kind.display_name(), kind.class_label())
                } else {
                    format!("{}  —  {}", kind.display_name(), kind.class_label())
                };
                col.spawn((
                    Text::new(head),
                    TextFont {
                        font_size: font_size::BODY_LG,
                        ..default()
                    },
                    TextColor(palette::BRAND),
                ));

                let stats = kind.combat_stats();
                let b = equipped_bonus(&party_equipment, &item_catalog, kind);
                col.spawn((
                    Text::new(format!(
                        "HP {}   Lethality {}{}   Hit {}{}   Armor {}{}   Agility {}{}   Mind {}{}   Morale {}{}",
                        stats.health.base,
                        stats.lethality.base, fmt_bonus(b.lethality),
                        stats.hit.base, fmt_bonus(b.hit),
                        stats.armor.base, fmt_bonus(b.armor),
                        stats.evasion.base, fmt_bonus(b.agility),
                        stats.mind.base, fmt_bonus(b.mind),
                        stats.morale.base, fmt_bonus(b.morale),
                    )),
                    TextFont {
                        font_size: font_size::LABEL,
                        ..default()
                    },
                    TextColor(palette::TEXT_PRIMARY),
                ));

                // Abilities + skills (read-only context).
                let ability_names: Vec<String> = kind
                    .abilities()
                    .iter()
                    .map(|&id| {
                        ability_tree
                            .as_deref()
                            .and_then(|t| t.0.find(id))
                            .map(|a| a.name)
                            .unwrap_or_else(|| format!("#{id}"))
                    })
                    .collect();
                let learned = progression.0.get(&kind).map(|p| p.learned.len()).unwrap_or(0);
                let sp = progression.0.get(&kind).map(|p| p.available).unwrap_or(0);
                col.spawn((
                    Text::new(format!(
                        "Abilities: {}\nSkills learned: {}   ·   Skill points: {} (spend on K)",
                        if ability_names.is_empty() { "—".into() } else { ability_names.join(", ") },
                        learned, sp,
                    )),
                    TextFont {
                        font_size: font_size::SMALL,
                        ..default()
                    },
                    TextColor(palette::TEXT_SECONDARY),
                ));

                // --- Equipped ---
                section_header(col, "Equipped");
                let equipped = party_equipment.0.get(&kind).cloned().unwrap_or_default();
                if equipped.is_empty() {
                    muted_line(col, "(nothing equipped)");
                } else {
                    for item_id in equipped {
                        let name = item_catalog
                            .0
                            .get(&item_id)
                            .map(|e| item_label(e))
                            .unwrap_or_else(|| format!("Item #{item_id}"));
                        action_row(
                            col,
                            &name,
                            palette::TEXT_PRIMARY,
                            "Unequip",
                            palette::ACCENT_DANGER,
                            SheetAction::Unequip { kind, item_id },
                        );
                    }
                }

                // --- Armory: owned, compatible gear ---
                section_header(col, "Armory (owned)");
                let mut shown = 0;
                for stack in inventory.0.iter() {
                    let Some(eq) = item_catalog.0.get(&stack.item_id) else {
                        continue; // not equipment (material / consumable)
                    };
                    if !member_accepts(kind, eq.equipment_type) {
                        continue;
                    }
                    shown += 1;
                    let label = format!("{}  ×{}", item_label(eq), stack.quantity);
                    if can_equip(&party_equipment, &item_catalog, kind, stack.item_id) {
                        action_row(
                            col,
                            &label,
                            palette::TEXT_PRIMARY,
                            "Equip",
                            palette::ACCENT_SUCCESS,
                            SheetAction::Equip { kind, item_id: stack.item_id },
                        );
                    } else {
                        // Owned + compatible but no free slot of that type.
                        action_row_disabled(col, &label, "slot full");
                    }
                }
                if shown == 0 {
                    muted_line(col, "(no compatible gear owned — buy or craft some)");
                }

                // --- Consumables (read-only; used from the battle item menu) ---
                section_header(col, "Consumables (used in battle)");
                let mut any_con = false;
                for stack in inventory.0.iter() {
                    if let Some(def) = inv_catalog.0.get(&stack.item_id) {
                        if matches!(def.kind, InventoryItemKind::Consumable { .. }) {
                            any_con = true;
                            muted_line(col, &format!("• {}  ×{}", def.name, stack.quantity));
                        }
                    }
                }
                if !any_con {
                    muted_line(col, "(none)");
                }

                col.spawn((
                    Text::new("Tabs select a member  ·  C or Esc — close"),
                    TextFont {
                        font_size: font_size::SMALL,
                        ..default()
                    },
                    TextColor(palette::TEXT_DIM),
                    Node {
                        margin: UiRect::top(Val::Px(spacing::MD)),
                        ..default()
                    },
                ));
            });
        });
}

fn fmt_bonus(b: i32) -> String {
    if b > 0 {
        format!(" (+{b})")
    } else if b < 0 {
        format!(" ({b})")
    } else {
        String::new()
    }
}

fn item_label(eq: &crate::combat_plugin::Equipment) -> String {
    let mut parts = Vec::new();
    if eq.lethality != 0 { parts.push(format!("Lth{:+}", eq.lethality)); }
    if eq.hit != 0 { parts.push(format!("Hit{:+}", eq.hit)); }
    if eq.armor != 0 { parts.push(format!("Arm{:+}", eq.armor)); }
    if eq.agility != 0 { parts.push(format!("Agi{:+}", eq.agility)); }
    if eq.mind != 0 { parts.push(format!("Mnd{:+}", eq.mind)); }
    if eq.morale != 0 { parts.push(format!("Mrl{:+}", eq.morale)); }
    if parts.is_empty() {
        eq.name.clone()
    } else {
        format!("{} ({})", eq.name, parts.join(" "))
    }
}

fn section_header(col: &mut ChildSpawnerCommands, label: &str) {
    col.spawn((
        Text::new(label.to_string()),
        TextFont {
            font_size: font_size::SUBHEADING,
            ..default()
        },
        TextColor(palette::TEXT_HEADING),
        Node {
            margin: UiRect::top(Val::Px(spacing::MD)),
            ..default()
        },
    ));
}

fn muted_line(col: &mut ChildSpawnerCommands, text: &str) {
    col.spawn((
        Text::new(text.to_string()),
        TextFont {
            font_size: font_size::LABEL,
            ..default()
        },
        TextColor(palette::TEXT_DIM),
    ));
}

/// A row with a label on the left and an action button on the right.
fn action_row(
    col: &mut ChildSpawnerCommands,
    label: &str,
    label_color: Color,
    btn_text: &str,
    btn_color: Color,
    action: SheetAction,
) {
    col.spawn(Node {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        column_gap: Val::Px(spacing::MD),
        margin: UiRect::vertical(Val::Px(2.0)),
        ..default()
    })
    .with_children(|row| {
        row.spawn((
            Text::new(label.to_string()),
            TextFont {
                font_size: font_size::LABEL,
                ..default()
            },
            TextColor(label_color),
        ));
        row.spawn((Button, button_node(28.0), button_visual(), action))
            .with_children(|b| {
                b.spawn((
                    Text::new(btn_text.to_string()),
                    TextFont {
                        font_size: font_size::LABEL,
                        ..default()
                    },
                    TextColor(btn_color),
                ));
            });
    });
}

fn action_row_disabled(col: &mut ChildSpawnerCommands, label: &str, note: &str) {
    col.spawn(Node {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        column_gap: Val::Px(spacing::MD),
        margin: UiRect::vertical(Val::Px(2.0)),
        ..default()
    })
    .with_children(|row| {
        row.spawn((
            Text::new(label.to_string()),
            TextFont {
                font_size: font_size::LABEL,
                ..default()
            },
            TextColor(palette::TEXT_DIM),
        ));
        row.spawn((
            Text::new(note.to_string()),
            TextFont {
                font_size: font_size::SMALL,
                ..default()
            },
            TextColor(palette::TEXT_DIM),
        ));
    });
}
