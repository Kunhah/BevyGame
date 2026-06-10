//! In-battle action picker UI.
//!
//! During the player's turn this spawns a bottom-anchored panel listing every
//! action the active combatant can take: Attack / Defend / Wait, any usable
//! inventory items, and every ability on its [`Abilities`] component grouped by
//! magic school. The panel is driven by both **mouse and keyboard** and shows
//! live affordability, costs, cooldowns, and a contextual hint line.
//!
//! ## Controls
//!
//! - **Mouse**: hover an option to focus it (its details show in the hint line);
//!   click to choose it. Attack / abilities then ask for a target — left-click
//!   an enemy to commit, right-click to cancel.
//! - **Keyboard**: `↑`/`↓` move the focus, `1`–`9` jump straight to an option,
//!   `Enter` chooses the focused option. While picking a target, `←`/`→`/`Tab`
//!   cycle enemies, `Enter` commits, `Esc` cancels.
//!
//! Affordability (AP / magic-pool cost) and status gates (Silenced, Terrified)
//! are evaluated every frame, so unusable options are dimmed and explain why in
//! the hint line. The target-click system runs before
//! [`crate::movement::mouse_click`] and consumes the click when it acts, so
//! click-to-move still works while the HUD is idle.

use bevy::prelude::*;

use crate::battle::{BattleParticipant, BattleSide};
use crate::combat_ability::{Ability, Ability_Tree, MagicSchool};
use crate::combat_plugin::{
    effective_element, Abilities, Attunement, CombatStats, ElementalAffinity, Inventory,
    InventoryItemCatalog, InventoryItemKind, PendingPlayerAction, PlayerAction,
    PlayerActionEvent, PolarityFlip, OVERLOAD_THRESHOLD,
};
use crate::gogyo::{damage_multiplier_overloaded, Element, Phase, Polarity};
use crate::constants::{BASIC_ATTACK_ACTION_POINT_COST, ITEM_ACTION_POINT_COST};
use crate::core::{GameState, Game_State, MainCamera};
use crate::skill_tree::MagicCostMultipliers;
use crate::status_effects::{action_gates, magic_cost_multiplier, StatusEffects};
use crate::ui_style::{font_size, palette, radius, spacing};

/// Plugin entry point.
pub struct CombatHudPlugin;

impl Plugin for CombatHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatHudState>()
            // Spawn / despawn must run before the input handlers so a freshly
            // spawned HUD is interactable on the same frame.
            .add_systems(Update, manage_combat_hud_lifetime)
            .add_systems(
                Update,
                (handle_combat_hud_mouse, handle_combat_hud_keyboard)
                    .after(manage_combat_hud_lifetime),
            )
            .add_systems(
                Update,
                handle_target_click.before(crate::movement::mouse_click),
            )
            // Appearance / hint / marker updates run in PostUpdate so they win
            // over the shared `update_standard_button_visuals` hover restyling
            // and always reflect the current frame's affordability.
            .add_systems(PostUpdate, (sync_flyout_visibility, sync_combat_hud, sync_hint))
            .add_systems(PostUpdate, sync_target_marker.after(sync_combat_hud))
            .add_systems(PostUpdate, sync_action_cursor)
            .add_systems(PostUpdate, sync_element_wheel);
    }
}

/// Distance (world units) within which a click counts as picking an enemy.
const TARGET_PICK_RADIUS: f32 = 40.0;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Resource, Debug, Clone, Default)]
pub struct CombatHudState {
    pub mode: HudMode,
    /// The flyout currently expanded (Skills / Items), if any.
    open: Option<FlyoutKind>,
    /// Index of the focused top-level category (used when no flyout is open).
    cat_focus: usize,
    /// Index of the focused option inside the open flyout.
    opt_focus: usize,
    /// Number of top-level categories spawned this turn (for focus wrap-around).
    cat_count: usize,
    /// Enemy currently aimed at while in [`HudMode::AwaitingTarget`].
    target: Option<Entity>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HudMode {
    #[default]
    Idle,
    /// An action is chosen; the next click / Enter commits the target.
    AwaitingTarget(SelectedAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedAction {
    Attack,
    Ability(u16),
}

/// What a HUD option does when chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudAction {
    Attack,
    Ability(u16),
    Item(u16),
    Defend,
    Wait,
}

/// The two expandable flyout menus hung off the command bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlyoutKind {
    Skills,
    Items,
}

/// A top-level command-bar entry: either fires an action directly, or expands
/// a flyout of further options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CategoryKind {
    Direct(HudAction),
    Open(FlyoutKind),
}

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

#[derive(Component)]
struct CombatHudRoot;

/// One top-level category button on the command bar.
#[derive(Component, Clone, Copy)]
struct CombatHudCategory {
    index: usize,
    kind: CategoryKind,
}

/// The text child of a category button (restyled for focus / affordability).
#[derive(Component)]
struct CombatHudCategoryLabel;

/// A flyout container; toggled visible when its `kind` is the open flyout.
#[derive(Component, Clone, Copy)]
struct CombatHudFlyout {
    kind: FlyoutKind,
}

/// One selectable option inside a flyout. `index` is its position within that
/// flyout's focus order.
#[derive(Component, Clone, Copy)]
struct CombatHudOption {
    index: usize,
    action: HudAction,
    flyout: FlyoutKind,
}

/// The text child of an option button (restyled for focus / affordability).
#[derive(Component)]
struct CombatHudOptionLabel;

/// Header line showing the actor name and resource pools.
#[derive(Component)]
struct CombatHudHeader;

/// Contextual hint / tooltip line at the bottom of the panel.
#[derive(Component)]
struct CombatHudHint;

/// Floating marker drawn over the currently aimed enemy.
#[derive(Component)]
struct CombatHudTargetMarker;

/// A chip that follows the cursor while an action is armed, naming what will
/// fire when the player clicks a target.
#[derive(Component)]
struct CombatHudCursorChip;

// --- 五行 element wheel widget (§10 of docs/gogyo_elemental_system.md) -------

/// Root of the top-anchored Gogyō wheel widget. Spawned/despawned with the HUD.
#[derive(Component)]
struct ElementWheelRoot;

/// One phase node in the pentagon. `sync_element_wheel` highlights the one that
/// matches the aimed target's (or, when idle, the actor's) effective phase.
#[derive(Component, Clone, Copy)]
struct WheelPip {
    phase: Phase,
}

/// The central live-multiplier readout ("×1.5" / "×0.66" / "—").
#[derive(Component)]
struct WheelMultiplier;

/// The element label under the wheel (e.g. "Fire · Yō").
#[derive(Component)]
struct WheelElementLabel;

// ---------------------------------------------------------------------------
// Lifetime
// ---------------------------------------------------------------------------

/// Spawn the HUD when a player turn begins; despawn when none is pending.
/// Re-spawns each turn so the option list tracks newly-unlocked abilities and
/// picked-up items.
fn manage_combat_hud_lifetime(
    mut commands: Commands,
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    item_catalog: Option<Res<InventoryItemCatalog>>,
    mut state: ResMut<CombatHudState>,
    abilities_q: Query<&Abilities>,
    inventory_q: Query<&Inventory>,
    hud_q: Query<Entity, With<CombatHudRoot>>,
    wheel_q: Query<Entity, With<ElementWheelRoot>>,
) {
    let should_show = game_state.0 == Game_State::Battle && pending.entity.is_some();
    let already_shown = !hud_q.is_empty();

    if should_show && !already_shown {
        let actor = pending.entity.unwrap();
        let abilities = abilities_q.get(actor).map(|a| a.0.clone()).unwrap_or_default();
        let items = inventory_q
            .get(actor)
            .map(|i| i.item_ids.clone())
            .unwrap_or_default();
        let cat_count = spawn_combat_hud(
            &mut commands,
            &abilities,
            &items,
            ability_tree.as_deref(),
            item_catalog.as_deref(),
        );
        spawn_element_wheel(&mut commands);
        state.mode = HudMode::Idle;
        state.open = None;
        state.cat_focus = 0;
        state.opt_focus = 0;
        state.cat_count = cat_count;
        state.target = None;
    } else if !should_show && already_shown {
        for entity in hud_q.iter() {
            commands.entity(entity).despawn();
        }
        for entity in wheel_q.iter() {
            commands.entity(entity).despawn();
        }
        *state = CombatHudState::default();
    }
}

/// Build the command bar. Returns the number of top-level categories spawned.
///
/// Layout (bottom-centre, grows upward):
/// ```text
///   ┌ flyout (only when a category is open) ┐
///   └───────────────────────────────────────┘
///   ┌ name · resources ─────────────────────┐
///   │ 攻 Attack · 守 Defend · 術 · 具 · 待   │
///   │ hint line                              │
///   └───────────────────────────────────────┘
/// ```
fn spawn_combat_hud(
    commands: &mut Commands,
    ability_ids: &[u16],
    item_ids: &[u16],
    ability_tree: Option<&Ability_Tree>,
    item_catalog: Option<&InventoryItemCatalog>,
) -> usize {
    // Resolve the actor's abilities once (placeholders for ids the tree can't
    // find) so we can both decide which categories exist and fill the flyouts.
    let mut abilities: Vec<Ability> = ability_ids
        .iter()
        .filter_map(|&id| ability_tree.and_then(|t| t.0.find(id)))
        .collect();
    for &id in ability_ids {
        if !abilities.iter().any(|a| a.id == id) {
            abilities.push(placeholder_ability(id));
        }
    }
    let has_skills = !abilities.is_empty();
    let has_items = !item_ids.is_empty();

    // Build the ordered category list. Direct actions fire immediately; Skills /
    // Items expand a flyout and only appear when the actor has any.
    let mut categories: Vec<(CategoryKind, &'static str)> = vec![
        (CategoryKind::Direct(HudAction::Attack), "攻 Attack"),
        (CategoryKind::Direct(HudAction::Defend), "守 Defend"),
    ];
    if has_skills {
        categories.push((CategoryKind::Open(FlyoutKind::Skills), "術 Skills"));
    }
    if has_items {
        categories.push((CategoryKind::Open(FlyoutKind::Items), "具 Items"));
    }
    categories.push((CategoryKind::Direct(HudAction::Wait), "待 Wait"));
    let cat_count = categories.len();

    // Full-width, transparent anchor pinned to the bottom; its children stack
    // upward and stay horizontally centred at any resolution (no pixel maths).
    let root = (
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(spacing::XL),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(spacing::SM),
            ..default()
        },
        CombatHudRoot,
    );

    commands.spawn(root).with_children(|col| {
        // Flyouts sit above the bar (earlier children render higher in a
        // column). Hidden until opened by `sync_flyout_visibility`.
        if has_skills {
            spawn_skills_flyout(col, &abilities);
        }
        if has_items {
            spawn_items_flyout(col, item_ids, item_catalog);
        }

        // The bar itself: header, category row, hint.
        col.spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(spacing::XS),
                padding: UiRect::axes(Val::Px(spacing::LG), Val::Px(spacing::SM)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::LG)),
                ..default()
            },
            BackgroundColor(palette::BG_PANEL),
            BorderColor::all(palette::BORDER_ACCENT),
        ))
        .with_children(|bar| {
            // Header — actor name + resources, filled in by `sync_combat_hud`.
            bar.spawn((
                text_node("", font_size::SMALL, palette::TEXT_HEADING),
                CombatHudHeader,
            ));

            // The category row.
            bar.spawn(Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(spacing::SM),
                ..default()
            })
            .with_children(|row| {
                for (index, (kind, label)) in categories.iter().enumerate() {
                    spawn_category(row, index, *kind, label);
                }
            });

            // Hint line.
            bar.spawn((
                text_node("", font_size::SMALL, palette::TEXT_SECONDARY),
                CombatHudHint,
            ));
        });
    });

    cat_count
}

/// Spawn the Skills flyout (abilities grouped by school / techniques). Hidden
/// until opened. Option indices are local to this flyout.
fn spawn_skills_flyout(parent: &mut ChildSpawnerCommands, abilities: &[Ability]) {
    let mut index = 0usize;
    parent
        .spawn((
            hidden_flyout(),
            CombatHudFlyout { kind: FlyoutKind::Skills },
        ))
        .with_children(|fly| {
            for group in ABILITY_GROUPS {
                let in_group: Vec<&Ability> =
                    abilities.iter().filter(|a| group.matches(a)).collect();
                if in_group.is_empty() {
                    continue;
                }
                section_label(fly, group.label);
                for ability in in_group {
                    let label = format_ability_label(ability);
                    spawn_option(
                        fly,
                        &mut index,
                        &label,
                        HudAction::Ability(ability.id),
                        FlyoutKind::Skills,
                    );
                }
            }
        });
}

/// Spawn the Items flyout. Hidden until opened.
fn spawn_items_flyout(
    parent: &mut ChildSpawnerCommands,
    item_ids: &[u16],
    item_catalog: Option<&InventoryItemCatalog>,
) {
    let mut index = 0usize;
    parent
        .spawn((
            hidden_flyout(),
            CombatHudFlyout { kind: FlyoutKind::Items },
        ))
        .with_children(|fly| {
            for &id in item_ids {
                let label = item_catalog
                    .and_then(|c| c.0.get(&id))
                    .map(format_item_label)
                    .unwrap_or_else(|| format!("Item {id}"));
                spawn_option(fly, &mut index, &label, HudAction::Item(id), FlyoutKind::Items);
            }
        });
}

/// A flyout panel bundle that starts hidden (`Display::None`); shown by
/// `sync_flyout_visibility` when its category is opened.
fn hidden_flyout() -> impl Bundle {
    (
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            min_width: Val::Px(220.0),
            row_gap: Val::Px(spacing::XS),
            padding: UiRect::all(Val::Px(spacing::SM)),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(radius::MD)),
            ..default()
        },
        BackgroundColor(palette::BG_PANEL),
        BorderColor::all(palette::BORDER_ACCENT),
    )
}

// ---------------------------------------------------------------------------
// 五行 element wheel
// ---------------------------------------------------------------------------

/// Phase tint for a pip (linear sRGB), matching the §10 colour intent.
fn phase_color(phase: Phase) -> Color {
    match phase {
        Phase::Wood => Color::srgb(0.32, 0.62, 0.34),
        Phase::Fire => Color::srgb(0.82, 0.36, 0.26),
        Phase::Earth => Color::srgb(0.74, 0.58, 0.30),
        Phase::Metal => Color::srgb(0.68, 0.71, 0.78),
        Phase::Water => Color::srgb(0.34, 0.54, 0.84),
    }
}

/// `木 Wood` etc. — kanji + name, used by the element label under the wheel.
fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Wood => "木 Wood",
        Phase::Fire => "火 Fire",
        Phase::Earth => "土 Earth",
        Phase::Metal => "金 Metal",
        Phase::Water => "水 Water",
    }
}

/// Bare kanji shown inside a (small, circular) pip.
fn phase_kanji(phase: Phase) -> &'static str {
    match phase {
        Phase::Wood => "木",
        Phase::Fire => "火",
        Phase::Earth => "土",
        Phase::Metal => "金",
        Phase::Water => "水",
    }
}

/// (left, top) of each 30px circular pip inside the 140×124 pentagon box —
/// Wood at the apex, then clockwise (Fire, Earth, Metal, Water).
fn pip_offset(phase: Phase) -> (f32, f32) {
    match phase {
        Phase::Wood => (55.0, 1.0),
        Phase::Fire => (101.0, 34.0),
        Phase::Earth => (83.0, 88.0),
        Phase::Metal => (27.0, 88.0),
        Phase::Water => (9.0, 34.0),
    }
}

/// Pip diameter (circular).
const WHEEL_PIP: f32 = 30.0;

const WHEEL_PHASES: [Phase; 5] =
    [Phase::Wood, Phase::Fire, Phase::Earth, Phase::Metal, Phase::Water];

/// Build the wheel widget. A full-width, top-anchored wrapper centres a compact
/// panel of circular phase pips around a live 剋 multiplier readout. Spawned
/// hidden; `sync_element_wheel` reveals it only while a relevant elemental
/// matchup is being aimed, and fills in the highlight / values each frame.
fn spawn_element_wheel(commands: &mut Commands) {
    // Responsive centring wrapper (no pixel-margin hack); toggled by sync.
    let root = (
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(spacing::XL),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            display: Display::None,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Center,
            ..default()
        },
        ElementWheelRoot,
    );

    commands.spawn(root).with_children(|wrap| {
        wrap.spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(spacing::XS),
                padding: UiRect::all(Val::Px(spacing::SM)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(radius::MD)),
                ..default()
            },
            BackgroundColor(palette::BG_OVERLAY),
            BorderColor::all(palette::BORDER_SUBTLE),
        ))
        .with_children(|col| {
            col.spawn(text_node("五行", font_size::SMALL, palette::ACCENT_PRIMARY));

            // Pentagon box: circular pips positioned absolutely, multiplier centre.
            col.spawn(Node {
                position_type: PositionType::Relative,
                width: Val::Px(140.0),
                height: Val::Px(124.0),
                ..default()
            })
            .with_children(|wheel| {
                for phase in WHEEL_PHASES {
                    let (left, top) = pip_offset(phase);
                    wheel
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(left),
                                top: Val::Px(top),
                                width: Val::Px(WHEEL_PIP),
                                height: Val::Px(WHEEL_PIP),
                                display: Display::Flex,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border: UiRect::all(Val::Px(1.5)),
                                border_radius: BorderRadius::all(Val::Px(WHEEL_PIP * 0.5)),
                                ..default()
                            },
                            BackgroundColor(phase_color(phase).with_alpha(0.28)),
                            BorderColor::all(palette::BORDER_SUBTLE),
                            WheelPip { phase },
                        ))
                        .with_children(|pip| {
                            pip.spawn(text_node(
                                phase_kanji(phase),
                                font_size::LABEL,
                                palette::TEXT_PRIMARY,
                            ));
                        });
                }

                // Centre readout — the live multiplier.
                wheel
                    .spawn(Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(42.0),
                        top: Val::Px(48.0),
                        width: Val::Px(56.0),
                        height: Val::Px(32.0),
                        display: Display::Flex,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    })
                    .with_children(|c| {
                        c.spawn((
                            text_node("—", font_size::BODY_LG, palette::TEXT_DIM),
                            WheelMultiplier,
                        ));
                    });
            });

            // Element label (target's effective element, or the actor's own).
            col.spawn((
                text_node("", font_size::SMALL, palette::TEXT_SECONDARY),
                WheelElementLabel,
            ));
        });
    });
}

// ---------------------------------------------------------------------------
// Grouping
// ---------------------------------------------------------------------------

struct AbilityGroup {
    label: &'static str,
    school: Option<MagicSchool>,
}

impl AbilityGroup {
    fn matches(&self, a: &Ability) -> bool {
        match self.school {
            // Magic group: ability has a cost and matches the school.
            Some(s) => a.magic_cost > 0.0 && std::mem::discriminant(&a.magic_school) == std::mem::discriminant(&s),
            // Techniques group: no magic cost.
            None => a.magic_cost <= 0.0,
        }
    }
}

const ABILITY_GROUPS: &[AbilityGroup] = &[
    AbilityGroup { label: "Kihō",       school: Some(MagicSchool::Kiho) },
    AbilityGroup { label: "Onmyōdō",    school: Some(MagicSchool::Onmyodo) },
    AbilityGroup { label: "Yōkaijutsu", school: Some(MagicSchool::Yokaijutsu) },
    AbilityGroup { label: "Kamishin",   school: Some(MagicSchool::Kamishin) },
    AbilityGroup { label: "Techniques", school: None },
];

// ---------------------------------------------------------------------------
// UI building helpers
// ---------------------------------------------------------------------------

fn text_node(text: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont { font_size: size, ..default() },
        TextColor(color),
    )
}

fn section_label(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Node {
            margin: UiRect::top(Val::Px(spacing::XS)),
            ..default()
        },
        text_node(text, font_size::SMALL, palette::ACCENT_PRIMARY),
    ));
}

/// A category button on the command bar. `index` is its position in the row.
fn spawn_category(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    kind: CategoryKind,
    label: &str,
) {
    parent
        .spawn((
            Button::default(),
            Node {
                min_height: Val::Px(34.0),
                display: Display::Flex,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(spacing::MD), Val::Px(spacing::XS)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::SM)),
                ..default()
            },
            BackgroundColor(palette::BG_BUTTON),
            BorderColor::all(palette::BORDER),
            CombatHudCategory { index, kind },
        ))
        .with_children(|btn| {
            btn.spawn((
                text_node(label, font_size::LABEL, palette::TEXT_PRIMARY),
                CombatHudCategoryLabel,
            ));
        });
}

/// A selectable option inside a flyout. `index` is local to that flyout.
fn spawn_option(
    parent: &mut ChildSpawnerCommands,
    index: &mut usize,
    label: &str,
    action: HudAction,
    flyout: FlyoutKind,
) {
    let i = *index;
    *index += 1;
    let display = format!("{}. {label}", i + 1);
    parent
        .spawn((
            Button::default(),
            Node {
                min_height: Val::Px(32.0),
                display: Display::Flex,
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(spacing::MD), Val::Px(spacing::XS)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::SM)),
                ..default()
            },
            BackgroundColor(palette::BG_BUTTON),
            BorderColor::all(palette::BORDER),
            CombatHudOption { index: i, action, flyout },
        ))
        .with_children(|btn| {
            btn.spawn((
                text_node(&display, font_size::LABEL, palette::TEXT_PRIMARY),
                CombatHudOptionLabel,
            ));
        });
}

// ---------------------------------------------------------------------------
// Labels & formatting
// ---------------------------------------------------------------------------

fn format_ability_label(ability: &Ability) -> String {
    let mut badges = Vec::new();
    if ability.action_point_cost > 0 {
        badges.push(format!("{} AP", ability.action_point_cost));
    }
    if ability.magic_cost > 0.0 {
        badges.push(format!(
            "{:.0} {}",
            ability.magic_cost,
            magic_school_short(ability.magic_school)
        ));
    }
    if ability.cooldown > 0 {
        badges.push(format!("CD {}", ability.cooldown));
    }
    if badges.is_empty() {
        ability.name.clone()
    } else {
        format!("{}   [{}]", ability.name, badges.join(" · "))
    }
}

fn format_item_label(def: &crate::combat_plugin::InventoryItemDefinition) -> String {
    let detail = match &def.kind {
        InventoryItemKind::Consumable { effect, .. } => match effect {
            crate::combat_plugin::ConsumableEffect::Heal { amount } => format!("heal {amount}"),
        },
        InventoryItemKind::Equipment(_) => "equipment".to_string(),
    };
    format!("{}   [{} AP · {detail}]", def.name, ITEM_ACTION_POINT_COST)
}

fn magic_school_short(school: MagicSchool) -> &'static str {
    match school {
        MagicSchool::Kiho => "Ki",
        MagicSchool::Onmyodo => "On",
        MagicSchool::Yokaijutsu => "Yō",
        MagicSchool::Kamishin => "Ka",
    }
}

fn placeholder_ability(id: u16) -> Ability {
    Ability {
        id,
        next_id: None,
        name: format!("Ability {id}"),
        health_cost: 0,
        magic_cost: 0.0,
        magic_school: MagicSchool::Kiho,
        element: None,
        action_point_cost: 0,
        cooldown: 0,
        description: String::new(),
        effects: Vec::new(),
        shape: crate::combat_ability::AbilityShape::Select,
        duration: 0,
        targets: 1,
    }
}

// ---------------------------------------------------------------------------
// Affordability
// ---------------------------------------------------------------------------

/// Whether an option can be used right now, and if not, a short reason.
struct Usability {
    enabled: bool,
    reason: Option<String>,
}

impl Usability {
    fn ok() -> Self {
        Self { enabled: true, reason: None }
    }
    fn no(reason: impl Into<String>) -> Self {
        Self { enabled: false, reason: Some(reason.into()) }
    }
}

/// Context resolved once per frame for the active actor.
struct ActorCtx<'a> {
    stats: &'a CombatStats,
    gates: crate::status_effects::ActionGates,
    cost_mult: f32,
    mults: MagicCostMultipliers,
    tree: Option<&'a Ability_Tree>,
}

impl ActorCtx<'_> {
    fn ability(&self, id: u16) -> Option<Ability> {
        self.tree.and_then(|t| t.0.find(id))
    }

    fn scaled_magic_cost(&self, a: &Ability) -> f32 {
        a.magic_cost * self.cost_mult * self.mults.for_school(a.magic_school)
    }

    fn usability(&self, action: HudAction) -> Usability {
        match action {
            HudAction::Defend | HudAction::Wait => Usability::ok(),
            HudAction::Attack => {
                if self.gates.block_attacks {
                    Usability::no("Can't act (Terrified)")
                } else if !self.stats.action_points.can_spend(BASIC_ATTACK_ACTION_POINT_COST) {
                    Usability::no(format!("Needs {BASIC_ATTACK_ACTION_POINT_COST} AP"))
                } else {
                    Usability::ok()
                }
            }
            HudAction::Item(_) => {
                if self.gates.block_attacks {
                    Usability::no("Can't act (Terrified)")
                } else if self.gates.block_items {
                    Usability::no("Items blocked (Silenced)")
                } else if !self.stats.action_points.can_spend(ITEM_ACTION_POINT_COST) {
                    Usability::no(format!("Needs {ITEM_ACTION_POINT_COST} AP"))
                } else {
                    Usability::ok()
                }
            }
            HudAction::Ability(id) => {
                let Some(a) = self.ability(id) else { return Usability::ok() };
                if self.gates.block_attacks {
                    return Usability::no("Can't act (Terrified)");
                }
                if a.magic_cost > 0.0 && self.gates.block_magic_abilities {
                    return Usability::no("Magic blocked (Silenced)");
                }
                if !self.stats.action_points.can_spend(a.action_point_cost) {
                    return Usability::no(format!("Needs {} AP", a.action_point_cost));
                }
                let cost = self.scaled_magic_cost(&a);
                if !self.stats.pool(a.magic_school).can_spend(cost) {
                    return Usability::no(format!(
                        "Needs {:.0} {}",
                        cost,
                        magic_school_short(a.magic_school)
                    ));
                }
                Usability::ok()
            }
        }
    }
}

/// Build the actor context, or `None` if the active actor lacks stats.
fn resolve_actor<'a>(
    actor: Entity,
    stats_q: &'a Query<&CombatStats>,
    status_q: &Query<&StatusEffects>,
    mult_q: &Query<&MagicCostMultipliers>,
    tree: Option<&'a Ability_Tree>,
) -> Option<ActorCtx<'a>> {
    let stats = stats_q.get(actor).ok()?;
    let se = status_q.get(actor).ok();
    Some(ActorCtx {
        stats,
        gates: action_gates(se),
        cost_mult: magic_cost_multiplier(se),
        mults: mult_q.get(actor).copied().unwrap_or_default(),
        tree,
    })
}

// ---------------------------------------------------------------------------
// Input — mouse
// ---------------------------------------------------------------------------

/// Mouse hover focuses a category / option; a click activates it. Clicking a
/// flyout category toggles it open; clicking an option (only the visible
/// flyout's options receive pointer events) chooses it.
#[allow(clippy::too_many_arguments)]
fn handle_combat_hud_mouse(
    category_q: Query<(&Interaction, &CombatHudCategory), Changed<Interaction>>,
    option_q: Query<(&Interaction, &CombatHudOption), Changed<Interaction>>,
    mut state: ResMut<CombatHudState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    stats_q: Query<&CombatStats>,
    status_q: Query<&StatusEffects>,
    mult_q: Query<&MagicCostMultipliers>,
    enemies_q: Query<(Entity, &BattleSide), With<BattleParticipant>>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    if state.mode != HudMode::Idle {
        return;
    }
    let Some(actor) = pending.entity else { return };
    let Some(ctx) = resolve_actor(actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref())
    else {
        return;
    };

    for (interaction, category) in &category_q {
        if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            state.cat_focus = category.index;
        }
        if *interaction == Interaction::Pressed {
            activate_category(category.kind, &ctx, &mut state, &enemies_q, &mut actions);
        }
    }

    for (interaction, option) in &option_q {
        if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            state.open = Some(option.flyout);
            state.opt_focus = option.index;
        }
        if *interaction == Interaction::Pressed {
            choose_option(option.action, &ctx, &mut state, &enemies_q, &mut actions);
        }
    }
}

/// Activate a top-level category: fire a direct action, or toggle its flyout.
fn activate_category(
    kind: CategoryKind,
    ctx: &ActorCtx,
    state: &mut CombatHudState,
    enemies_q: &Query<(Entity, &BattleSide), With<BattleParticipant>>,
    actions: &mut MessageWriter<PlayerActionEvent>,
) {
    match kind {
        CategoryKind::Direct(action) => {
            choose_option(action, ctx, state, enemies_q, actions);
        }
        CategoryKind::Open(kind) => {
            if state.open == Some(kind) {
                state.open = None;
            } else {
                state.open = Some(kind);
                state.opt_focus = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Input — keyboard
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_combat_hud_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CombatHudState>,
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    stats_q: Query<&CombatStats>,
    status_q: Query<&StatusEffects>,
    mult_q: Query<&MagicCostMultipliers>,
    category_q: Query<&CombatHudCategory>,
    option_q: Query<&CombatHudOption>,
    enemies_q: Query<(Entity, &BattleSide), With<BattleParticipant>>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }
    let Some(actor) = pending.entity else { return };

    // Esc backs out: cancel targeting, or close an open flyout.
    if keys.just_pressed(KeyCode::Escape) {
        if state.mode != HudMode::Idle {
            state.mode = HudMode::Idle;
            return;
        }
        if state.open.is_some() {
            state.open = None;
            return;
        }
    }

    match state.mode {
        HudMode::AwaitingTarget(selected) => {
            let mut enemies = living_enemies(&enemies_q);
            if enemies.is_empty() {
                return;
            }
            enemies.sort();
            let forward = keys.just_pressed(KeyCode::ArrowRight)
                || keys.just_pressed(KeyCode::Tab)
                || keys.just_pressed(KeyCode::ArrowDown);
            let backward = keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::ArrowUp);
            if forward || backward {
                let cur = state
                    .target
                    .and_then(|t| enemies.iter().position(|e| *e == t))
                    .unwrap_or(0);
                let len = enemies.len();
                let next = if forward { (cur + 1) % len } else { (cur + len - 1) % len };
                state.target = Some(enemies[next]);
            }
            if state.target.is_none() {
                state.target = enemies.first().copied();
            }
            if keys.just_pressed(KeyCode::Enter) {
                if let Some(target) = state.target {
                    commit_target(selected, target, &mut state, &mut actions);
                }
            }
        }
        HudMode::Idle => {
            match state.open {
                // Navigating the open flyout.
                Some(kind) => {
                    let mut opts: Vec<&CombatHudOption> =
                        option_q.iter().filter(|o| o.flyout == kind).collect();
                    opts.sort_by_key(|o| o.index);
                    let count = opts.len().max(1);
                    if keys.just_pressed(KeyCode::ArrowDown) {
                        state.opt_focus = (state.opt_focus + 1) % count;
                    }
                    if keys.just_pressed(KeyCode::ArrowUp) {
                        state.opt_focus = (state.opt_focus + count - 1) % count;
                    }
                    for (key, idx) in NUMBER_KEYS.iter().copied() {
                        if keys.just_pressed(key) && idx < opts.len() {
                            state.opt_focus = idx;
                        }
                    }
                    // ArrowLeft backs out to the category row.
                    if keys.just_pressed(KeyCode::ArrowLeft) {
                        state.open = None;
                        return;
                    }
                    if keys.just_pressed(KeyCode::Enter) {
                        let focus = state.opt_focus;
                        if let Some(action) =
                            opts.iter().find(|o| o.index == focus).map(|o| o.action)
                        {
                            if let Some(ctx) = resolve_actor(
                                actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref(),
                            ) {
                                choose_option(action, &ctx, &mut state, &enemies_q, &mut actions);
                            }
                        }
                    }
                }
                // Navigating the top-level category row.
                None => {
                    let count = state.cat_count.max(1);
                    if keys.just_pressed(KeyCode::ArrowRight) {
                        state.cat_focus = (state.cat_focus + 1) % count;
                    }
                    if keys.just_pressed(KeyCode::ArrowLeft) {
                        state.cat_focus = (state.cat_focus + count - 1) % count;
                    }
                    for (key, idx) in NUMBER_KEYS.iter().copied() {
                        if keys.just_pressed(key) && idx < count {
                            state.cat_focus = idx;
                        }
                    }
                    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::ArrowUp) {
                        let focus = state.cat_focus;
                        if let Some(kind) =
                            category_q.iter().find(|c| c.index == focus).map(|c| c.kind)
                        {
                            if let Some(ctx) = resolve_actor(
                                actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref(),
                            ) {
                                activate_category(kind, &ctx, &mut state, &enemies_q, &mut actions);
                            }
                        }
                    }
                }
            }
        }
    }
}

const NUMBER_KEYS: &[(KeyCode, usize)] = &[
    (KeyCode::Digit1, 0),
    (KeyCode::Digit2, 1),
    (KeyCode::Digit3, 2),
    (KeyCode::Digit4, 3),
    (KeyCode::Digit5, 4),
    (KeyCode::Digit6, 5),
    (KeyCode::Digit7, 6),
    (KeyCode::Digit8, 7),
    (KeyCode::Digit9, 8),
];

// ---------------------------------------------------------------------------
// Choosing / committing actions
// ---------------------------------------------------------------------------

/// Resolve a chosen option: fire immediately, or arm target selection.
fn choose_option(
    action: HudAction,
    ctx: &ActorCtx,
    state: &mut CombatHudState,
    enemies_q: &Query<(Entity, &BattleSide), With<BattleParticipant>>,
    actions: &mut MessageWriter<PlayerActionEvent>,
) {
    if !ctx.usability(action).enabled {
        return;
    }
    match action {
        HudAction::Defend => {
            actions.write(PlayerActionEvent { action: PlayerAction::Defend });
            state.mode = HudMode::Idle;
        }
        HudAction::Wait => {
            actions.write(PlayerActionEvent { action: PlayerAction::Wait });
            state.mode = HudMode::Idle;
        }
        HudAction::Item(id) => {
            // Self-target consumables for now.
            actions.write(PlayerActionEvent {
                action: PlayerAction::UseItem(id, None),
            });
            state.mode = HudMode::Idle;
        }
        HudAction::Attack => {
            state.mode = HudMode::AwaitingTarget(SelectedAction::Attack);
            state.target = first_enemy(enemies_q);
        }
        HudAction::Ability(id) => {
            state.mode = HudMode::AwaitingTarget(SelectedAction::Ability(id));
            state.target = first_enemy(enemies_q);
        }
    }
}

fn commit_target(
    selected: SelectedAction,
    target: Entity,
    state: &mut CombatHudState,
    actions: &mut MessageWriter<PlayerActionEvent>,
) {
    match selected {
        SelectedAction::Attack => {
            actions.write(PlayerActionEvent { action: PlayerAction::Attack(target) });
        }
        SelectedAction::Ability(id) => {
            actions.write(PlayerActionEvent {
                action: PlayerAction::UseAbility(id as u32, target),
            });
        }
    }
    state.mode = HudMode::Idle;
    state.target = None;
}

// ---------------------------------------------------------------------------
// World target click
// ---------------------------------------------------------------------------

/// When awaiting a target, a left-click inside [`TARGET_PICK_RADIUS`] of an
/// enemy commits the chosen action. Right-click cancels. Consumes the click so
/// it isn't also read as a move request.
fn handle_target_click(
    mut state: ResMut<CombatHudState>,
    mut mouse_input: ResMut<ButtonInput<MouseButton>>,
    game_state: Res<GameState>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    windows: Query<&Window>,
    enemies_q: Query<(Entity, &Transform, &BattleSide), With<BattleParticipant>>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }
    let HudMode::AwaitingTarget(selected) = state.mode else {
        return;
    };

    if mouse_input.just_pressed(MouseButton::Right) {
        state.mode = HudMode::Idle;
        state.target = None;
        mouse_input.clear_just_pressed(MouseButton::Right);
        return;
    }
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }

    let Some((camera, camera_tf)) = camera_q.iter().next() else { return };
    let Some(window) = windows.iter().next() else { return };
    let Some(screen_pos) = window.cursor_position() else { return };
    let Some(cursor_world) = crate::render3d::cursor_to_ground(camera, camera_tf, screen_pos)
    else {
        return;
    };

    if let Some(target) = nearest_enemy_within(&enemies_q, cursor_world, TARGET_PICK_RADIUS) {
        commit_target(selected, target, &mut state, &mut actions);
    }
    // Swallow the click either way so we don't accidentally move the avatar.
    mouse_input.clear_just_pressed(MouseButton::Left);
}

fn living_enemies(
    enemies_q: &Query<(Entity, &BattleSide), With<BattleParticipant>>,
) -> Vec<Entity> {
    enemies_q
        .iter()
        .filter(|(_, side)| **side == BattleSide::Enemy)
        .map(|(e, _)| e)
        .collect()
}

fn first_enemy(
    enemies_q: &Query<(Entity, &BattleSide), With<BattleParticipant>>,
) -> Option<Entity> {
    let mut v = living_enemies(enemies_q);
    v.sort();
    v.first().copied()
}

fn nearest_enemy_within(
    enemies_q: &Query<(Entity, &Transform, &BattleSide), With<BattleParticipant>>,
    cursor_world: Vec2,
    radius: f32,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (entity, tf, side) in enemies_q.iter() {
        if *side != BattleSide::Enemy {
            continue;
        }
        let d = tf.translation.truncate().distance(cursor_world);
        if d > radius {
            continue;
        }
        if best.map(|(_, prev)| d < prev).unwrap_or(true) {
            best = Some((entity, d));
        }
    }
    best.map(|(e, _)| e)
}

// ---------------------------------------------------------------------------
// Visual sync (PostUpdate)
// ---------------------------------------------------------------------------

/// Toggle each flyout's visibility to match the open category.
fn sync_flyout_visibility(
    state: Res<CombatHudState>,
    mut flyout_q: Query<(&CombatHudFlyout, &mut Node)>,
) {
    for (flyout, mut node) in &mut flyout_q {
        let want = if state.mode == HudMode::Idle && state.open == Some(flyout.kind) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_combat_hud(
    state: Res<CombatHudState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    stats_q: Query<&CombatStats>,
    status_q: Query<&StatusEffects>,
    mult_q: Query<&MagicCostMultipliers>,
    name_q: Query<&Name>,
    mut category_q: Query<
        (
            &CombatHudCategory,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
        ),
        Without<CombatHudOption>,
    >,
    mut options_q: Query<
        (
            &CombatHudOption,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
        ),
        Without<CombatHudCategory>,
    >,
    mut cat_text_q: Query<
        &mut TextColor,
        (With<CombatHudCategoryLabel>, Without<CombatHudOptionLabel>),
    >,
    mut text_q: Query<
        &mut TextColor,
        (With<CombatHudOptionLabel>, Without<CombatHudCategoryLabel>),
    >,
    mut header_q: Query<&mut Text, With<CombatHudHeader>>,
) {
    let Some(actor) = pending.entity else { return };
    let Some(ctx) = resolve_actor(actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref())
    else {
        return;
    };

    let idle = state.mode == HudMode::Idle;

    // Per-category appearance.
    for (category, interaction, mut bg, mut border, children) in &mut category_q {
        // A Direct category dims when its action can't be used; flyouts never dim.
        let usable = match category.kind {
            CategoryKind::Direct(action) => ctx.usability(action).enabled,
            CategoryKind::Open(_) => true,
        };
        let is_open = matches!(category.kind, CategoryKind::Open(k) if state.open == Some(k));
        let focused = idle && state.open.is_none() && category.index == state.cat_focus;
        let hovered = *interaction == Interaction::Hovered;
        let pressed = *interaction == Interaction::Pressed;

        let (bg_c, border_c, text_c) = if !usable {
            (palette::BG_PANEL_SUNK, palette::BORDER_SUBTLE, palette::TEXT_DIM)
        } else if pressed || is_open {
            (palette::BG_BUTTON_PRESSED, palette::BORDER_PRESSED, palette::TEXT_HEADING)
        } else if focused || hovered {
            (palette::BG_BUTTON_HOVER, palette::BORDER_ACCENT, palette::TEXT_HEADING)
        } else {
            (palette::BG_BUTTON, palette::BORDER, palette::TEXT_PRIMARY)
        };

        bg.0 = bg_c;
        set_border(&mut border, border_c);
        for child in children.iter() {
            if let Ok(mut tc) = cat_text_q.get_mut(child) {
                tc.0 = text_c;
            }
        }
    }

    // Per-option appearance (only the open flyout's options are focusable).
    for (option, interaction, mut bg, mut border, children) in &mut options_q {
        let usable = ctx.usability(option.action);
        let focused =
            idle && state.open == Some(option.flyout) && option.index == state.opt_focus;
        let hovered = *interaction == Interaction::Hovered;
        let pressed = *interaction == Interaction::Pressed;

        let (bg_c, border_c, text_c) = if !usable.enabled {
            (
                palette::BG_PANEL_SUNK,
                palette::BORDER_SUBTLE,
                palette::TEXT_DIM,
            )
        } else if pressed {
            (
                palette::BG_BUTTON_PRESSED,
                palette::BORDER_PRESSED,
                palette::TEXT_HEADING,
            )
        } else if focused || hovered {
            (
                palette::BG_BUTTON_HOVER,
                palette::BORDER_ACCENT,
                palette::TEXT_HEADING,
            )
        } else {
            (palette::BG_BUTTON, palette::BORDER, palette::TEXT_PRIMARY)
        };

        bg.0 = bg_c;
        set_border(&mut border, border_c);
        for child in children.iter() {
            if let Ok(mut tc) = text_q.get_mut(child) {
                tc.0 = text_c;
            }
        }
    }

    // Header — actor name + live resources.
    if let Ok(mut header) = header_q.single_mut() {
        let name = name_q.get(actor).map(|n| n.as_str()).unwrap_or("Combatant");
        let mut parts = vec![
            name.to_string(),
            format!(
                "AP {}/{}",
                ctx.stats.action_points.current, ctx.stats.action_points.base
            ),
        ];
        for (school, label) in [
            (MagicSchool::Kiho, "Ki"),
            (MagicSchool::Onmyodo, "On"),
            (MagicSchool::Yokaijutsu, "Yō"),
            (MagicSchool::Kamishin, "Ka"),
        ] {
            let pool = ctx.stats.pool(school);
            if pool.base > 0.0 {
                parts.push(format!("{label} {:.0}/{:.0}", pool.current, pool.base));
            }
        }
        let desired = parts.join("    ");
        if header.0 != desired {
            header.0 = desired;
        }
    }
}

/// Fill the bottom hint line: describe the focused option while idle, or the
/// aimed target + estimate while picking. Kept separate from `sync_combat_hud`
/// so it can read the option list immutably without fighting the appearance
/// pass for the option text borrow.
#[allow(clippy::too_many_arguments)]
fn sync_hint(
    state: Res<CombatHudState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    item_catalog: Option<Res<InventoryItemCatalog>>,
    stats_q: Query<&CombatStats>,
    status_q: Query<&StatusEffects>,
    mult_q: Query<&MagicCostMultipliers>,
    name_q: Query<&Name>,
    category_q: Query<&CombatHudCategory>,
    options_q: Query<&CombatHudOption>,
    mut hint_q: Query<&mut Text, With<CombatHudHint>>,
) {
    let Ok(mut hint) = hint_q.single_mut() else { return };
    let Some(actor) = pending.entity else { return };
    let Some(ctx) = resolve_actor(actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref())
    else {
        return;
    };

    let desired = match state.mode {
        HudMode::AwaitingTarget(selected) => {
            let target_name = state
                .target
                .and_then(|t| name_q.get(t).ok())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "—".to_string());
            let (what, est) = match selected {
                SelectedAction::Attack => {
                    ("Attack".to_string(), format!("≈{} dmg", ctx.stats.lethality.current))
                }
                SelectedAction::Ability(id) => match ctx.ability(id) {
                    Some(a) => (a.name.clone(), ability_estimate(&a)),
                    None => ("Ability".to_string(), String::new()),
                },
            };
            format!(
                "{what} → {target_name} {est}  ·  ←/→ cycle · Enter confirm · Esc cancel · or click an enemy"
            )
        }
        HudMode::Idle => match state.open {
            // Describe the focused option inside the open flyout.
            Some(kind) => options_q
                .iter()
                .find(|o| o.flyout == kind && o.index == state.opt_focus)
                .map(|o| describe_action(o.action, &ctx, item_catalog.as_deref()))
                .unwrap_or_else(|| {
                    "↑/↓ choose · Enter use · ←/Esc back".to_string()
                }),
            // Describe the focused top-level category.
            None => category_q
                .iter()
                .find(|c| c.index == state.cat_focus)
                .map(|c| describe_category(c.kind, &ctx))
                .unwrap_or_else(|| {
                    "←/→ choose · Enter / ↑ open · click to act".to_string()
                }),
        },
    };

    if hint.0 != desired {
        hint.0 = desired;
    }
}

/// One-line hint for a focused top-level category.
fn describe_category(kind: CategoryKind, ctx: &ActorCtx) -> String {
    match kind {
        CategoryKind::Direct(action) => describe_action(action, ctx, None),
        CategoryKind::Open(FlyoutKind::Skills) => {
            "Skills — abilities by school. Enter / ↑ to open.".to_string()
        }
        CategoryKind::Open(FlyoutKind::Items) => {
            "Items — usable consumables. Enter / ↑ to open.".to_string()
        }
    }
}

/// Drive the 五行 wheel widget: highlight the phase pip for the aimed target's
/// (or, when idle, the actor's own) **effective** element, and show the live
/// 剋 multiplier of the selected on-wheel ability against that target. Reads
/// the same `effective_element` + `damage_multiplier_overloaded` the damage
/// pipeline uses, so the readout matches what the hit will actually do.
#[allow(clippy::type_complexity)]
fn sync_element_wheel(
    state: Res<CombatHudState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    affinity_q: Query<&ElementalAffinity>,
    attune_q: Query<&Attunement>,
    flip_q: Query<(), With<PolarityFlip>>,
    stats_q: Query<&CombatStats>,
    mut root_q: Query<&mut Node, With<ElementWheelRoot>>,
    mut pip_q: Query<(&WheelPip, &mut BackgroundColor, &mut BorderColor)>,
    mut mult_q: Query<
        (&mut Text, &mut TextColor),
        (With<WheelMultiplier>, Without<WheelElementLabel>),
    >,
    mut label_q: Query<&mut Text, (With<WheelElementLabel>, Without<WheelMultiplier>)>,
) {
    let Some(actor) = pending.entity else { return };

    let eff = |e: Entity| {
        effective_element(affinity_q.get(e).ok(), attune_q.get(e).ok(), flip_q.get(e).is_ok())
    };

    // The chosen action's element (None for a basic attack / off-wheel) + target.
    let (sel_element, target): (Option<Element>, Option<Entity>) = match state.mode {
        HudMode::AwaitingTarget(SelectedAction::Ability(id)) => (
            ability_tree.as_deref().and_then(|t| t.0.find(id)).and_then(|a| a.element),
            state.target,
        ),
        HudMode::AwaitingTarget(SelectedAction::Attack) => (None, state.target),
        HudMode::Idle => (None, None),
    };

    // Highlight the target's element while aiming; otherwise the actor's own.
    let highlight = match target {
        Some(t) => eff(t),
        None => eff(actor),
    };

    // The wheel is only meaningful while aiming at an element-bearing target
    // (or casting an on-wheel ability). Hide it the rest of the time so it
    // doesn't clutter the idle menu.
    let relevant =
        matches!(state.mode, HudMode::AwaitingTarget(_)) && (highlight.is_some() || sel_element.is_some());
    if let Ok(mut node) = root_q.single_mut() {
        let want = if relevant { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    if !relevant {
        return;
    }

    // Live multiplier — only when an on-wheel ability is aimed at an element-bearing target.
    let (mult_text, mult_color) = match (sel_element, target.and_then(|t| eff(t).map(|d| (t, d)))) {
        (Some(s), Some((t, d))) => {
            let ap = stats_q.get(actor).map(|cs| cs.mind.current as f32).unwrap_or(0.0);
            let dp = stats_q.get(t).map(|cs| cs.mind.current as f32).unwrap_or(0.0);
            let m = damage_multiplier_overloaded(s, d, ap, dp, OVERLOAD_THRESHOLD);
            let color = if m > 1.001 {
                palette::ACCENT_SUCCESS
            } else if m < 0.999 {
                palette::ACCENT_DANGER
            } else {
                palette::TEXT_DIM
            };
            (format!("×{m:.2}"), color)
        }
        _ => ("—".to_string(), palette::TEXT_DIM),
    };

    for (pip, mut bg, mut border) in &mut pip_q {
        let is_hi = highlight.map(|e| e.phase == pip.phase).unwrap_or(false);
        let base = phase_color(pip.phase);
        if is_hi {
            bg.0 = base.with_alpha(0.85);
            let bc = if target.is_some() && sel_element.is_some() {
                mult_color
            } else {
                palette::BORDER_ACCENT
            };
            set_border(&mut border, bc);
        } else {
            bg.0 = base.with_alpha(0.30);
            set_border(&mut border, palette::BORDER_SUBTLE);
        }
    }

    if let Ok((mut text, mut color)) = mult_q.single_mut() {
        if text.0 != mult_text {
            text.0 = mult_text;
        }
        color.0 = mult_color;
    }

    if let Ok(mut label) = label_q.single_mut() {
        let desired = match highlight {
            Some(e) => {
                let pol = match e.polarity {
                    Polarity::In => "In 陰",
                    Polarity::Yo => "Yō 陽",
                };
                let who = if target.is_some() { "target" } else { "you" };
                format!("{} · {pol}  ({who})", phase_label(e.phase))
            }
            None => "no element".to_string(),
        };
        if label.0 != desired {
            label.0 = desired;
        }
    }
}

/// One-line description of a focused option, with its cost / cooldown and, if
/// it can't be used right now, the reason.
fn describe_action(
    action: HudAction,
    ctx: &ActorCtx,
    item_catalog: Option<&InventoryItemCatalog>,
) -> String {
    let body = match action {
        HudAction::Attack => "Basic weapon attack on one enemy.".to_string(),
        HudAction::Defend => "Brace: reduce incoming damage until your next turn.".to_string(),
        HudAction::Wait => "Pass the rest of your turn.".to_string(),
        HudAction::Item(id) => item_catalog
            .and_then(|c| c.0.get(&id))
            .map(|d| match &d.kind {
                InventoryItemKind::Consumable { effect, .. } => match effect {
                    crate::combat_plugin::ConsumableEffect::Heal { amount } => {
                        format!("{}: restore {amount} health.", d.name)
                    }
                },
                InventoryItemKind::Equipment(_) => format!("{}: equipment.", d.name),
            })
            .unwrap_or_else(|| "Use item.".to_string()),
        HudAction::Ability(id) => match ctx.ability(id) {
            Some(a) if !a.description.is_empty() => a.description.clone(),
            Some(a) => format!("{}.", a.name),
            None => "Ability.".to_string(),
        },
    };

    match ctx.usability(action).reason {
        Some(reason) => format!("⚠ {reason} — {body}"),
        None => body,
    }
}

fn ability_estimate(a: &Ability) -> String {
    use crate::combat_ability::AbilityEffect;
    for e in &a.effects {
        match e {
            AbilityEffect::Damage { floor, ceiling, .. } => {
                return format!("≈{floor}–{ceiling} dmg");
            }
            AbilityEffect::Heal { floor, ceiling, .. } => {
                return format!("≈{floor}–{ceiling} heal");
            }
            AbilityEffect::DrainMorale { floor, ceiling, .. } => {
                return format!("≈{floor}–{ceiling} sanity");
            }
            _ => {}
        }
    }
    String::new()
}

fn set_border(border: &mut BorderColor, color: Color) {
    border.top = color;
    border.right = color;
    border.bottom = color;
    border.left = color;
}

// ---------------------------------------------------------------------------
// Floating target marker
// ---------------------------------------------------------------------------

fn sync_target_marker(
    mut commands: Commands,
    state: Res<CombatHudState>,
    game_state: Res<GameState>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    target_tf_q: Query<&Transform, With<BattleParticipant>>,
    mut marker_q: Query<(Entity, &mut Node), With<CombatHudTargetMarker>>,
) {
    let active = game_state.0 == Game_State::Battle
        && matches!(state.mode, HudMode::AwaitingTarget(_))
        && state.target.is_some();

    if !active {
        for (e, _) in &marker_q {
            commands.entity(e).despawn();
        }
        return;
    }

    let target = state.target.unwrap();
    let Ok(tf) = target_tf_q.get(target) else { return };
    let Some((camera, cam_tf)) = camera_q.iter().next() else { return };
    let world = tf.translation + Vec3::new(0.0, 0.0, 48.0);
    let Ok(screen) = camera.world_to_viewport(cam_tf, world) else {
        return;
    };

    if let Some((_, mut node)) = marker_q.iter_mut().next() {
        node.left = Val::Px(screen.x - 12.0);
        node.top = Val::Px(screen.y - 24.0);
    } else {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(screen.x - 12.0),
                top: Val::Px(screen.y - 24.0),
                ..default()
            },
            Text::new("▼"),
            TextFont { font_size: 28.0, ..default() },
            TextColor(palette::ACCENT_DANGER),
            CombatHudTargetMarker,
        ));
    }
}

/// While an action is armed (`AwaitingTarget`), show a small chip that follows
/// the cursor naming what will fire on the next click. Spawned on demand and
/// repositioned each frame; despawned as soon as targeting ends.
fn sync_action_cursor(
    mut commands: Commands,
    state: Res<CombatHudState>,
    game_state: Res<GameState>,
    ability_tree: Option<Res<Ability_Tree>>,
    windows: Query<&Window>,
    mut chip_q: Query<(Entity, &mut Node, &Children), With<CombatHudCursorChip>>,
    mut text_q: Query<&mut Text>,
) {
    let armed = game_state.0 == Game_State::Battle;
    let label = match (armed, state.mode) {
        (true, HudMode::AwaitingTarget(SelectedAction::Attack)) => Some("⚔ Attack".to_string()),
        (true, HudMode::AwaitingTarget(SelectedAction::Ability(id))) => Some(format!(
            "✦ {}",
            ability_tree
                .as_deref()
                .and_then(|t| t.0.find(id))
                .map(|a| a.name)
                .unwrap_or_else(|| "Ability".to_string())
        )),
        _ => None,
    };

    let Some(label) = label else {
        for (e, _, _) in &chip_q {
            commands.entity(e).despawn();
        }
        return;
    };

    let cursor = windows.iter().next().and_then(|w| w.cursor_position());
    let Some(cursor) = cursor else { return };
    let (left, top) = (cursor.x + 18.0, cursor.y + 18.0);

    if let Some((_, mut node, children)) = chip_q.iter_mut().next() {
        node.left = Val::Px(left);
        node.top = Val::Px(top);
        for child in children.iter() {
            if let Ok(mut text) = text_q.get_mut(child) {
                if text.0 != label {
                    text.0 = label.clone();
                }
            }
        }
    } else {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left),
                    top: Val::Px(top),
                    padding: UiRect::axes(Val::Px(spacing::SM), Val::Px(spacing::XS)),
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::all(Val::Px(radius::SM)),
                    ..default()
                },
                BackgroundColor(palette::BG_PANEL),
                BorderColor::all(palette::ACCENT_DANGER),
                CombatHudCursorChip,
            ))
            .with_children(|c| {
                c.spawn(text_node(&label, font_size::SMALL, palette::TEXT_HEADING));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_default() {
        assert_eq!(CombatHudState::default().mode, HudMode::Idle);
    }

    #[test]
    fn awaiting_target_compares_by_action() {
        let a = HudMode::AwaitingTarget(SelectedAction::Ability(42));
        let b = HudMode::AwaitingTarget(SelectedAction::Ability(42));
        let c = HudMode::AwaitingTarget(SelectedAction::Ability(43));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, HudMode::Idle);
    }

    #[test]
    fn group_matches_split_magic_and_techniques() {
        let mut magic = placeholder_ability(1);
        magic.magic_cost = 5.0;
        magic.magic_school = MagicSchool::Onmyodo;
        let technique = placeholder_ability(2); // magic_cost 0

        let onmyodo = &ABILITY_GROUPS[1];
        let techniques = &ABILITY_GROUPS[4];
        assert!(onmyodo.matches(&magic));
        assert!(!onmyodo.matches(&technique));
        assert!(techniques.matches(&technique));
        assert!(!techniques.matches(&magic));
    }
}
