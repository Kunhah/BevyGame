//! Resting: inn / camp / ritual.
//!
//! Resting is an interactive, time-stepped activity. The player opens the rest
//! selector (at an inn, at a shrine for a rite, or anywhere on the map to make
//! camp), chooses a duration in multiples of 4 minutes, and the game advances
//! game-time 4 minutes (30 ticks) at a time toward that target. On each step a
//! [`RestStepEvent`] fires carrying the cumulative time rested, and the
//! random-event engine rolls *at most one* event for that step (highest
//! probability first), based on place, time rested, and global story flags.
//!
//! When the session ends (target reached or interrupted), a single tick-native
//! [`RestEvent`] drives the existing regen pipeline (scaled by a location
//! "quality" multiplier), a [`RestCompletedEvent`] lets other systems react
//! (e.g. the inn's effect on the local town), and rituals fire a
//! [`PerformActivityEvent`] for their magic/kegare restoration.
//!
//! Place-bound rituals (shrine rites) rest *without* random events; camp
//! rituals can be interrupted.

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use rand::Rng;

use crate::activities::{ActivityKind, PerformActivityEvent};
use crate::combat_plugin::{ActionCause, RestEvent, RestRates};
use crate::constants::TIMESTAMP_TICKS_PER_HOUR;
use crate::core::{GameState, Game_State, Player, Timestamp};
use crate::economy::{PlayerWallet, TradeLogEvent};
use crate::money::Money;
use crate::quests::QuestFlags;
use crate::status_effects::{ApplyStatusEvent, DebuffKind, StatusKind};
use crate::story_flags::{FlagChangedEvent, StoryFlags};
use crate::ui_style::{font_size, palette, radius, spacing};

/// One rest step is 4 minutes = 240 s = exactly 30 game ticks.
pub const TICKS_PER_REST_STEP: u32 = 30;
pub const MINUTES_PER_REST_STEP: u32 = 4;
/// Wall-clock seconds each step takes, so stepping is visible/interruptible
/// rather than instant. Tune to taste.
const STEP_REAL_SECONDS: f32 = 0.05;
/// Default and maximum selectable steps (1 h default, 24 h cap).
const DEFAULT_REST_STEPS: u32 = 15;
const MAX_REST_STEPS: u32 = 360;

/// Placeholder lodging rate and offering — currency tuning is deferred.
const INN_MON_PER_HOUR: u32 = 40;
const SHRINE_OFFERING_MON: u32 = 50;

// ---------------------------------------------------------------------------
// Context & restoration scaling
// ---------------------------------------------------------------------------

/// Where/how the party is resting. Drives the regen quality multiplier and
/// whether random events can occur.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestContext {
    Inn,
    Camp,
    Ritual {
        kind: ActivityKind,
        place_bound: bool,
    },
}

impl RestContext {
    pub fn label(self) -> &'static str {
        match self {
            RestContext::Inn => "inn",
            RestContext::Camp => "camp",
            RestContext::Ritual { .. } => "ritual",
        }
    }

    /// Whether random events may interrupt/occur during this rest. Place-bound
    /// rites are undisturbed.
    fn events_enabled(self) -> bool {
        match self {
            RestContext::Inn | RestContext::Camp => true,
            RestContext::Ritual { place_bound, .. } => !place_bound,
        }
    }
}

/// Per-stat regen each location contributes per hour, *added* to each party
/// member's own `*_per_rest_hour` rates (see `rest_regen_system`). Inn beds
/// restore the most body and magic; a roadside camp less; a ritual restores
/// little bodily rest (its real payoff is the rite's own magic/kegare effect).
/// Placeholder values — tune freely.
pub fn location_rest_rates(ctx: RestContext) -> RestRates {
    match ctx {
        RestContext::Inn => RestRates {
            health: 4.0,
            morale: 6.0,
            kiho: 0.5,
            onmyodo: 0.5,
            yokaijutsu: 0.5,
            kamishin: 0.5,
        },
        RestContext::Camp => RestRates {
            health: 2.0,
            morale: 3.0,
            kiho: 0.25,
            onmyodo: 0.25,
            yokaijutsu: 0.25,
            kamishin: 0.25,
        },
        RestContext::Ritual { .. } => RestRates {
            health: 0.5,
            morale: 1.0,
            ..Default::default()
        },
    }
}

/// Cost of a rest of `ticks` in the given context. Camp and camp-rituals are
/// free; inns charge per (rounded-up) hour; shrine rites take a flat offering.
pub fn rest_cost(ctx: RestContext, ticks: u32) -> Money {
    match ctx {
        RestContext::Camp => Money::ZERO,
        RestContext::Inn => {
            let hours = ticks.div_ceil(TIMESTAMP_TICKS_PER_HOUR).max(1);
            Money(hours * INN_MON_PER_HOUR)
        }
        RestContext::Ritual { place_bound, .. } => {
            if place_bound {
                Money(SHRINE_OFFERING_MON)
            } else {
                Money::ZERO
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session state machine
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct RestSession {
    pub active: Option<ActiveRest>,
}

pub struct ActiveRest {
    /// Regen target: `None` applies to the whole party (every `CombatStats`).
    pub target: Option<Entity>,
    /// The actor for ritual activities / status-event targeting (the leader).
    pub performer: Entity,
    pub context: RestContext,
    pub target_ticks: u32,
    pub accumulated_ticks: u32,
    pub step_index: u32,
    pub events_enabled: bool,
    pub interrupted: bool,
    /// Wall-clock accumulator that paces stepping.
    pub step_timer: f32,
    /// Most recent random-event blurb, for the progress overlay.
    pub last_event: Option<String>,
}

/// Fired once per 4-minute step, carrying the cumulative time rested so far.
#[derive(Message, Clone)]
pub struct RestStepEvent {
    pub target: Option<Entity>,
    pub performer: Entity,
    pub context: RestContext,
    pub cumulative_ticks: u32,
    pub cumulative_minutes: u32,
    pub step_index: u32,
}

/// Fired once when a rest session ends (completed or interrupted).
#[derive(Message, Clone)]
pub struct RestCompletedEvent {
    pub context: RestContext,
    pub ticks: u32,
    pub interrupted: bool,
}

// ---------------------------------------------------------------------------
// Random-event engine
// ---------------------------------------------------------------------------

/// Read-only inputs handed to each candidate event's probability/effect fns.
#[derive(Clone, Copy)]
pub struct RestEventContext {
    pub context: RestContext,
    pub cumulative_ticks: u32,
    pub cumulative_minutes: u32,
    pub step_index: u32,
    pub time_of_day_hour: u32,
}

/// What a fired random event does. Side effects are declarative so the table
/// stays plain data and one system applies them with full ECS access.
pub enum RestEventEffect {
    Interrupt { reason: &'static str },
    ApplyStatus { kind: StatusKind, tier: u8 },
    /// Positive grants money, negative takes it (in mon).
    AdjustMoney(i64),
    SetStoryFlag(&'static str),
    StartAmbush { reason: &'static str },
    Log(&'static str),
}

pub struct RandomRestEvent {
    pub id: &'static str,
    /// Likelihood as a *per-hour rate* (roughly "chance this happens in an
    /// hour"). The engine converts it to the 4-minute step probability
    /// (`rate * 4/60`) so authoring stays intuitive and long rests don't make
    /// every event a certainty. Return `0.0` to not be a candidate this step.
    pub rate_per_hour: fn(&RestEventContext, &StoryFlags, &QuestFlags) -> f32,
    pub effect: fn(&RestEventContext) -> RestEventEffect,
}

#[derive(Resource)]
pub struct RestEventTable(pub Vec<RandomRestEvent>);

impl Default for RestEventTable {
    fn default() -> Self {
        // Rates below are PER HOUR (see `rate_per_hour`).
        RestEventTable(vec![
            // Roadside bandits — only at camp, only at night, more likely the
            // longer you linger and if you're a wanted outlaw. Interrupts.
            RandomRestEvent {
                id: "bandit_ambush",
                rate_per_hour: |ctx, flags, _q| {
                    if !matches!(ctx.context, RestContext::Camp) {
                        return 0.0;
                    }
                    let night = ctx.time_of_day_hour >= 20 || ctx.time_of_day_hour < 5;
                    if !night {
                        return 0.0;
                    }
                    let hours = ctx.cumulative_minutes as f32 / 60.0;
                    let mut rate = (0.25 + 0.05 * hours).min(0.8);
                    if flags.is_set("wanted_outlaw") {
                        rate += 0.3;
                    }
                    rate
                },
                effect: |_ctx| RestEventEffect::StartAmbush {
                    reason: "Bandits ambush your camp!",
                },
            },
            // An uneasy night — risk of bad dreams grows with time slept.
            RandomRestEvent {
                id: "ominous_dream",
                rate_per_hour: |ctx, _f, _q| {
                    if !matches!(ctx.context, RestContext::Camp | RestContext::Inn) {
                        return 0.0;
                    }
                    let hours = ctx.cumulative_minutes as f32 / 60.0;
                    (0.1 + 0.05 * hours).min(0.4)
                },
                effect: |_ctx| RestEventEffect::ApplyStatus {
                    kind: StatusKind::Debuff(DebuffKind::HauntedDreams),
                    tier: 1,
                },
            },
            // A light-fingered guest at the inn.
            RandomRestEvent {
                id: "pickpocket",
                rate_per_hour: |ctx, _f, _q| {
                    if matches!(ctx.context, RestContext::Inn) {
                        0.1
                    } else {
                        0.0
                    }
                },
                effect: |_ctx| RestEventEffect::AdjustMoney(-30),
            },
            // Benign control: an undisturbed stretch. Low, always-eligible — its
            // presence proves the roll picks at most one event.
            RandomRestEvent {
                id: "peaceful_rest",
                rate_per_hour: |_ctx, _f, _q| 0.2,
                effect: |_ctx| RestEventEffect::Log("You rest undisturbed."),
            },
        ])
    }
}

// ---------------------------------------------------------------------------
// Camp / rest UI
// ---------------------------------------------------------------------------

/// Camp menu options, in display order.
const CAMP_MENU: [&str; 3] = ["Rest", "Perform a rite", "Break camp"];
/// Placeless rites the player can perform at camp (none require a sacred site).
const CAMP_RITUALS: [ActivityKind; 8] = [
    ActivityKind::Meditation,
    ActivityKind::BreathExercises,
    ActivityKind::Prayer,
    ActivityKind::Forage,
    ActivityKind::CraftTalisman,
    ActivityKind::NightRitual,
    ActivityKind::SpiritOffering,
    ActivityKind::BloodPact,
];

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum RestUiMode {
    #[default]
    Closed,
    /// Camp root menu (rest / rite / leave).
    CampMenu,
    /// Choosing which placeless rite to perform at camp.
    RitualPicker,
    /// Choosing a rest duration for `pending_context`.
    Selector,
}

#[derive(Resource, Default)]
pub struct RestUi {
    pub mode: RestUiMode,
    root: Option<Entity>,
    last_sig: u64,
    menu_index: usize,
    ritual_index: usize,
    steps: u32,
    pending_context: Option<RestContext>,
    /// Selector was reached via the camp menu, so ESC returns there.
    entered_via_camp: bool,
}

impl RestUi {
    pub fn is_open(&self) -> bool {
        self.mode != RestUiMode::Closed
    }

    /// Open the camp root menu (used by the world `Z` hotkey).
    pub fn open_camp_menu(&mut self) {
        self.mode = RestUiMode::CampMenu;
        self.menu_index = 0;
    }

    /// Jump straight to the duration selector for a context (inn / shrine).
    pub fn open_selector(&mut self, context: RestContext) {
        self.mode = RestUiMode::Selector;
        self.pending_context = Some(context);
        self.steps = DEFAULT_REST_STEPS;
        self.entered_via_camp = false;
    }

    fn close(&mut self) {
        self.mode = RestUiMode::Closed;
        self.pending_context = None;
        self.entered_via_camp = false;
    }
}

#[derive(Component)]
struct RestUiRoot;

/// Per-frame-updated progress line during a rest.
#[derive(Component)]
struct RestUiStatus;

/// Progress-bar fill whose width tracks rest completion.
#[derive(Component)]
struct RestUiBar;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct RestPlugin;

impl Plugin for RestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RestSession>()
            .init_resource::<RestUi>()
            .init_resource::<RestEventTable>()
            .add_message::<RestStepEvent>()
            .add_message::<RestCompletedEvent>()
            .add_systems(
                Update,
                (
                    sync_rest_game_state,
                    camp_open_input,
                    rest_ui_input,
                    advance_rest_session,
                    roll_and_apply_rest_events.after(advance_rest_session),
                    rebuild_rest_ui,
                    update_rest_ui_dynamic,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// State sync: any open rest UI / active rest holds the world in `Resting`
// (which gates movement and other exploration input), and releases it after.
// ---------------------------------------------------------------------------

fn sync_rest_game_state(
    ui: Res<RestUi>,
    session: Res<RestSession>,
    mut game_state: ResMut<GameState>,
) {
    let busy = ui.is_open() || session.active.is_some();
    if busy && game_state.0 == Game_State::Exploring {
        game_state.0 = Game_State::Resting;
    } else if !busy && game_state.0 == Game_State::Resting {
        game_state.0 = Game_State::Exploring;
    }
}

// ---------------------------------------------------------------------------
// Input: open the camp menu anywhere on the map
// ---------------------------------------------------------------------------

fn camp_open_input(
    input: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    mut ui: ResMut<RestUi>,
    session: Res<RestSession>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    if game_state.0 != Game_State::Exploring || ui.is_open() || session.active.is_some() {
        return;
    }
    if input.just_pressed(KeyCode::KeyZ) {
        ui.open_camp_menu();
        logs.write(TradeLogEvent {
            message: "made camp (↑/↓ select, ENTER, ESC)".to_string(),
        });
    }
}

// ---------------------------------------------------------------------------
// Input: drive the camp menu / ritual picker / duration selector
// ---------------------------------------------------------------------------

fn rest_ui_input(
    input: Res<ButtonInput<KeyCode>>,
    mut ui: ResMut<RestUi>,
    mut wallet: ResMut<PlayerWallet>,
    mut session: ResMut<RestSession>,
    player_q: Query<Entity, With<Player>>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    let up = input.just_pressed(KeyCode::ArrowUp) || input.just_pressed(KeyCode::KeyW);
    let down = input.just_pressed(KeyCode::ArrowDown) || input.just_pressed(KeyCode::KeyS);
    let confirm = input.just_pressed(KeyCode::Enter) || input.just_pressed(KeyCode::Space);
    let cancel = input.just_pressed(KeyCode::Escape);

    match ui.mode {
        RestUiMode::Closed => {}
        RestUiMode::CampMenu => {
            let n = CAMP_MENU.len();
            if cancel {
                ui.close();
            } else if up {
                ui.menu_index = (ui.menu_index + n - 1) % n;
            } else if down {
                ui.menu_index = (ui.menu_index + 1) % n;
            } else if confirm {
                match ui.menu_index {
                    0 => {
                        ui.open_selector(RestContext::Camp);
                        ui.entered_via_camp = true;
                    }
                    1 => {
                        ui.mode = RestUiMode::RitualPicker;
                        ui.ritual_index = 0;
                    }
                    _ => ui.close(),
                }
            }
        }
        RestUiMode::RitualPicker => {
            let n = CAMP_RITUALS.len();
            if cancel {
                ui.mode = RestUiMode::CampMenu;
            } else if up {
                ui.ritual_index = (ui.ritual_index + n - 1) % n;
            } else if down {
                ui.ritual_index = (ui.ritual_index + 1) % n;
            } else if confirm {
                let kind = CAMP_RITUALS[ui.ritual_index];
                ui.open_selector(RestContext::Ritual {
                    kind,
                    place_bound: false,
                });
                ui.entered_via_camp = true;
            }
        }
        RestUiMode::Selector => {
            let Some(context) = ui.pending_context else {
                ui.close();
                return;
            };
            if cancel {
                if ui.entered_via_camp {
                    ui.mode = RestUiMode::CampMenu;
                } else {
                    ui.close();
                }
                return;
            }
            if up {
                ui.steps = (ui.steps + 1).min(MAX_REST_STEPS);
            }
            if down {
                ui.steps = ui.steps.saturating_sub(1).max(1);
            }
            if confirm {
                let target_ticks = ui.steps.max(1) * TICKS_PER_REST_STEP;
                let cost = rest_cost(context, target_ticks);
                if wallet.coins < cost.0 {
                    logs.write(TradeLogEvent {
                        message: format!(
                            "cannot afford {} rest: need {}, have {}",
                            context.label(),
                            cost,
                            wallet.coins
                        ),
                    });
                    return;
                }
                let Ok(performer) = player_q.single() else {
                    return;
                };
                wallet.coins = wallet.coins.saturating_sub(cost.0);
                session.active = Some(ActiveRest {
                    target: None,
                    performer,
                    context,
                    target_ticks,
                    accumulated_ticks: 0,
                    step_index: 0,
                    events_enabled: context.events_enabled(),
                    interrupted: false,
                    step_timer: 0.0,
                    last_event: None,
                });
                ui.close();
                logs.write(TradeLogEvent {
                    message: format!(
                        "resting at {} for {} (paid {})",
                        context.label(),
                        fmt_duration(target_ticks),
                        cost
                    ),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stepping + finalization
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn advance_rest_session(
    time: Res<Time>,
    mut session: ResMut<RestSession>,
    mut timestamp: ResMut<Timestamp>,
    mut step_w: MessageWriter<RestStepEvent>,
    mut rest_w: MessageWriter<RestEvent>,
    mut completed_w: MessageWriter<RestCompletedEvent>,
    mut activity_w: MessageWriter<PerformActivityEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    let Some(active) = session.active.as_mut() else {
        return;
    };

    // Finalize when the target is reached or an event interrupted the rest.
    if active.interrupted || active.accumulated_ticks >= active.target_ticks {
        let ctx = active.context;
        let ticks = active.accumulated_ticks;
        let performer = active.performer;
        let target = active.target;
        let interrupted = active.interrupted;

        if ticks > 0 {
            rest_w.write(RestEvent {
                target,
                ticks,
                location: location_rest_rates(ctx),
                cause: ActionCause::Player,
            });
        }
        if let RestContext::Ritual { kind, .. } = ctx {
            // The activity system gates on `hours > 0`; round up so even a
            // short rite fires its kegare/magic effect once.
            let hours = ticks.div_ceil(TIMESTAMP_TICKS_PER_HOUR).max(1);
            activity_w.write(PerformActivityEvent {
                performer,
                activity: kind,
                hours,
            });
        }
        completed_w.write(RestCompletedEvent {
            context: ctx,
            ticks,
            interrupted,
        });
        logs.write(TradeLogEvent {
            message: format!(
                "rest ended ({}): {} rested{}",
                ctx.label(),
                fmt_duration(ticks),
                if interrupted { " — interrupted!" } else { "" }
            ),
        });

        session.active = None;
        return;
    }

    // Pace the steps in wall-clock time.
    active.step_timer += time.delta_secs();
    if active.step_timer < STEP_REAL_SECONDS {
        return;
    }
    active.step_timer -= STEP_REAL_SECONDS;

    active.accumulated_ticks += TICKS_PER_REST_STEP;
    active.step_index += 1;
    timestamp.0 = timestamp.0.saturating_add(TICKS_PER_REST_STEP);

    step_w.write(RestStepEvent {
        target: active.target,
        performer: active.performer,
        context: active.context,
        cumulative_ticks: active.accumulated_ticks,
        cumulative_minutes: active.step_index * MINUTES_PER_REST_STEP,
        step_index: active.step_index,
    });
}

// ---------------------------------------------------------------------------
// Random-event roll + apply (one event per step, highest probability first)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn roll_and_apply_rest_events(
    mut reader: MessageReader<RestStepEvent>,
    mut session: ResMut<RestSession>,
    table: Res<RestEventTable>,
    timestamp: Res<Timestamp>,
    mut story_flags: ResMut<StoryFlags>,
    quest_flags: Res<QuestFlags>,
    mut wallet: ResMut<PlayerWallet>,
    mut apply_status_w: MessageWriter<ApplyStatusEvent>,
    mut flag_w: MessageWriter<FlagChangedEvent>,
    mut logs: MessageWriter<TradeLogEvent>,
) {
    let mut rng = rand::rng();
    for ev in reader.read() {
        let Some(active) = session.active.as_mut() else {
            continue;
        };
        if !active.events_enabled {
            continue;
        }

        let ctx = RestEventContext {
            context: ev.context,
            cumulative_ticks: ev.cumulative_ticks,
            cumulative_minutes: ev.cumulative_minutes,
            step_index: ev.step_index,
            time_of_day_hour: (timestamp.0 / TIMESTAMP_TICKS_PER_HOUR) % 24,
        };

        // Convert each candidate's per-hour rate to this 4-minute step's
        // probability, sort highest-first, then roll each in turn and fire the
        // first that hits — "highest probability first", at most one per step.
        let step_frac = MINUTES_PER_REST_STEP as f32 / 60.0;
        let mut candidates: Vec<(usize, f32)> = table
            .0
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let rate = (e.rate_per_hour)(&ctx, &story_flags, &quest_flags);
                (i, (rate * step_frac).clamp(0.0, 1.0))
            })
            .filter(|(_, p)| *p > 0.0)
            .collect();
        candidates
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut chosen: Option<usize> = None;
        for (i, p) in &candidates {
            if rng.random::<f32>() < *p {
                chosen = Some(*i);
                break;
            }
        }
        let Some(idx) = chosen else {
            continue;
        };
        let id = table.0[idx].id;
        let effect = (table.0[idx].effect)(&ctx);

        match effect {
            RestEventEffect::Log(msg) => {
                active.last_event = Some(msg.to_string());
                logs.write(TradeLogEvent {
                    message: format!("rest event [{id}]: {msg}"),
                });
            }
            RestEventEffect::Interrupt { reason } => {
                active.interrupted = true;
                active.last_event = Some(reason.to_string());
                logs.write(TradeLogEvent {
                    message: format!("rest interrupted [{id}]: {reason}"),
                });
            }
            RestEventEffect::StartAmbush { reason } => {
                // TODO: hand off to battle setup. For now interrupt + log so the
                // pipeline is exercised end-to-end.
                active.interrupted = true;
                active.last_event = Some(reason.to_string());
                logs.write(TradeLogEvent {
                    message: format!("rest interrupted [{id}]: {reason} (ambush)"),
                });
            }
            RestEventEffect::ApplyStatus { kind, tier } => {
                apply_status_w.write(ApplyStatusEvent {
                    target: active.performer,
                    kind,
                    tier,
                    source: None,
                    expiry_override: None,
                    resource_focus: None,
                });
                active.last_event = Some(format!("{kind:?}"));
                logs.write(TradeLogEvent {
                    message: format!("rest event [{id}]: gained {kind:?} t{tier}"),
                });
            }
            RestEventEffect::AdjustMoney(delta) => {
                if delta < 0 {
                    wallet.coins = wallet.coins.saturating_sub((-delta) as u32);
                } else {
                    wallet.coins = wallet.coins.saturating_add(delta as u32);
                }
                active.last_event = Some(format!("money {delta:+}"));
                logs.write(TradeLogEvent {
                    message: format!("rest event [{id}]: money {delta:+} mon"),
                });
            }
            RestEventEffect::SetStoryFlag(name) => {
                if story_flags.set(name) {
                    flag_w.write(FlagChangedEvent {
                        name: name.to_string(),
                        set: true,
                    });
                }
                active.last_event = Some(format!("flag {name}"));
                logs.write(TradeLogEvent {
                    message: format!("rest event [{id}]: set flag {name}"),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// UI rendering
// ---------------------------------------------------------------------------

/// A signature of everything that changes the panel *layout* (not the
/// continuously-updating progress, which is handled by markers). The panel is
/// only rebuilt when this changes.
fn rest_ui_sig(ui: &RestUi, session: &RestSession) -> u64 {
    let mode = match ui.mode {
        RestUiMode::Closed => 0u64,
        RestUiMode::CampMenu => 1,
        RestUiMode::RitualPicker => 2,
        RestUiMode::Selector => 3,
    };
    mode | ((ui.menu_index as u64) << 4)
        | ((ui.ritual_index as u64) << 12)
        | ((ui.steps as u64) << 20)
        | ((session.active.is_some() as u64) << 40)
}

/// (Re)build the panel when its layout signature changes; despawn it when no
/// rest UI is showing.
fn rebuild_rest_ui(
    mut commands: Commands,
    mut ui: ResMut<RestUi>,
    session: Res<RestSession>,
    wallet: Res<PlayerWallet>,
) {
    let busy = ui.is_open() || session.active.is_some();
    if !busy {
        if let Some(root) = ui.root.take() {
            commands.entity(root).despawn();
        }
        ui.last_sig = 0;
        return;
    }
    let sig = rest_ui_sig(&ui, &session);
    if ui.root.is_some() && sig == ui.last_sig {
        return;
    }
    if let Some(root) = ui.root.take() {
        commands.entity(root).despawn();
    }

    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(32.0),
                right: Val::Percent(32.0),
                top: Val::Percent(28.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(spacing::SM),
                padding: UiRect::all(Val::Px(spacing::LG)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Px(radius::LG)),
                ..default()
            },
            BackgroundColor(palette::BG_PANEL),
            BorderColor::all(palette::BORDER_ACCENT),
            RestUiRoot,
        ))
        .with_children(|panel| {
            if let Some(active) = &session.active {
                build_resting_view(panel, active);
            } else {
                match ui.mode {
                    RestUiMode::CampMenu => build_camp_menu(panel, ui.menu_index),
                    RestUiMode::RitualPicker => build_ritual_picker(panel, ui.ritual_index),
                    RestUiMode::Selector => {
                        if let Some(ctx) = ui.pending_context {
                            build_selector(panel, ctx, ui.steps, &wallet);
                        }
                    }
                    RestUiMode::Closed => {}
                }
            }
        })
        .id();

    ui.root = Some(root);
    ui.last_sig = sig;
}

fn title(panel: &mut ChildSpawnerCommands, text: impl Into<String>) {
    panel.spawn((
        Text::new(text.into()),
        TextFont {
            font_size: font_size::SUBHEADING,
            ..default()
        },
        TextColor(palette::TEXT_HEADING),
    ));
}

fn row(panel: &mut ChildSpawnerCommands, text: impl Into<String>, selected: bool) {
    let (prefix, color) = if selected {
        ("▶ ", palette::ACCENT_PRIMARY)
    } else {
        ("   ", palette::TEXT_SECONDARY)
    };
    panel.spawn((
        Text::new(format!("{prefix}{}", text.into())),
        TextFont {
            font_size: font_size::BODY,
            ..default()
        },
        TextColor(color),
    ));
}

fn hint(panel: &mut ChildSpawnerCommands, text: impl Into<String>) {
    panel.spawn((
        Text::new(text.into()),
        TextFont {
            font_size: font_size::SMALL,
            ..default()
        },
        TextColor(palette::TEXT_DIM),
    ));
}

fn build_camp_menu(panel: &mut ChildSpawnerCommands, selected: usize) {
    title(panel, "Camp");
    for (i, label) in CAMP_MENU.iter().enumerate() {
        row(panel, *label, i == selected);
    }
    hint(panel, "↑/↓ select · ENTER · ESC");
}

fn build_ritual_picker(panel: &mut ChildSpawnerCommands, selected: usize) {
    title(panel, "Perform a rite");
    for (i, kind) in CAMP_RITUALS.iter().enumerate() {
        row(panel, format!("{kind:?}"), i == selected);
    }
    hint(panel, "↑/↓ select · ENTER · ESC back");
}

fn build_selector(
    panel: &mut ChildSpawnerCommands,
    ctx: RestContext,
    steps: u32,
    wallet: &PlayerWallet,
) {
    let ticks = steps.max(1) * TICKS_PER_REST_STEP;
    let cost = rest_cost(ctx, ticks);
    title(panel, format!("Rest — {}", ctx.label()));
    // Big duration readout.
    panel.spawn((
        Text::new(fmt_duration(ticks)),
        TextFont {
            font_size: font_size::HEADING,
            ..default()
        },
        TextColor(palette::ACCENT_PRIMARY),
    ));
    panel.spawn((
        Text::new(format!("cost {}   ·   you have {}", cost, wallet.coins)),
        TextFont {
            font_size: font_size::BODY,
            ..default()
        },
        TextColor(palette::TEXT_SECONDARY),
    ));
    if !ctx.events_enabled() {
        hint(panel, "a sacred rite — undisturbed by chance");
    }
    hint(panel, "↑/↓ ±4 min · ENTER rest · ESC");
}

fn build_resting_view(panel: &mut ChildSpawnerCommands, active: &ActiveRest) {
    title(panel, format!("Resting — {}", active.context.label()));
    // Progress-bar track + fill.
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(14.0),
                border_radius: BorderRadius::all(Val::Px(radius::SM)),
                ..default()
            },
            BackgroundColor(palette::BG_PANEL_SUNK),
        ))
        .with_children(|track| {
            track.spawn((
                Node {
                    width: Val::Percent(0.0),
                    height: Val::Percent(100.0),
                    border_radius: BorderRadius::all(Val::Px(radius::SM)),
                    ..default()
                },
                BackgroundColor(palette::ACCENT_SUCCESS),
                RestUiBar,
            ));
        });
    // Live status line (updated each frame).
    panel.spawn((
        Text::new(String::new()),
        TextFont {
            font_size: font_size::BODY,
            ..default()
        },
        TextColor(palette::TEXT_PRIMARY),
        RestUiStatus,
    ));
}

/// Update the continuously-changing bits of the resting view (progress bar +
/// status line) without rebuilding the panel each step.
fn update_rest_ui_dynamic(
    session: Res<RestSession>,
    mut status_q: Query<&mut Text, With<RestUiStatus>>,
    mut bar_q: Query<&mut Node, With<RestUiBar>>,
) {
    let Some(active) = &session.active else {
        return;
    };
    let frac = if active.target_ticks > 0 {
        (active.accumulated_ticks as f32 / active.target_ticks as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    for mut node in &mut bar_q {
        node.width = Val::Percent(frac * 100.0);
    }
    for mut text in &mut status_q {
        let mut s = format!(
            "{} / {}",
            fmt_duration(active.accumulated_ticks),
            fmt_duration(active.target_ticks)
        );
        if let Some(ev) = &active.last_event {
            s.push_str(&format!("\n· {ev}"));
        }
        text.0 = s;
    }
}

/// Format a tick duration as `H:MM` (or `MMm` under an hour).
fn fmt_duration(ticks: u32) -> String {
    let total_min = ticks * MINUTES_PER_REST_STEP / TICKS_PER_REST_STEP;
    let h = total_min / 60;
    let m = total_min % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_is_four_minutes() {
        // 30 ticks * 8 s/tick = 240 s = 4 min.
        assert_eq!(TICKS_PER_REST_STEP * 8, MINUTES_PER_REST_STEP * 60);
    }

    #[test]
    fn duration_formats() {
        assert_eq!(fmt_duration(TICKS_PER_REST_STEP), "4m");
        assert_eq!(fmt_duration(TIMESTAMP_TICKS_PER_HOUR), "1h00m");
        assert_eq!(fmt_duration(8 * TIMESTAMP_TICKS_PER_HOUR), "8h00m");
    }

    #[test]
    fn inn_cost_scales_camp_is_free() {
        assert_eq!(rest_cost(RestContext::Camp, 8 * TIMESTAMP_TICKS_PER_HOUR), Money::ZERO);
        // 8 h at the inn at the placeholder per-hour rate.
        assert_eq!(
            rest_cost(RestContext::Inn, 8 * TIMESTAMP_TICKS_PER_HOUR),
            Money(8 * INN_MON_PER_HOUR)
        );
        // Sub-hour inn stay rounds up to one hour.
        assert_eq!(
            rest_cost(RestContext::Inn, TICKS_PER_REST_STEP),
            Money(INN_MON_PER_HOUR)
        );
    }

    #[test]
    fn place_bound_rituals_have_no_events() {
        let shrine = RestContext::Ritual {
            kind: ActivityKind::Harae,
            place_bound: true,
        };
        let camp_rite = RestContext::Ritual {
            kind: ActivityKind::Meditation,
            place_bound: false,
        };
        assert!(!shrine.events_enabled());
        assert!(camp_rite.events_enabled());
        assert!(RestContext::Camp.events_enabled());
    }
}
