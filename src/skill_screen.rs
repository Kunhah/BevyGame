//! Character / skill-tree overlay (`Game_State::SkillTree`).
//!
//! Opened with `K` while exploring. Shows the party leader's skill points and
//! lets the player browse every tree the leader has access to and spend points
//! on nodes. Both mouse and keyboard work:
//!
//! - **Mouse**: click a tree tab to switch trees; click a node to learn it (if
//!   its prerequisites are met and you can afford it). Hovering a node shows its
//!   description and effects in the detail line.
//! - **Keyboard**: `←`/`→` switch trees, `↑`/`↓` move the node focus, `Enter`
//!   learns the focused node, `K`/`Esc` close.
//!
//! The source of truth is the persistent [`PartyProgression`] resource (keyed by
//! [`CharacterKind`]), so points spent here carry into battle: every combatant
//! spawned for that character replays its learned skills via
//! [`crate::skill_tree::apply_party_progression_system`]. The panel rebuilds
//! whenever the selection, focus, or the leader's points / learned set change.

use bevy::prelude::*;

use crate::characters::CharacterKind;
use crate::core::{GameState, Game_State, Player};
use crate::skill_tree::{
    CharacterProgress, PartyProgression, SkillEffect, SkillNode, SkillTreeData, SkillTreeKind,
};
use crate::ui_style::{font_size, palette, radius, spacing};

pub struct SkillScreenPlugin;

impl Plugin for SkillScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkillScreenState>().add_systems(
            Update,
            (
                toggle_skill_screen,
                skill_screen_keyboard,
                skill_screen_mouse,
                render_skill_screen,
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct SkillScreenState {
    /// Index into the leader's accessible-tree list.
    tree_index: usize,
    /// Focused node index within the current tree.
    focus: usize,
    /// Snapshot the panel was last built from, to avoid rebuilding every frame.
    built: Option<Snapshot>,
}

#[derive(PartialEq, Eq, Clone)]
struct Snapshot {
    leader: CharacterKind,
    tree_index: usize,
    focus: usize,
    available: u32,
    learned: usize,
}

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

#[derive(Component)]
struct SkillScreenRoot;

#[derive(Component, Clone, Copy)]
struct SkillTreeTab(usize);

#[derive(Component, Clone, Copy)]
struct SkillNodeButton {
    id: u16,
    index: usize,
}

// ---------------------------------------------------------------------------
// Toggle
// ---------------------------------------------------------------------------

fn toggle_skill_screen(keys: Res<ButtonInput<KeyCode>>, mut game_state: ResMut<GameState>) {
    if !keys.just_pressed(KeyCode::KeyK) {
        return;
    }
    game_state.0 = match game_state.0 {
        Game_State::Exploring => Game_State::SkillTree,
        Game_State::SkillTree => Game_State::Exploring,
        other => other,
    };
}

// ---------------------------------------------------------------------------
// Leader lookup
// ---------------------------------------------------------------------------

/// Everything the screen needs about the party leader, resolved from the
/// `Player` tag + the persistent progression store.
struct LeaderView {
    kind: CharacterKind,
    progress: CharacterProgress,
    trees: Vec<SkillTreeKind>,
}

fn leader_view(
    player_q: &Query<&CharacterKind, With<Player>>,
    progression: &PartyProgression,
) -> Option<LeaderView> {
    let kind = *player_q.iter().next()?;
    Some(LeaderView {
        kind,
        progress: progression.0.get(&kind).cloned().unwrap_or_default(),
        trees: kind.skill_access().allowed,
    })
}

/// Nodes of `kind`, sorted tier-then-id for a stable, readable column.
fn sorted_nodes(data: &SkillTreeData, kind: SkillTreeKind) -> Vec<SkillNode> {
    let mut nodes = data.tree_for(kind).to_vec();
    nodes.sort_by_key(|n| (n.tier, n.id));
    nodes
}

// ---------------------------------------------------------------------------
// Input — keyboard
// ---------------------------------------------------------------------------

fn skill_screen_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    mut state: ResMut<SkillScreenState>,
    data: Res<SkillTreeData>,
    mut progression: ResMut<PartyProgression>,
    player_q: Query<&CharacterKind, With<Player>>,
) {
    if game_state.0 != Game_State::SkillTree {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        game_state.0 = Game_State::Exploring;
        return;
    }
    let Some(leader) = leader_view(&player_q, &progression) else { return };
    if leader.trees.is_empty() {
        return;
    }

    let tree_count = leader.trees.len();
    if keys.just_pressed(KeyCode::ArrowRight) {
        state.tree_index = (state.tree_index + 1) % tree_count;
        state.focus = 0;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        state.tree_index = (state.tree_index + tree_count - 1) % tree_count;
        state.focus = 0;
    }

    let kind = leader.trees[state.tree_index.min(tree_count - 1)];
    let nodes = sorted_nodes(&data, kind);
    if nodes.is_empty() {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        state.focus = (state.focus + 1) % nodes.len();
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        state.focus = (state.focus + nodes.len() - 1) % nodes.len();
    }
    if keys.just_pressed(KeyCode::Enter) {
        if let Some(node) = nodes.get(state.focus.min(nodes.len() - 1)) {
            progression.entry_mut(leader.kind).learn(node);
        }
    }
}

// ---------------------------------------------------------------------------
// Input — mouse
// ---------------------------------------------------------------------------

fn skill_screen_mouse(
    game_state: Res<GameState>,
    mut state: ResMut<SkillScreenState>,
    data: Res<SkillTreeData>,
    mut progression: ResMut<PartyProgression>,
    tabs: Query<(&Interaction, &SkillTreeTab), Changed<Interaction>>,
    nodes: Query<(&Interaction, &SkillNodeButton), Changed<Interaction>>,
    player_q: Query<&CharacterKind, With<Player>>,
) {
    if game_state.0 != Game_State::SkillTree {
        return;
    }
    let Some(leader) = leader_view(&player_q, &progression) else { return };
    if leader.trees.is_empty() {
        return;
    }

    for (interaction, tab) in &tabs {
        if *interaction != Interaction::None {
            state.tree_index = tab.0;
        }
        if *interaction == Interaction::Pressed {
            state.tree_index = tab.0;
            state.focus = 0;
        }
    }

    for (interaction, node) in &nodes {
        if *interaction != Interaction::None {
            state.focus = node.index;
        }
        if *interaction == Interaction::Pressed {
            let kind = leader.trees[state.tree_index.min(leader.trees.len() - 1)];
            if let Some(def) = sorted_nodes(&data, kind).into_iter().find(|n| n.id == node.id) {
                progression.entry_mut(leader.kind).learn(&def);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Render (spawn / despawn / rebuild)
// ---------------------------------------------------------------------------

fn render_skill_screen(
    mut commands: Commands,
    game_state: Res<GameState>,
    mut state: ResMut<SkillScreenState>,
    data: Res<SkillTreeData>,
    progression: Res<PartyProgression>,
    player_q: Query<&CharacterKind, With<Player>>,
    roots: Query<Entity, With<SkillScreenRoot>>,
) {
    if game_state.0 != Game_State::SkillTree {
        if !roots.is_empty() {
            for e in &roots {
                commands.entity(e).despawn();
            }
            state.built = None;
        }
        return;
    }

    let Some(leader) = leader_view(&player_q, &progression) else { return };
    if leader.trees.is_empty() {
        return;
    }
    state.tree_index = state.tree_index.min(leader.trees.len() - 1);

    let snapshot = Snapshot {
        leader: leader.kind,
        tree_index: state.tree_index,
        focus: state.focus,
        available: leader.progress.available,
        learned: leader.progress.learned.len(),
    };
    if state.built.as_ref() == Some(&snapshot) && !roots.is_empty() {
        return; // nothing changed
    }

    for e in &roots {
        commands.entity(e).despawn();
    }

    let kind = leader.trees[state.tree_index];
    let nodes = sorted_nodes(&data, kind);
    state.focus = if nodes.is_empty() {
        0
    } else {
        state.focus.min(nodes.len() - 1)
    };

    spawn_panel(&mut commands, &leader, &nodes, state.tree_index, state.focus);
    state.built = Some(snapshot);
}

fn spawn_panel(
    commands: &mut Commands,
    leader: &LeaderView,
    nodes: &[SkillNode],
    tree_index: usize,
    focus: usize,
) {
    commands
        .spawn((crate::ui_style::overlay_root(), SkillScreenRoot))
        .with_children(|root| {
            root.spawn(crate::ui_style::panel(760.0)).with_children(|col| {
                // Header.
                col.spawn(crate::ui_style::heading_text(format!(
                    "{} the {} — Skill Trees",
                    leader.kind.display_name(),
                    leader.kind.class_label()
                )));
                col.spawn(text(
                    &format!("Skill points available: {}", leader.progress.available),
                    font_size::BODY,
                    palette::ACCENT_WARNING,
                ));
                col.spawn(text(
                    "←/→ switch tree · ↑/↓ move · Enter learn · K/Esc close",
                    font_size::SMALL,
                    palette::TEXT_SECONDARY,
                ));

                // Tree tabs.
                col.spawn(Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(spacing::XS),
                    row_gap: Val::Px(spacing::XS),
                    margin: UiRect::vertical(Val::Px(spacing::SM)),
                    ..default()
                })
                .with_children(|tabs| {
                    for (i, t) in leader.trees.iter().copied().enumerate() {
                        let active = i == tree_index;
                        let (bg, border, fg) = if active {
                            (palette::BG_BUTTON_PRESSED, palette::BORDER_ACCENT, palette::TEXT_HEADING)
                        } else {
                            (palette::BG_BUTTON, palette::BORDER, palette::TEXT_SECONDARY)
                        };
                        tabs.spawn((
                            Button::default(),
                            Node {
                                padding: UiRect::axes(Val::Px(spacing::SM), Val::Px(spacing::XS)),
                                border: UiRect::all(Val::Px(1.5)),
                                border_radius: BorderRadius::all(Val::Px(radius::SM)),
                                ..default()
                            },
                            BackgroundColor(bg),
                            BorderColor::all(border),
                            SkillTreeTab(i),
                        ))
                        .with_children(|b| {
                            b.spawn(text(tree_label(t), font_size::LABEL, fg));
                        });
                    }
                });

                // Node list.
                if nodes.is_empty() {
                    col.spawn(text("(this tree has no nodes)", font_size::BODY, palette::TEXT_DIM));
                    return;
                }
                for (i, node) in nodes.iter().enumerate() {
                    spawn_node_row(col, node, i, focus, &leader.progress);
                }

                // Detail line for the focused node.
                if let Some(node) = nodes.get(focus) {
                    col.spawn((
                        Node { margin: UiRect::top(Val::Px(spacing::SM)), ..default() },
                        text(&node_detail(node), font_size::SMALL, palette::TEXT_PRIMARY),
                    ));
                }
            });
        });
}

fn spawn_node_row(
    parent: &mut ChildSpawnerCommands,
    node: &SkillNode,
    index: usize,
    focus: usize,
    progress: &CharacterProgress,
) {
    let is_learned = progress.has(node.id);
    let prereqs_met = node.prerequisites.iter().all(|p| progress.has(*p));
    let affordable = progress.available >= node.cost;
    let focused = index == focus;

    let (status, status_color) = if is_learned {
        ("✓ learned", palette::ACCENT_SUCCESS)
    } else if !prereqs_met {
        ("locked", palette::TEXT_DIM)
    } else if !affordable {
        ("need SP", palette::ACCENT_DANGER)
    } else {
        ("learn", palette::ACCENT_PRIMARY)
    };

    let (bg, border) = if focused {
        (palette::BG_BUTTON_HOVER, palette::BORDER_ACCENT)
    } else if is_learned {
        (palette::BG_PANEL_RAISED, palette::BORDER_SUBTLE)
    } else {
        (palette::BG_BUTTON, palette::BORDER)
    };

    parent
        .spawn((
            Button::default(),
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(spacing::MD), Val::Px(spacing::XS)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::SM)),
                margin: UiRect::bottom(Val::Px(spacing::XS)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
            SkillNodeButton { id: node.id, index },
        ))
        .with_children(|row| {
            let name_color = if is_learned {
                palette::TEXT_SECONDARY
            } else {
                palette::TEXT_PRIMARY
            };
            row.spawn(text(
                &format!("T{}  {}   ({} SP)", node.tier, node.name, node.cost),
                font_size::LABEL,
                name_color,
            ));
            row.spawn(text(status, font_size::SMALL, status_color));
        });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn node_detail(node: &SkillNode) -> String {
    let effects: Vec<String> = node.effects.iter().map(describe_effect).collect();
    let effects = if effects.is_empty() {
        "no direct effect".to_string()
    } else {
        effects.join(", ")
    };
    if node.description.is_empty() {
        format!("{}  —  {}", node.name, effects)
    } else {
        format!("{}  —  {}  ({})", node.name, node.description, effects)
    }
}

fn describe_effect(e: &SkillEffect) -> String {
    match e {
        SkillEffect::StatBonus { target, amount } => format!("+{amount} {target:?}"),
        SkillEffect::MagicRegenBonus { school, amount } => {
            format!("+{amount} {school:?} regen/rest")
        }
        SkillEffect::UnlockAbility { ability_id } => format!("unlock ability {ability_id}"),
        SkillEffect::MagicCostReduction { school, percent } => {
            format!("-{:.0}% {school:?} cost", percent * 100.0)
        }
        SkillEffect::Trigger { trigger_id } => format!("triggers event #{trigger_id}"),
    }
}

fn tree_label(kind: SkillTreeKind) -> &'static str {
    match kind {
        SkillTreeKind::Kiho => "Kihō",
        SkillTreeKind::Onmyodo => "Onmyōdō",
        SkillTreeKind::Yokaijutsu => "Yōkaijutsu",
        SkillTreeKind::Kamishin => "Kamishin",
        SkillTreeKind::Martial => "Martial",
        SkillTreeKind::Survival => "Survival",
        SkillTreeKind::Bound => "Bound",
        SkillTreeKind::RinaRogue => "Rogue",
        SkillTreeKind::SayakaCleric => "Cleric",
        SkillTreeKind::HoujouSamurai => "Samurai",
        SkillTreeKind::ToshikoVessel => "Vessel",
        SkillTreeKind::RenjiroMonk => "Monk",
        SkillTreeKind::SuzukaOnmyoji => "Onmyoji",
        SkillTreeKind::KanzoExorcist => "Exorcist",
    }
}

fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont { font_size: size, ..default() },
        TextColor(color),
    )
}
