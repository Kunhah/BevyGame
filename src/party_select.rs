//! Party-selection screen (`Game_State::PartySelection`).
//!
//! Shown once between the main menu and exploration when starting a new run.
//! The player toggles up to [`crate::constants::MAX_OBJECTS`] characters from
//! the seven-strong roster; the first pick becomes the party leader (the
//! overworld avatar). Confirming writes the choice into [`SelectedParty`], which
//! [`crate::world::spawn_party`] then reads to spawn the avatar + companions.
//!
//! Built with `bevy_ui`, matching the idiom in [`crate::menu`] / [`crate::ui_style`]:
//! a full-screen overlay root tagged [`PartySelectRoot`], spawned when the state
//! is entered and despawned when it is left. Selection is shown on each toggle's
//! child label (a ●/○ marker + colour), which is robust against the shared
//! `update_standard_button_visuals` hover/press restyling.

use bevy::prelude::*;

use crate::characters::{CharacterKind, SelectedParty};
use crate::combat_ability::MagicSchool;
use crate::constants::MAX_OBJECTS;
use crate::core::{GameState, Game_State};
use crate::ui_style::{
    button_node, button_text, button_visual, heading_text, label_text, palette, panel, radius,
    spacing,
};

/// The roster being assembled, in pick order. Element 0 is the leader. Cleared
/// each time the screen opens.
#[derive(Resource, Default)]
pub struct PartyDraft(pub Vec<CharacterKind>);

/// Root node of the selection overlay (despawned on leaving the state).
#[derive(Component)]
struct PartySelectRoot;

/// A character toggle button.
#[derive(Component)]
struct PartyToggle(CharacterKind);

/// The child text of a toggle button (updated to reflect selection).
#[derive(Component)]
struct PartyToggleLabel(CharacterKind);

/// The confirm button.
#[derive(Component)]
struct PartyConfirm;

/// The child text of the confirm button (shows the running count).
#[derive(Component)]
struct PartyConfirmLabel;

/// Detail line describing the keyboard-focused character.
#[derive(Component)]
struct PartyDetail;

/// Index (into [`CharacterKind::ALL`]) of the keyboard-focused tile.
#[derive(Resource, Default)]
struct PartyFocus(usize);

pub struct PartySelectPlugin;

impl Plugin for PartySelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PartyDraft>()
            .init_resource::<PartyFocus>()
            .add_systems(
                Update,
                (
                    spawn_party_select_ui,
                    handle_party_select_interactions,
                    handle_party_select_keyboard,
                    update_party_select_visuals,
                    teardown_party_select_ui,
                ),
            );
    }
}

fn magic_short(school: MagicSchool) -> &'static str {
    match school {
        MagicSchool::Kiho => "Kihō",
        MagicSchool::Onmyodo => "Onmyōdō",
        MagicSchool::Yokaijutsu => "Yōkaijutsu",
        MagicSchool::Kamishin => "Kamishin",
    }
}

fn affinity_text(kind: CharacterKind) -> String {
    let schools: Vec<&str> = kind.magic_affinities().iter().map(|s| magic_short(*s)).collect();
    if schools.is_empty() {
        "no magic".to_string()
    } else {
        schools.join(", ")
    }
}

/// One fixed-width toggle tile so the roster wraps into a tidy grid.
fn toggle_node() -> Node {
    Node {
        width: Val::Px(272.0),
        height: Val::Px(48.0),
        display: Display::Flex,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        padding: UiRect::all(Val::Px(spacing::SM)),
        border: UiRect::all(Val::Px(1.5)),
        border_radius: BorderRadius::all(Val::Px(radius::MD)),
        ..default()
    }
}

fn spawn_party_select_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    mut draft: ResMut<PartyDraft>,
    mut focus: ResMut<PartyFocus>,
    existing: Query<(), With<PartySelectRoot>>,
) {
    if game_state.0 != Game_State::PartySelection || !existing.is_empty() {
        return;
    }
    // Fresh selection each time the screen opens.
    draft.0.clear();
    focus.0 = 0;

    let root = commands
        .spawn((crate::ui_style::overlay_root(), PartySelectRoot))
        .id();
    commands.entity(root).with_children(|parent| {
        parent.spawn(panel(620.0)).with_children(|col| {
            col.spawn(heading_text("Choose Your Party"));
            col.spawn(label_text(format!(
                "Pick up to {MAX_OBJECTS}. Your first pick leads the party."
            )));
            col.spawn(label_text(
                "Arrows move · Space toggles · Enter confirms · or use the mouse.",
            ));

            // Roster grid (wraps to two columns inside the panel).
            col.spawn(Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(spacing::SM),
                row_gap: Val::Px(spacing::SM),
                justify_content: JustifyContent::Center,
                margin: UiRect::vertical(Val::Px(spacing::SM)),
                ..default()
            })
            .with_children(|grid| {
                for kind in CharacterKind::ALL {
                    grid.spawn((
                        Button::default(),
                        toggle_node(),
                        button_visual(),
                        PartyToggle(kind),
                    ))
                    .with_children(|btn| {
                        btn.spawn((label_text(""), PartyToggleLabel(kind)));
                    });
                }
            });

            // Detail line for the focused character.
            col.spawn((label_text(""), PartyDetail));

            // Confirm.
            col.spawn((Button::default(), button_node(48.0), button_visual(), PartyConfirm))
                .with_children(|btn| {
                    btn.spawn((button_text("Confirm"), PartyConfirmLabel));
                });
        });
    });
}

fn teardown_party_select_ui(
    mut commands: Commands,
    game_state: Res<GameState>,
    roots: Query<Entity, With<PartySelectRoot>>,
) {
    if game_state.0 == Game_State::PartySelection {
        return;
    }
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}

/// Toggle a character in/out of the draft, respecting the party cap.
fn toggle_kind(draft: &mut PartyDraft, kind: CharacterKind) {
    if let Some(pos) = draft.0.iter().position(|k| *k == kind) {
        draft.0.remove(pos); // deselect
    } else if draft.0.len() < MAX_OBJECTS {
        draft.0.push(kind); // select (until the party is full)
    }
}

/// Commit the draft and leave for exploration. No-op without a leader.
fn confirm_party(
    draft: &PartyDraft,
    selected: &mut SelectedParty,
    game_state: &mut GameState,
) {
    if draft.0.is_empty() {
        return;
    }
    selected.0 = draft.0.clone();
    info!("Party confirmed: {:?}", selected.0);
    game_state.0 = Game_State::Exploring;
}

fn handle_party_select_interactions(
    mut game_state: ResMut<GameState>,
    mut draft: ResMut<PartyDraft>,
    mut focus: ResMut<PartyFocus>,
    mut selected: ResMut<SelectedParty>,
    toggles: Query<(&Interaction, &PartyToggle), (Changed<Interaction>, With<Button>)>,
    confirms: Query<&Interaction, (Changed<Interaction>, With<Button>, With<PartyConfirm>)>,
) {
    if game_state.0 != Game_State::PartySelection {
        return;
    }

    for (interaction, toggle) in &toggles {
        // Hovering moves the focus so the detail line tracks the mouse.
        if *interaction != Interaction::None {
            if let Some(idx) = CharacterKind::ALL.iter().position(|k| *k == toggle.0) {
                focus.0 = idx;
            }
        }
        if *interaction == Interaction::Pressed {
            toggle_kind(&mut draft, toggle.0);
        }
    }

    for interaction in &confirms {
        if *interaction == Interaction::Pressed {
            confirm_party(&draft, &mut selected, &mut game_state);
        }
    }
}

/// Arrow keys move the focus, Space toggles the focused character, Enter
/// confirms.
fn handle_party_select_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    mut draft: ResMut<PartyDraft>,
    mut focus: ResMut<PartyFocus>,
    mut selected: ResMut<SelectedParty>,
) {
    if game_state.0 != Game_State::PartySelection {
        return;
    }
    let n = CharacterKind::ALL.len();
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::ArrowRight) {
        focus.0 = (focus.0 + 1) % n;
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::ArrowLeft) {
        focus.0 = (focus.0 + n - 1) % n;
    }
    if keys.just_pressed(KeyCode::Space) {
        toggle_kind(&mut draft, CharacterKind::ALL[focus.0]);
    }
    if keys.just_pressed(KeyCode::Enter) {
        confirm_party(&draft, &mut selected, &mut game_state);
    }
}

fn update_party_select_visuals(
    draft: Res<PartyDraft>,
    focus: Res<PartyFocus>,
    mut toggle_labels: Query<(&PartyToggleLabel, &mut Text, &mut TextColor)>,
    mut confirm_label: Query<
        &mut Text,
        (With<PartyConfirmLabel>, Without<PartyToggleLabel>, Without<PartyDetail>),
    >,
    mut detail: Query<
        &mut Text,
        (With<PartyDetail>, Without<PartyToggleLabel>, Without<PartyConfirmLabel>),
    >,
) {
    let focused_kind = CharacterKind::ALL.get(focus.0).copied();

    // Cheap (7 labels) and must also run on the frame after the UI spawns, when
    // the draft isn't "changed" but the freshly-created labels are still blank.
    for (label, mut text, mut color) in &mut toggle_labels {
        let kind = label.0;
        let idx = draft.0.iter().position(|k| *k == kind);
        let is_leader = idx == Some(0);
        let is_focused = focused_kind == Some(kind);
        let focus_mark = if is_focused { "▶ " } else { "  " };
        let mark = if idx.is_some() { "●" } else { "○" };
        let suffix = if is_leader { "  (leader)" } else { "" };
        *text = Text::new(format!(
            "{focus_mark}{mark} {} · {}{}",
            kind.display_name(),
            kind.class_label(),
            suffix
        ));
        color.0 = if is_leader {
            palette::ACCENT_PRIMARY
        } else if idx.is_some() {
            palette::ACCENT_SUCCESS
        } else if is_focused {
            palette::TEXT_HEADING
        } else {
            palette::TEXT_SECONDARY
        };
    }

    if let Ok(mut text) = detail.single_mut() {
        let desired = match focused_kind {
            Some(kind) => format!(
                "{} the {} — magic: {}",
                kind.display_name(),
                kind.class_label(),
                affinity_text(kind)
            ),
            None => String::new(),
        };
        if text.0 != desired {
            *text = Text::new(desired);
        }
    }

    if let Ok(mut text) = confirm_label.single_mut() {
        *text = Text::new(format!("Confirm  ({}/{})", draft.0.len(), MAX_OBJECTS));
    }
}
