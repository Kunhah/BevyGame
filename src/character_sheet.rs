//! Read-only party / character-sheet overlay, opened with `C`.
//!
//! Surfaces what was previously only visible by reading the code: each party
//! member's identity, core combat stats, magic affinities, ability list,
//! equipment slots and persistent skill progression — plus the shared wallet
//! and trade-goods inventory. It reads everything straight off
//! [`CharacterKind`] (which can recompute a member's stat block without a live
//! combat entity) and the persistent [`PartyProgression`] / [`PlayerInventory`]
//! resources, so it is accurate on the overworld where no combatants exist.
//!
//! Editing (equipping gear, spending items) is intentionally out of scope: that
//! needs a persistent per-member equip model that doesn't exist yet. Skill
//! points are still spent on the dedicated skill screen (`K`).

use bevy::prelude::*;

use crate::characters::{CharacterKind, SelectedParty};
use crate::combat_ability::Ability_Tree;
use crate::core::{GameState, Game_State};
use crate::economy::{ItemCatalog, PlayerInventory, PlayerWallet};
use crate::skill_tree::PartyProgression;
use crate::ui_style::{font_size, overlay_root, palette, panel, spacing};

pub struct CharacterSheetPlugin;

impl Plugin for CharacterSheetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, toggle_character_sheet)
            .add_systems(Update, sync_character_sheet);
    }
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

fn sync_character_sheet(
    mut commands: Commands,
    game_state: Res<GameState>,
    party: Res<SelectedParty>,
    progression: Res<PartyProgression>,
    ability_tree: Option<Res<Ability_Tree>>,
    wallet: Res<PlayerWallet>,
    inventory: Res<PlayerInventory>,
    item_catalog: Res<ItemCatalog>,
    existing: Query<Entity, With<CharacterSheetRoot>>,
) {
    if game_state.0 != Game_State::CharacterSheet {
        for e in existing.iter() {
            commands.entity(e).despawn();
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }

    let leader = party.0.first().copied();

    commands
        .spawn((overlay_root(), CharacterSheetRoot))
        .with_children(|root| {
            root.spawn(panel(760.0)).with_children(|col| {
                col.spawn((
                    Text::new("Party"),
                    TextFont {
                        font_size: font_size::HEADING,
                        ..default()
                    },
                    TextColor(palette::TEXT_HEADING),
                ));

                // Shared resources line.
                col.spawn((
                    Text::new(format!("Purse: {}", wallet.coins.format_short())),
                    TextFont {
                        font_size: font_size::BODY,
                        ..default()
                    },
                    TextColor(palette::ACCENT_WARNING),
                    Node {
                        margin: UiRect::bottom(Val::Px(spacing::SM)),
                        ..default()
                    },
                ));

                if party.0.is_empty() {
                    col.spawn((
                        Text::new("No party selected."),
                        TextFont {
                            font_size: font_size::BODY,
                            ..default()
                        },
                        TextColor(palette::TEXT_SECONDARY),
                    ));
                }

                for kind in &party.0 {
                    spawn_member_card(
                        col,
                        *kind,
                        Some(*kind) == leader,
                        &progression,
                        ability_tree.as_deref(),
                    );
                }

                // Trade goods (economy inventory) summary.
                col.spawn((
                    Text::new("Inventory"),
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
                if inventory.0.is_empty() {
                    col.spawn((
                        Text::new("Empty."),
                        TextFont {
                            font_size: font_size::LABEL,
                            ..default()
                        },
                        TextColor(palette::TEXT_DIM),
                    ));
                } else {
                    for stack in inventory.0.iter().take(10) {
                        let name = item_catalog
                            .0
                            .get(&stack.item_id)
                            .map(|e| e.name.clone())
                            .unwrap_or_else(|| format!("Item #{}", stack.item_id));
                        col.spawn((
                            Text::new(format!("• {} ×{}", name, stack.quantity)),
                            TextFont {
                                font_size: font_size::LABEL,
                                ..default()
                            },
                            TextColor(palette::TEXT_PRIMARY),
                        ));
                    }
                }

                col.spawn((
                    Text::new("C or Esc — close   ·   K — spend skill points"),
                    TextFont {
                        font_size: font_size::SMALL,
                        ..default()
                    },
                    TextColor(palette::TEXT_DIM),
                    Node {
                        margin: UiRect::top(Val::Px(spacing::LG)),
                        ..default()
                    },
                ));
            });
        });
}

fn spawn_member_card(
    col: &mut ChildSpawnerCommands,
    kind: CharacterKind,
    is_leader: bool,
    progression: &PartyProgression,
    ability_tree: Option<&Ability_Tree>,
) {
    let stats = kind.combat_stats();
    let progress = progression.0.get(&kind);
    let learned = progress.map(|p| p.learned.len()).unwrap_or(0);
    let available = progress.map(|p| p.available).unwrap_or(0);

    // Name + class header.
    let header = if is_leader {
        format!("{}  —  {}  (Leader)", kind.display_name(), kind.class_label())
    } else {
        format!("{}  —  {}", kind.display_name(), kind.class_label())
    };
    col.spawn((
        Text::new(header),
        TextFont {
            font_size: font_size::BODY_LG,
            ..default()
        },
        TextColor(if is_leader { palette::BRAND } else { palette::TEXT_HEADING }),
        Node {
            margin: UiRect::top(Val::Px(spacing::MD)),
            ..default()
        },
    ));

    // Core combat stats (base values — the live current pool only exists in a
    // battle).
    col.spawn((
        Text::new(format!(
            "HP {}   Morale {}   Lethality {}   Hit {}   Armor {}   Speed {}   Evasion {}   Mind {}",
            stats.health.base,
            stats.morale.base,
            stats.lethality.base,
            stats.hit.base,
            stats.armor.base,
            stats.speed.base,
            stats.evasion.base,
            stats.mind.base,
        )),
        TextFont {
            font_size: font_size::LABEL,
            ..default()
        },
        TextColor(palette::TEXT_PRIMARY),
    ));

    // Magic affinities.
    let affinities: Vec<String> = kind
        .magic_affinities()
        .iter()
        .map(|s| format!("{:?}", s))
        .collect();
    col.spawn((
        Text::new(format!(
            "Magic: {}   ·   Element: {:?}",
            if affinities.is_empty() { "—".to_string() } else { affinities.join(", ") },
            kind.innate_element(),
        )),
        TextFont {
            font_size: font_size::LABEL,
            ..default()
        },
        TextColor(palette::TEXT_SECONDARY),
    ));

    // Abilities (names where the ability tree is loaded, ids otherwise).
    let ability_names: Vec<String> = kind
        .abilities()
        .iter()
        .map(|&id| {
            ability_tree
                .and_then(|t| t.0.find(id))
                .map(|a| a.name)
                .unwrap_or_else(|| format!("#{id}"))
        })
        .collect();
    col.spawn((
        Text::new(format!(
            "Abilities: {}",
            if ability_names.is_empty() { "—".to_string() } else { ability_names.join(", ") }
        )),
        TextFont {
            font_size: font_size::LABEL,
            ..default()
        },
        TextColor(palette::TEXT_SECONDARY),
    ));

    // Skill progression.
    let sp_color = if available > 0 { palette::ACCENT_SUCCESS } else { palette::TEXT_DIM };
    col.spawn((
        Text::new(format!(
            "Skills learned: {}   ·   Skill points: {}",
            learned, available
        )),
        TextFont {
            font_size: font_size::LABEL,
            ..default()
        },
        TextColor(sp_color),
    ));
}
