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
    InventoryItemCatalog, InventoryItemKind, Name, PendingPlayerAction, PlayerAction,
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
            .add_systems(PostUpdate, (sync_combat_hud, sync_hint))
            .add_systems(PostUpdate, sync_target_marker.after(sync_combat_hud))
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
    /// Index of the keyboard-focused option (also set by mouse hover).
    focus: usize,
    /// Number of options currently spawned, for focus wrap-around.
    option_count: usize,
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

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

#[derive(Component)]
struct CombatHudRoot;

/// One selectable option. `index` is its position in the focus order.
#[derive(Component, Clone, Copy)]
struct CombatHudOption {
    index: usize,
    action: HudAction,
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
        let count = spawn_combat_hud(
            &mut commands,
            &abilities,
            &items,
            ability_tree.as_deref(),
            item_catalog.as_deref(),
        );
        spawn_element_wheel(&mut commands);
        state.mode = HudMode::Idle;
        state.focus = 0;
        state.option_count = count;
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

/// Build the panel. Returns the number of selectable options spawned.
fn spawn_combat_hud(
    commands: &mut Commands,
    ability_ids: &[u16],
    item_ids: &[u16],
    ability_tree: Option<&Ability_Tree>,
    item_catalog: Option<&InventoryItemCatalog>,
) -> usize {
    let mut index = 0usize;

    let panel = (
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(spacing::XL),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-260.0)),
            width: Val::Px(520.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(spacing::SM),
            padding: UiRect::all(Val::Px(spacing::LG)),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(radius::LG)),
            ..default()
        },
        BackgroundColor(palette::BG_PANEL),
        BorderColor::all(palette::BORDER_ACCENT),
        CombatHudRoot,
    );

    commands.spawn(panel).with_children(|col| {
        // Header — actor name + resources, filled in by `sync_combat_hud`.
        col.spawn((
            text_node("", font_size::LABEL, palette::TEXT_HEADING),
            CombatHudHeader,
        ));

        // Basic actions row.
        section_label(col, "Actions");
        button_row(col, |row| {
            spawn_option(row, &mut index, "Attack", HudAction::Attack);
            spawn_option(row, &mut index, "Defend", HudAction::Defend);
            spawn_option(row, &mut index, "Wait", HudAction::Wait);
        });

        // Items.
        if !item_ids.is_empty() {
            section_label(col, "Items");
            for &id in item_ids {
                let label = item_catalog
                    .and_then(|c| c.0.get(&id))
                    .map(format_item_label)
                    .unwrap_or_else(|| format!("Item {id}"));
                spawn_option(col, &mut index, &label, HudAction::Item(id));
            }
        }

        // Abilities grouped by school / techniques.
        if ability_ids.is_empty() {
            section_label(col, "Abilities");
            col.spawn(text_node("(none learned)", font_size::SMALL, palette::TEXT_DIM));
        } else {
            let mut abilities: Vec<Ability> = ability_ids
                .iter()
                .filter_map(|&id| ability_tree.and_then(|t| t.0.find(id)))
                .collect();
            // Keep any ids the tree couldn't resolve as bare entries.
            for &id in ability_ids {
                if !abilities.iter().any(|a| a.id == id) {
                    // Synthesize a minimal placeholder so the option still works.
                    abilities.push(placeholder_ability(id));
                }
            }

            for group in ABILITY_GROUPS {
                let in_group: Vec<&Ability> = abilities
                    .iter()
                    .filter(|a| group.matches(a))
                    .collect();
                if in_group.is_empty() {
                    continue;
                }
                section_label(col, group.label);
                for ability in in_group {
                    let label = format_ability_label(ability);
                    spawn_option(col, &mut index, &label, HudAction::Ability(ability.id));
                }
            }
        }

        // Hint line.
        col.spawn((
            Node {
                margin: UiRect::top(Val::Px(spacing::XS)),
                ..default()
            },
            text_node("", font_size::SMALL, palette::TEXT_SECONDARY),
            CombatHudHint,
        ));
    });

    index
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

/// `木 Wood` etc. — kanji + name, short enough for a 48px pip.
fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Wood => "木 Wood",
        Phase::Fire => "火 Fire",
        Phase::Earth => "土 Earth",
        Phase::Metal => "金 Metal",
        Phase::Water => "水 Water",
    }
}

/// (left, top) for each phase inside the 176×140 pentagon box — Wood at the
/// apex, then clockwise (Fire, Earth, Metal, Water), matching the §2 diagram.
fn pip_offset(phase: Phase) -> (f32, f32) {
    match phase {
        Phase::Wood => (64.0, 2.0),
        Phase::Fire => (122.0, 44.0),
        Phase::Earth => (100.0, 110.0),
        Phase::Metal => (28.0, 110.0),
        Phase::Water => (6.0, 44.0),
    }
}

const WHEEL_PHASES: [Phase; 5] =
    [Phase::Wood, Phase::Fire, Phase::Earth, Phase::Metal, Phase::Water];

/// Build the top-anchored wheel widget. Pips, a central multiplier readout, and
/// an element label; `sync_element_wheel` fills in highlight/values each frame.
fn spawn_element_wheel(commands: &mut Commands) {
    let panel = (
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(spacing::XL),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-98.0)),
            width: Val::Px(196.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(spacing::XS),
            padding: UiRect::all(Val::Px(spacing::SM)),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(radius::LG)),
            ..default()
        },
        BackgroundColor(palette::BG_PANEL),
        BorderColor::all(palette::BORDER_ACCENT),
        ElementWheelRoot,
    );

    commands.spawn(panel).with_children(|col| {
        col.spawn(text_node("五行 — Elements", font_size::SMALL, palette::ACCENT_PRIMARY));

        // Pentagon box: pips positioned absolutely, multiplier in the centre.
        col.spawn(Node {
            position_type: PositionType::Relative,
            width: Val::Px(176.0),
            height: Val::Px(140.0),
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
                            width: Val::Px(48.0),
                            height: Val::Px(24.0),
                            display: Display::Flex,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(Val::Px(1.5)),
                            border_radius: BorderRadius::all(Val::Px(radius::SM)),
                            ..default()
                        },
                        BackgroundColor(phase_color(phase).with_alpha(0.30)),
                        BorderColor::all(palette::BORDER_SUBTLE),
                        WheelPip { phase },
                    ))
                    .with_children(|pip| {
                        pip.spawn(text_node(phase_label(phase), 11.0, palette::TEXT_PRIMARY));
                    });
            }

            // Centre readout — the live multiplier.
            wheel
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(58.0),
                    top: Val::Px(52.0),
                    width: Val::Px(60.0),
                    height: Val::Px(36.0),
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

fn button_row(parent: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(spacing::SM),
            ..default()
        })
        .with_children(build);
}

fn spawn_option(
    parent: &mut ChildSpawnerCommands,
    index: &mut usize,
    label: &str,
    action: HudAction,
) {
    let i = *index;
    *index += 1;
    let display = format!("{}. {label}", i + 1);
    parent
        .spawn((
            Button::default(),
            Node {
                min_height: Val::Px(32.0),
                flex_grow: 1.0,
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
            CombatHudOption { index: i, action },
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

/// Mouse hover focuses an option; a click chooses it (subject to affordability).
fn handle_combat_hud_mouse(
    mut interactions: Query<(&Interaction, &CombatHudOption), Changed<Interaction>>,
    mut state: ResMut<CombatHudState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    stats_q: Query<&CombatStats>,
    status_q: Query<&StatusEffects>,
    mult_q: Query<&MagicCostMultipliers>,
    enemies_q: Query<(Entity, &BattleSide), With<BattleParticipant>>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    let Some(actor) = pending.entity else { return };
    let Some(ctx) = resolve_actor(actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref())
    else {
        return;
    };

    for (interaction, option) in &mut interactions {
        match *interaction {
            Interaction::Hovered | Interaction::Pressed => {
                state.focus = option.index;
            }
            Interaction::None => {}
        }
        if *interaction == Interaction::Pressed {
            choose_option(option.action, &ctx, &mut state, &enemies_q, &mut actions);
        }
    }
}

// ---------------------------------------------------------------------------
// Input — keyboard
// ---------------------------------------------------------------------------

fn handle_combat_hud_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CombatHudState>,
    game_state: Res<GameState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    stats_q: Query<&CombatStats>,
    status_q: Query<&StatusEffects>,
    mult_q: Query<&MagicCostMultipliers>,
    options_q: Query<&CombatHudOption>,
    enemies_q: Query<(Entity, &BattleSide), With<BattleParticipant>>,
    mut actions: MessageWriter<PlayerActionEvent>,
) {
    if game_state.0 != Game_State::Battle {
        return;
    }
    let Some(actor) = pending.entity else { return };

    // Cancel takes priority in either mode.
    if keys.just_pressed(KeyCode::Escape) && state.mode != HudMode::Idle {
        state.mode = HudMode::Idle;
        return;
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
            let count = state.option_count.max(1);
            if keys.just_pressed(KeyCode::ArrowDown) {
                state.focus = (state.focus + 1) % count;
            }
            if keys.just_pressed(KeyCode::ArrowUp) {
                state.focus = (state.focus + count - 1) % count;
            }
            // Number keys 1..=9 jump to an option.
            for (key, idx) in NUMBER_KEYS.iter().copied() {
                if keys.just_pressed(key) && idx < count {
                    state.focus = idx;
                }
            }
            if keys.just_pressed(KeyCode::Enter) {
                let focus = state.focus;
                if let Some(option) = options_q.iter().find(|o| o.index == focus).copied() {
                    if let Some(ctx) =
                        resolve_actor(actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref())
                    {
                        choose_option(option.action, &ctx, &mut state, &enemies_q, &mut actions);
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

#[allow(clippy::too_many_arguments)]
fn sync_combat_hud(
    state: Res<CombatHudState>,
    pending: Res<PendingPlayerAction>,
    ability_tree: Option<Res<Ability_Tree>>,
    stats_q: Query<&CombatStats>,
    status_q: Query<&StatusEffects>,
    mult_q: Query<&MagicCostMultipliers>,
    name_q: Query<&Name>,
    mut options_q: Query<(
        &CombatHudOption,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut text_q: Query<&mut TextColor, With<CombatHudOptionLabel>>,
    mut header_q: Query<&mut Text, With<CombatHudHeader>>,
) {
    let Some(actor) = pending.entity else { return };
    let Some(ctx) = resolve_actor(actor, &stats_q, &status_q, &mult_q, ability_tree.as_deref())
    else {
        return;
    };

    // Per-option appearance.
    for (option, interaction, mut bg, mut border, children) in &mut options_q {
        let usable = ctx.usability(option.action);
        let focused = option.index == state.focus;
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
        let name = name_q.get(actor).map(|n| n.0.as_str()).unwrap_or("Combatant");
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
                .map(|n| n.0.clone())
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
        HudMode::Idle => {
            match options_q.iter().find(|o| o.index == state.focus) {
                Some(option) => describe_action(option.action, &ctx, item_catalog.as_deref()),
                None => "↑/↓ or 1–9 to choose · Enter to act · Space to wait".to_string(),
            }
        }
    };

    if hint.0 != desired {
        hint.0 = desired;
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
