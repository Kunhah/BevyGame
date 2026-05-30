use std::collections::VecDeque;

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::activities::{ActivityKind, PerformActivityEvent};
use crate::combat_ability::MagicSchool;
use crate::combat_plugin::{
    AccessoryType, ArmorType, CharacterId, CombatStats, Equipment, EquipmentLoadout,
    EquipmentType, Experience, Inventory, ItemMaterial, ItemMaterialCost, Level,
    Name as CombatName, StatPool, WeaponType,
};
use crate::contract::{ConfessAtShrineEvent, DrinkTeaWithBoundEvent};
use crate::core::Player;
use crate::ui_style::{font_size, palette, radius, spacing};

pub struct DebugConsolePlugin;

impl Plugin for DebugConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugConsoleState>()
            .init_resource::<DebugEntityCounter>()
            .add_systems(Startup, setup_console_ui)
            .add_systems(Update, toggle_console)
            .add_systems(Update, handle_console_input)
            .add_systems(Update, update_console_ui);
    }
}

#[derive(Resource)]
struct DebugConsoleState {
    open: bool,
    input: String,
    history: VecDeque<String>,
    max_lines: usize,
}

impl Default for DebugConsoleState {
    fn default() -> Self {
        Self {
            open: false,
            input: String::new(),
            history: VecDeque::new(),
            max_lines: 24,
        }
    }
}

#[derive(Resource, Default)]
struct DebugEntityCounter(u32);

#[derive(Component)]
struct DebugConsoleRoot;

#[derive(Component)]
struct DebugConsoleOutput;

#[derive(Component)]
struct DebugConsoleInput;

fn setup_console_ui(mut commands: Commands) {
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(60.0),
                height: Val::Percent(35.0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(spacing::SM),
                padding: UiRect::all(Val::Px(spacing::MD)),
                border: UiRect::all(Val::Px(1.0)),
                position_type: PositionType::Absolute,
                left: Val::Px(spacing::MD),
                bottom: Val::Px(spacing::MD),
                border_radius: BorderRadius::all(Val::Px(radius::MD)),
                ..default()
            },
            BackgroundColor(palette::BG_PANEL_SUNK),
            BorderColor::all(palette::BORDER_SUBTLE),
            DebugConsoleRoot,
            Visibility::Hidden,
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        parent.spawn((
            Text::new(""),
            TextFont {
                font_size: font_size::LABEL,
                ..default()
            },
            TextColor(palette::TEXT_SECONDARY),
            DebugConsoleOutput,
        ));

        parent.spawn((
            Text::new("> "),
            TextFont {
                font_size: font_size::BODY,
                ..default()
            },
            TextColor(palette::TEXT_HEADING),
            DebugConsoleInput,
        ));
    });
}

fn toggle_console(
    input: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<DebugConsoleState>,
) {
    if input.just_pressed(KeyCode::Backquote) {
        state.open = !state.open;
    }
}

/// Message writers the console needs, bundled into one `SystemParam` so
/// `handle_console_input` stays under Bevy's per-system param limit.
#[derive(bevy::ecs::system::SystemParam)]
struct ConsoleWriters<'w> {
    activity: MessageWriter<'w, PerformActivityEvent>,
    confess: MessageWriter<'w, ConfessAtShrineEvent>,
    tea: MessageWriter<'w, DrinkTeaWithBoundEvent>,
    purify: MessageWriter<'w, crate::kegare::PurifyEvent>,
}

fn handle_console_input(
    mut commands: Commands,
    mut state: ResMut<DebugConsoleState>,
    key_input: Res<ButtonInput<KeyCode>>,
    mut counter: ResMut<DebugEntityCounter>,
    player_q: Query<Entity, With<Player>>,
    name_q: Query<(Entity, &CombatName)>,
    id_q: Query<(Entity, &CharacterId)>,
    mut transforms: Query<&mut Transform>,
    mut stats_q: Query<&mut CombatStats>,
    mut xp_q: Query<&mut Experience>,
    mut level_q: Query<&mut Level>,
    mut inventory_q: Query<&mut Inventory>,
    mut loadout_q: Query<&mut EquipmentLoadout>,
    mut writers: ConsoleWriters,
) {
    if !state.open {
        return;
    }

    if key_input.just_pressed(KeyCode::Escape) {
        state.open = false;
        return;
    }

    for key in key_input.get_just_pressed() {
        if let Some(ch) = keycode_to_char(*key) {
            state.input.push(ch);
        }
    }

    if key_input.just_pressed(KeyCode::Backspace) {
        state.input.pop();
    }

    if key_input.just_pressed(KeyCode::Enter) {
        let line = state.input.trim().to_string();
        state.input.clear();
        if !line.is_empty() {
            let clear_requested = line.eq_ignore_ascii_case("clear");
            if clear_requested {
                state.history.clear();
            } else {
                push_history(&mut state, format!("> {}", line));
            }
            let outputs = execute_command(
                &line,
                &mut commands,
                &mut counter,
                &player_q,
                &name_q,
                &id_q,
                &mut transforms,
                &mut stats_q,
                &mut xp_q,
                &mut level_q,
                &mut inventory_q,
                &mut loadout_q,
                &mut writers,
            );
            for out in outputs {
                push_history(&mut state, out);
            }
        }
    }
}

fn update_console_ui(
    state: Res<DebugConsoleState>,
    mut root: Query<&mut Visibility, With<DebugConsoleRoot>>,
    mut text_q: Query<(
        &mut Text,
        Option<&DebugConsoleOutput>,
        Option<&DebugConsoleInput>,
    )>,
) {
    if !state.is_changed() {
        return;
    }

    if let Ok(mut vis) = root.single_mut() {
        *vis = if state.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    for (mut text, is_output, is_input) in &mut text_q {
        if is_output.is_some() {
            text.0 = state.history.iter().cloned().collect::<Vec<_>>().join("\n");
        } else if is_input.is_some() {
            text.0 = format!("> {}", state.input);
        }
    }
}

fn keycode_to_char(key: KeyCode) -> Option<char> {
    match key {
        KeyCode::KeyA => Some('a'),
        KeyCode::KeyB => Some('b'),
        KeyCode::KeyC => Some('c'),
        KeyCode::KeyD => Some('d'),
        KeyCode::KeyE => Some('e'),
        KeyCode::KeyF => Some('f'),
        KeyCode::KeyG => Some('g'),
        KeyCode::KeyH => Some('h'),
        KeyCode::KeyI => Some('i'),
        KeyCode::KeyJ => Some('j'),
        KeyCode::KeyK => Some('k'),
        KeyCode::KeyL => Some('l'),
        KeyCode::KeyM => Some('m'),
        KeyCode::KeyN => Some('n'),
        KeyCode::KeyO => Some('o'),
        KeyCode::KeyP => Some('p'),
        KeyCode::KeyQ => Some('q'),
        KeyCode::KeyR => Some('r'),
        KeyCode::KeyS => Some('s'),
        KeyCode::KeyT => Some('t'),
        KeyCode::KeyU => Some('u'),
        KeyCode::KeyV => Some('v'),
        KeyCode::KeyW => Some('w'),
        KeyCode::KeyX => Some('x'),
        KeyCode::KeyY => Some('y'),
        KeyCode::KeyZ => Some('z'),
        KeyCode::Digit0 => Some('0'),
        KeyCode::Digit1 => Some('1'),
        KeyCode::Digit2 => Some('2'),
        KeyCode::Digit3 => Some('3'),
        KeyCode::Digit4 => Some('4'),
        KeyCode::Digit5 => Some('5'),
        KeyCode::Digit6 => Some('6'),
        KeyCode::Digit7 => Some('7'),
        KeyCode::Digit8 => Some('8'),
        KeyCode::Digit9 => Some('9'),
        KeyCode::Space => Some(' '),
        KeyCode::Comma => Some(','),
        KeyCode::Period => Some('.'),
        KeyCode::Slash => Some('/'),
        KeyCode::Minus => Some('-'),
        KeyCode::Equal => Some('='),
        _ => None,
    }
}

fn push_history(state: &mut DebugConsoleState, line: String) {
    state.history.push_back(line);
    while state.history.len() > state.max_lines {
        state.history.pop_front();
    }
}

fn execute_command(
    line: &str,
    commands: &mut Commands,
    counter: &mut DebugEntityCounter,
    player_q: &Query<Entity, With<Player>>,
    name_q: &Query<(Entity, &CombatName)>,
    id_q: &Query<(Entity, &CharacterId)>,
    transforms: &mut Query<&mut Transform>,
    stats_q: &mut Query<&mut CombatStats>,
    xp_q: &mut Query<&mut Experience>,
    level_q: &mut Query<&mut Level>,
    inventory_q: &mut Query<&mut Inventory>,
    loadout_q: &mut Query<&mut EquipmentLoadout>,
    writers: &mut ConsoleWriters,
) -> Vec<String> {
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else {
        return vec![];
    };

    match cmd {
        "help" => vec![help_text()],
        "clear" => vec!["(console cleared)".to_string()],
        "status" => {
            let target = parts.next();
            match resolve_target(target, player_q, name_q, id_q) {
                Ok(entity) => vec![format_status(
                    entity, name_q, stats_q, xp_q, level_q,
                )],
                Err(err) => vec![err],
            }
        }
        "teleport" | "tp" => {
            let x = parts.next();
            let y = parts.next();
            let target = parts.next();
            match (x, y) {
                (Some(x), Some(y)) => match (x.parse::<i32>(), y.parse::<i32>()) {
                    (Ok(x), Ok(y)) => match resolve_target(target, player_q, name_q, id_q) {
                        Ok(entity) => {
                            if let Ok(mut tf) = transforms.get_mut(entity) {
                                tf.translation.x = x as f32;
                                tf.translation.y = y as f32;
                            }
                            vec![format!("teleport: moved entity {:?} to {}, {}", entity, x, y)]
                        }
                        Err(err) => vec![err],
                    },
                    _ => vec!["teleport: x/y must be integers".to_string()],
                },
                _ => vec!["teleport: usage `teleport <x> <y> [target]`".to_string()],
            }
        }
        "set_stat" | "set" => {
            let stat = parts.next();
            let value = parts.next();
            let target = parts.next();
            match (stat, value) {
                (Some(stat), Some(value)) => match value.parse::<i32>() {
                    Ok(value) => match resolve_target(target, player_q, name_q, id_q) {
                        Ok(entity) => {
                            let result = apply_stat(
                                StatOp::Set(value),
                                stat,
                                entity,
                                commands,
                                stats_q,
                                xp_q,
                                level_q,
                            );
                            vec![result]
                        }
                        Err(err) => vec![err],
                    },
                    Err(_) => vec!["set_stat: value must be an integer".to_string()],
                },
                _ => vec!["set_stat: usage `set_stat <stat> <value> [target]`".to_string()],
            }
        }
        "add_stat" | "add" => {
            let stat = parts.next();
            let value = parts.next();
            let target = parts.next();
            match (stat, value) {
                (Some(stat), Some(value)) => match value.parse::<i32>() {
                    Ok(value) => match resolve_target(target, player_q, name_q, id_q) {
                        Ok(entity) => {
                            let result = apply_stat(
                                StatOp::Add(value),
                                stat,
                                entity,
                                commands,
                                stats_q,
                                xp_q,
                                level_q,
                            );
                            vec![result]
                        }
                        Err(err) => vec![err],
                    },
                    Err(_) => vec!["add_stat: value must be an integer".to_string()],
                },
                _ => vec!["add_stat: usage `add_stat <stat> <value> [target]`".to_string()],
            }
        }
        "kegare" => {
            // Opt a character into the kegare system and set their defilement
            // live. Value is a percentage (0-100) of the hidden accumulator;
            // `clear` removes the components (opts the character back out, so
            // no gating applies). The derived state is reported along with which
            // schools it gates, so you can watch Kamishin lock as you climb.
            use crate::combat_ability::MagicSchool;
            use crate::kegare::{Defilement, Kegare};

            let arg = parts.next();
            let target = parts.next();
            match arg {
                None => vec!["kegare: usage `kegare <0-100|clear> [target]`".to_string()],
                Some(a) if a.eq_ignore_ascii_case("clear") => {
                    match resolve_target(target, player_q, name_q, id_q) {
                        Ok(entity) => {
                            commands
                                .entity(entity)
                                .remove::<Kegare>()
                                .remove::<Defilement>();
                            vec![format!(
                                "kegare: removed from {:?} — opted out, no gating applies",
                                entity
                            )]
                        }
                        Err(err) => vec![err],
                    }
                }
                Some(a) => match a.parse::<i32>() {
                    Ok(pct) => match resolve_target(target, player_q, name_q, id_q) {
                        Ok(entity) => {
                            let level = (pct.clamp(0, 100) as f32) / 100.0;
                            // Derive from Clean so a manual set is deterministic
                            // ("X% -> state Y"); this is idempotent with
                            // derive_defilement_system, which holds the same
                            // state from the next frame on.
                            let state = Defilement::derive(level, Defilement::Clean);
                            commands
                                .entity(entity)
                                .insert((Kegare::new(level), state));
                            vec![format!(
                                "kegare: {:?} set to {}% -> {:?}  | Kamishin regen x{:.2} cost x{:.2}  | Yokaijutsu regen x{:.2} cost x{:.2}",
                                entity,
                                (level * 100.0) as i32,
                                state,
                                crate::kegare::regen_multiplier(state, MagicSchool::Kamishin),
                                crate::kegare::cost_multiplier(state, MagicSchool::Kamishin),
                                crate::kegare::regen_multiplier(state, MagicSchool::Yokaijutsu),
                                crate::kegare::cost_multiplier(state, MagicSchool::Yokaijutsu),
                            )]
                        }
                        Err(err) => vec![err],
                    },
                    Err(_) => {
                        vec!["kegare: value must be an integer 0-100, or `clear`".to_string()]
                    }
                },
            }
        }
        "purify" => {
            // Run a rite of purification, reducing the target's kegare. Drives
            // the real `PurifyEvent` -> `purify_system` path (no-op unless the
            // target is in the kegare system).
            use crate::kegare::{Purification, PurifyEvent};

            let method_arg = parts.next();
            let target = parts.next();
            let method = match method_arg.map(|s| s.to_ascii_lowercase()) {
                Some(ref s) if s == "misogi" || s == "salt" => Some(Purification::Misogi),
                Some(ref s) if s == "harae" || s == "rite" || s == "ritual" => {
                    Some(Purification::Harae)
                }
                Some(_) => None,
                None => Some(Purification::Misogi),
            };
            let Some(method) = method else {
                return vec![
                    "purify: usage `purify <misogi|harae> [target]`".to_string(),
                ];
            };
            match resolve_target(target, player_q, name_q, id_q) {
                Ok(entity) => {
                    writers.purify.write(PurifyEvent {
                        target: entity,
                        method,
                    });
                    vec![format!(
                        "purify: {:?} on {:?} (-{:.0}% kegare; state updates next frame, `status`/`kegare` to read)",
                        method,
                        entity,
                        method.potency() * 100.0,
                    )]
                }
                Err(err) => vec![err],
            }
        }
        "give_item" | "give" => {
            let item_id = parts.next();
            let maybe_slot = parts.next();
            let maybe_target = parts.next();
            match item_id.and_then(|id| id.parse::<u16>().ok()) {
                Some(item_id) => {
                    let (slot, target) = match maybe_slot {
                        Some(slot) if is_slot_name(slot) => (Some(slot), maybe_target),
                        Some(other) => (None, Some(other)),
                        None => (None, None),
                    };
                    match resolve_target(target, player_q, name_q, id_q) {
                        Ok(entity) => {
                            let equip_entity = commands
                                .spawn(Equipment {
                                    id: item_id,
                                    name: format!("DebugItem{}", item_id),
                                    equipment_type: equipment_type_from_name(
                                        slot.unwrap_or("weapon"),
                                    ),
                                    base_price: 1000,
                                    materials: vec![ItemMaterialCost {
                                        material: ItemMaterial::IronIngot,
                                        quantity: 1,
                                    }],
                                    lethality: 0,
                                    hit: 0,
                                    armor: 0,
                                    agility: 0,
                                    mind: 0,
                                    morale: 0,
                                })
                                .id();
                            give_item_to_character(
                                item_id,
                                slot,
                                entity,
                                equip_entity,
                                commands,
                                inventory_q,
                                loadout_q,
                            )
                        }
                        Err(err) => vec![err],
                    }
                }
                None => vec!["give_item: usage `give_item <item_id> [slot] [target]`".to_string()],
            }
        }
        "spawn_entity" | "spawn" => {
            let (name, x, y) = parse_spawn_args(parts.collect::<Vec<_>>());
            match (x, y) {
                (Some(x), Some(y)) => {
                    let entity = spawn_debug_entity(
                        commands,
                        counter,
                        name.unwrap_or_else(|| "DebugEntity".to_string()),
                        x,
                        y,
                    );
                    vec![format!("spawn: spawned {:?} at {}, {}", entity, x, y)]
                }
                _ => vec!["spawn: usage `spawn_entity [name] <x> <y>`".to_string()],
            }
        }
        "activity" => {
            let activity_name = parts.next();
            let hours_str = parts.next();
            let target = parts.next();
            let Some(activity_name) = activity_name else {
                return vec!["activity: usage `activity <kind> [hours] [target]`".to_string()];
            };
            let Some(activity) = parse_activity_kind(activity_name) else {
                return vec![format!(
                    "activity: unknown kind `{}` (see `help` for the list)",
                    activity_name
                )];
            };
            let hours = match hours_str {
                Some(s) => match s.parse::<u32>() {
                    Ok(h) => h,
                    Err(_) => return vec!["activity: hours must be a non-negative integer".to_string()],
                },
                None => 1,
            };
            match resolve_target(target, player_q, name_q, id_q) {
                Ok(entity) => {
                    writers.activity.write(PerformActivityEvent {
                        performer: entity,
                        activity,
                        hours,
                    });
                    vec![format!(
                        "activity: queued {:?} for {:?} ({} h, school {:?})",
                        activity,
                        entity,
                        hours,
                        activity.school()
                    )]
                }
                Err(err) => vec![err],
            }
        }
        "confess" => {
            let target = parts.next();
            match resolve_target(target, player_q, name_q, id_q) {
                Ok(entity) => {
                    writers.confess.write(ConfessAtShrineEvent { who: entity });
                    vec![format!("confess: shrine confession fired for {:?}", entity)]
                }
                Err(err) => vec![err],
            }
        }
        "tea" | "drink_tea" => {
            // Two targets: who is drinking, and the bound they share with.
            // The first arg (the bound) is required; the second (the drinker)
            // defaults to the player.
            let with = parts.next();
            let drinker = parts.next();
            let Some(with) = with else {
                return vec!["tea: usage `tea <with_target> [drinker]`".to_string()];
            };
            let with_entity = match resolve_target(Some(with), player_q, name_q, id_q) {
                Ok(e) => e,
                Err(err) => return vec![err],
            };
            let drinker_entity = match resolve_target(drinker, player_q, name_q, id_q) {
                Ok(e) => e,
                Err(err) => return vec![err],
            };
            writers.tea.write(DrinkTeaWithBoundEvent {
                who: drinker_entity,
                with_bound: with_entity,
            });
            vec![format!(
                "tea: drank with {:?} (drinker={:?})",
                with_entity, drinker_entity
            )]
        }
        _ => vec![format!(
            "unknown command `{}` (try `help`)",
            cmd
        )],
    }
}

/// Maps a CLI keyword to an `ActivityKind`. Accepts both short and long
/// names (e.g. both `meditation` and `meditate`).
fn parse_activity_kind(name: &str) -> Option<ActivityKind> {
    let normalized = name.to_ascii_lowercase();
    Some(match normalized.as_str() {
        // Kiho
        "meditation" | "meditate" => ActivityKind::Meditation,
        "breath" | "breath_exercises" => ActivityKind::BreathExercises,
        "kata" | "kata_practice" => ActivityKind::KataPractice,
        "sparring" | "sparring_drills" => ActivityKind::SparringDrills,
        "shrine_focus" => ActivityKind::ShrineFocus,
        // Onmyodo
        "nature" | "nature_spirit" => ActivityKind::NatureSpiritInteraction,
        "grove" | "tend_grove" => ActivityKind::TendSacredGrove,
        "forage" => ActivityKind::Forage,
        "talisman" | "craft_talisman" => ActivityKind::CraftTalisman,
        "fertile" | "rest_fertile" => ActivityKind::RestFertileTerrain,
        // Yokaijutsu
        "night_ritual" | "ritual" => ActivityKind::NightRitual,
        "spirit_offering" => ActivityKind::SpiritOffering,
        "blood_pact" | "blood" => ActivityKind::BloodPact,
        "binding" | "binding_circle" => ActivityKind::BindingCircle,
        "haunted" | "haunted_location" => ActivityKind::HauntedLocation,
        // Kamishin
        "prayer" | "pray" => ActivityKind::Prayer,
        "shrine_offering" => ActivityKind::ShrineOffering,
        "rite" | "formal_rite" => ActivityKind::FormalRite,
        "pilgrimage" => ActivityKind::Pilgrimage,
        "blessing" | "temple_blessing" => ActivityKind::TempleBlessing,
        // Purification (strips kegare; "rite"/"ritual" are already taken above)
        "misogi" | "salt" | "ablution" => ActivityKind::Misogi,
        "harae" | "harai" => ActivityKind::Harae,
        _ => return None,
    })
}

fn help_text() -> String {
    [
        "Commands:",
        "  help",
        "  clear",
        "  status [target]",
        "  teleport|tp <x> <y> [target]",
        "  set_stat|set <stat> <value> [target]",
        "  add_stat|add <stat> <value> [target]",
        "  kegare <0-100|clear> [target]   (defilement %; tilts Kamishin/Yokaijutsu regen & cost)",
        "  purify <misogi|harae> [target]  (misogi -25% / harae -60% kegare)",
        "  give_item|give <item_id> [slot] [target]",
        "  spawn_entity|spawn [name] <x> <y>",
        "  activity <kind> [hours] [target]",
        "  confess [target]",
        "  tea|drink_tea <with_target> [drinker]",
        "",
        "Targets: `player`, `name:<Name>`, `id:<Number>` (default is player).",
        "Stats: hp, hpmax, morale, moralemax, ap, apmax, movement, movementmax, kiho, kihomax, kihoregen, onmyodo, chiseimax, chiseiregen, yokaijutsu, yokaimax, yokairegen, kamishin, kamishinmax, kamishinregen, lethality, lethalitymax, hit, hitmax, armor, armormax, speed, speedmax, evasion, evasionmax, mind, mindmax, xp, level.",
        "Slots: weapon, armor, accessory.",
        "Activity kinds:",
        "  Kiho: meditation, breath, kata, sparring, shrine_focus",
        "  Onmyodo: nature, grove, forage, talisman, fertile",
        "  Yokaijutsu: night_ritual, spirit_offering, blood_pact, binding, haunted",
        "  Kamishin: prayer, shrine_offering, rite, pilgrimage, blessing",
        "  Purification: misogi, harae (strip kegare instead of restoring magic)",
    ]
    .join("\n")
}

fn resolve_target(
    target: Option<&str>,
    player_q: &Query<Entity, With<Player>>,
    name_q: &Query<(Entity, &CombatName)>,
    id_q: &Query<(Entity, &CharacterId)>,
) -> Result<Entity, String> {
    if let Some(target) = target {
        if target.eq_ignore_ascii_case("player") {
            return player_q
                .iter()
                .next()
                .ok_or_else(|| "target player not found".to_string());
        }

        if let Some(name) = target.strip_prefix("name:") {
            return name_q
                .iter()
                .find(|(_, n)| n.0.eq_ignore_ascii_case(name))
                .map(|(e, _)| e)
                .ok_or_else(|| format!("target name `{}` not found", name));
        }

        if let Some(id) = target.strip_prefix("id:") {
            let id = id
                .parse::<u32>()
                .map_err(|_| "target id must be a number".to_string())?;
            return id_q
                .iter()
                .find(|(_, c)| c.0 == id)
                .map(|(e, _)| e)
                .ok_or_else(|| format!("target id `{}` not found", id));
        }

        return name_q
            .iter()
            .find(|(_, n)| n.0.eq_ignore_ascii_case(target))
            .map(|(e, _)| e)
            .ok_or_else(|| format!("target `{}` not found", target));
    }

    player_q
        .iter()
        .next()
        .ok_or_else(|| "target player not found".to_string())
}

fn parse_spawn_args(args: Vec<&str>) -> (Option<String>, Option<i32>, Option<i32>) {
    match args.len() {
        2 => (None, args[0].parse().ok(), args[1].parse().ok()),
        3 => (
            Some(args[0].to_string()),
            args[1].parse().ok(),
            args[2].parse().ok(),
        ),
        _ => (None, None, None),
    }
}

fn spawn_debug_entity(
    commands: &mut Commands,
    counter: &mut DebugEntityCounter,
    name: String,
    x: i32,
    y: i32,
) -> Entity {
    counter.0 = counter.0.saturating_add(1);
    let mut stats = CombatStats::default();
    stats.health = <StatPool<i32>>::new(100);
    stats.morale = <StatPool<i32>>::new(50);
    stats.movement = <StatPool<i32>>::new(5);
    stats.kiho = <StatPool<f32>>::new(2.0);
    stats.onmyodo = <StatPool<f32>>::new(1.0);
    stats.yokaijutsu = <StatPool<f32>>::new(1.0);
    stats.kamishin = <StatPool<f32>>::new(1.0);
    stats.lethality = <StatPool<i32>>::new(10);
    stats.hit = <StatPool<i32>>::new(50);
    stats.speed = <StatPool<i32>>::new(5);
    stats.evasion = <StatPool<i32>>::new(5);
    stats.mind = <StatPool<i32>>::new(5);
    commands
        .spawn((
            Sprite {
                color: Color::srgb(0.2, 0.6, 0.9),
                custom_size: Some(Vec2::new(32.0, 32.0)),
                ..default()
            },
            Transform::from_xyz(x as f32, y as f32, 0.0),
            CombatName(name),
            CharacterId(counter.0),
            stats,
        ))
        .id()
}

fn format_status(
    entity: Entity,
    name_q: &Query<(Entity, &CombatName)>,
    stats_q: &mut Query<&mut CombatStats>,
    xp_q: &mut Query<&mut Experience>,
    level_q: &mut Query<&mut Level>,
) -> String {
    let name = name_q
        .iter()
        .find(|(e, _)| *e == entity)
        .map(|(_, n)| n.0.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let body = stats_q.get_mut(entity).ok().map(|s| {
        format!(
            "hp {}/{} morale {}/{} ap {}/{} | kiho {:.2}/{:.2} chisei {:.2}/{:.2} yokai {:.2}/{:.2} kamishin {:.2}/{:.2} \
             | lethality {} hit {} armor {} speed {} evasion {} mind {}",
            s.health.current, s.health.base,
            s.morale.current, s.morale.base,
            s.action_points.current, s.action_points.base,
            s.kiho.current, s.kiho.base,
            s.onmyodo.current, s.onmyodo.base,
            s.yokaijutsu.current, s.yokaijutsu.base,
            s.kamishin.current, s.kamishin.base,
            s.lethality.current, s.hit.current, s.armor.current,
            s.speed.current, s.evasion.current, s.mind.current,
        )
    });
    let xp = xp_q
        .get_mut(entity)
        .ok()
        .map(|x| x.0.to_string())
        .unwrap_or_else(|| "N/A".to_string());
    let level = level_q
        .get_mut(entity)
        .ok()
        .map(|l| l.0.to_string())
        .unwrap_or_else(|| "N/A".to_string());

    match body {
        Some(b) => format!("status {} (xp {} lvl {}): {}", name, xp, level, b),
        None => format!("status {} (xp {} lvl {}): no CombatStats", name, xp, level),
    }
}

#[derive(Clone, Copy)]
enum StatOp {
    Set(i32),
    Add(i32),
}

#[derive(Clone, Copy)]
enum MagicField {
    Current,
    Max,
    Regen,
}

fn school_label(school: MagicSchool) -> &'static str {
    match school {
        MagicSchool::Kiho => "kiho",
        MagicSchool::Onmyodo => "onmyodo",
        MagicSchool::Yokaijutsu => "yokaijutsu",
        MagicSchool::Kamishin => "kamishin",
    }
}

fn apply_magic_school_stat(
    op: StatOp,
    entity: Entity,
    _commands: &mut Commands,
    stats_q: &mut Query<&mut CombatStats>,
    school: MagicSchool,
    field: MagicField,
) -> String {
    let label = school_label(school);

    let Ok(mut stats) = stats_q.get_mut(entity) else {
        return format!("debug: target has no CombatStats; cannot edit {}", label);
    };

    match field {
        MagicField::Current => {
            let pool = stats.pool_mut(school);
            match op {
                StatOp::Set(v) => pool.current = v as f32,
                StatOp::Add(v) => pool.current += v as f32,
            }
            if pool.base < 0.0 {
                pool.base = 0.0;
            }
            pool.current = pool.current.clamp(0.0, pool.base);
            format!("set {} = {:.2} (base {:.2})", label, pool.current, pool.base)
        }
        MagicField::Max => {
            let pool = stats.pool_mut(school);
            match op {
                StatOp::Set(v) => pool.base = v as f32,
                StatOp::Add(v) => pool.base += v as f32,
            }
            if pool.base < 0.0 {
                pool.base = 0.0;
            }
            if pool.current > pool.base {
                pool.current = pool.base;
            }
            format!("set {} base = {:.2}", label, pool.base)
        }
        MagicField::Regen => {
            let target_rate = match school {
                MagicSchool::Kiho => &mut stats.kiho_per_rest_hour,
                MagicSchool::Onmyodo => &mut stats.onmyodo_per_rest_hour,
                MagicSchool::Yokaijutsu => &mut stats.yokaijutsu_per_rest_hour,
                MagicSchool::Kamishin => &mut stats.kamishin_per_rest_hour,
            };
            match op {
                StatOp::Set(v) => *target_rate = v as f32,
                StatOp::Add(v) => *target_rate += v as f32,
            }
            *target_rate = target_rate.max(0.0);
            format!("set {} per-rest-hour = {:.4}", label, *target_rate)
        }
    }
}

fn apply_stat(
    op: StatOp,
    stat: &str,
    entity: Entity,
    commands: &mut Commands,
    stats_q: &mut Query<&mut CombatStats>,
    xp_q: &mut Query<&mut Experience>,
    level_q: &mut Query<&mut Level>,
) -> String {
    let stat = stat.to_ascii_lowercase();
    let pool_field: Option<&str> = match stat.as_str() {
        "hp" | "health" | "hpmax" | "healthmax" | "maxhp" => Some("health"),
        "morale" | "moralemax" | "maxmorale" => Some("morale"),
        "ap" | "actionpoints" | "action_points" | "stam" | "stamina"
        | "apmax" | "actionpointsmax" | "action_points_max" | "stammax" | "staminamax" | "maxstam" => Some("action_points"),
        "movement" | "movementmax" | "maxmovement" => Some("movement"),
        "lethality" | "lethalitymax" | "maxlethality" => Some("lethality"),
        "hit" | "hitmax" | "maxhit" => Some("hit"),
        "armor" | "armormax" | "maxarmor" => Some("armor"),
        "speed" | "speedmax" | "maxspeed" => Some("speed"),
        "evasion" | "evasionmax" | "maxevasion" => Some("evasion"),
        "mind" | "mindmax" | "maxmind" => Some("mind"),
        _ => None,
    };

    if let Some(field) = pool_field {
        let Ok(mut stats) = stats_q.get_mut(entity) else {
            return format!("debug: target has no CombatStats; cannot edit {}", stat);
        };
        let edits_max = matches!(stat.as_str(),
            "hpmax" | "healthmax" | "maxhp" |
            "moralemax" | "maxmorale" |
            "apmax" | "actionpointsmax" | "action_points_max" | "stammax" | "staminamax" | "maxstam" |
            "movementmax" | "maxmovement" |
            "lethalitymax" | "maxlethality" |
            "hitmax" | "maxhit" |
            "armormax" | "maxarmor" |
            "speedmax" | "maxspeed" |
            "evasionmax" | "maxevasion" |
            "mindmax" | "maxmind");
        let pool = pool_mut_by_field(&mut stats, field);
        match (op, edits_max) {
            (StatOp::Set(v), false) => pool.current = v,
            (StatOp::Add(v), false) => pool.current += v,
            (StatOp::Set(v), true) => pool.base = v.max(0),
            (StatOp::Add(v), true) => pool.base = (pool.base + v).max(0),
        }
        if pool.base < 0 {
            pool.base = 0;
        }
        if pool.current > pool.base {
            pool.current = pool.base;
        }
        if pool.current < 0 {
            pool.current = 0;
        }
        return format!("set {} = {} (base {})", stat, pool.current, pool.base);
    }

    match stat.as_str() {
        "mp" | "mana" | "magic" | "mpmax" | "magicmax" | "maxmp" => {
            "use kiho/onmyodo/yokaijutsu/kamishin fields directly".to_string()
        }
        "kiho" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Kiho, MagicField::Current),
        "kihomax" | "maxkiho" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Kiho, MagicField::Max),
        "kihoregen" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Kiho, MagicField::Regen),
        "onmyodo" | "chisei" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Onmyodo, MagicField::Current),
        "onmyodomax" | "chiseimax" | "maxchisei" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Onmyodo, MagicField::Max),
        "onmyodoregen" | "chiseiregen" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Onmyodo, MagicField::Regen),
        "yokaijutsu" | "yokai" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Yokaijutsu, MagicField::Current),
        "yokaijutsumax" | "yokaimax" | "maxyokai" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Yokaijutsu, MagicField::Max),
        "yokaijutsuregen" | "yokairegen" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Yokaijutsu, MagicField::Regen),
        "kamishin" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Kamishin, MagicField::Current),
        "kamishinmax" | "maxkamishin" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Kamishin, MagicField::Max),
        "kamishinregen" => apply_magic_school_stat(op, entity, commands, stats_q, MagicSchool::Kamishin, MagicField::Regen),
        "xp" | "exp" => {
            let value = match op {
                StatOp::Set(value) => value,
                StatOp::Add(delta) => delta,
            }
            .max(0) as u32;
            if let Ok(mut xp) = xp_q.get_mut(entity) {
                match op {
                    StatOp::Set(_) => xp.0 = value,
                    StatOp::Add(_) => xp.0 = xp.0.saturating_add(value),
                }
                return format!("set xp = {}", xp.0);
            }
            commands.entity(entity).insert(Experience(value));
            format!("set xp = {}", value)
        }
        "level" => {
            let value = match op {
                StatOp::Set(value) => value,
                StatOp::Add(delta) => delta,
            }
            .max(0) as u32;
            if let Ok(mut lvl) = level_q.get_mut(entity) {
                match op {
                    StatOp::Set(_) => lvl.0 = value,
                    StatOp::Add(_) => lvl.0 = lvl.0.saturating_add(value),
                }
                return format!("set level = {}", lvl.0);
            }
            commands.entity(entity).insert(Level(value));
            format!("set level = {}", value)
        }
        _ => format!("unknown stat `{}`", stat),
    }
}

fn pool_mut_by_field<'a>(stats: &'a mut CombatStats, field: &str) -> &'a mut StatPool<i32> {
    match field {
        "health" => &mut stats.health,
        "morale" => &mut stats.morale,
        "action_points" => &mut stats.action_points,
        "movement" => &mut stats.movement,
        "lethality" => &mut stats.lethality,
        "hit" => &mut stats.hit,
        "armor" => &mut stats.armor,
        "speed" => &mut stats.speed,
        "evasion" => &mut stats.evasion,
        "mind" => &mut stats.mind,
        _ => unreachable!("unknown pool field {}", field),
    }
}

fn is_slot_name(slot: &str) -> bool {
    matches!(slot, "weapon" | "armor" | "accessory")
}

fn equipment_type_from_name(slot: &str) -> EquipmentType {
    match slot {
        "armor" => EquipmentType::Armor(ArmorType::LightArmor),
        "accessory" => EquipmentType::Accessory(AccessoryType::Charm),
        _ => EquipmentType::Weapon(WeaponType::Sword),
    }
}

fn give_item_to_character(
    item_id: u16,
    slot: Option<&str>,
    entity: Entity,
    equip_entity: Entity,
    commands: &mut Commands,
    inventory_q: &mut Query<&mut Inventory>,
    loadout_q: &mut Query<&mut EquipmentLoadout>,
) -> Vec<String> {
    let slot_name = slot.unwrap_or("weapon");
    let equipment_type = equipment_type_from_name(slot_name);
    let slot_type = equipment_type.slot_type();

    if let Ok(mut inventory) = inventory_q.get_mut(entity) {
        inventory.item_ids.push(item_id);
    } else {
        commands.entity(entity).insert(Inventory {
            item_ids: vec![item_id],
        });
    }

    if let Ok(mut loadout) = loadout_q.get_mut(entity) {
        if loadout.equip_in_first_matching_slot(equipment_type, equip_entity) {
            return vec![format!(
                "give_item: added item {} and equipped {:?} as {:?}",
                item_id, equip_entity, equipment_type
            )];
        }
        return vec![format!(
            "give_item: added item {} but target cannot equip {:?}",
            item_id, equipment_type
        )];
    }

    let mut loadout = EquipmentLoadout::with_slots([slot_type]);
    loadout.equip_in_first_matching_slot(equipment_type, equip_entity);
    commands.entity(entity).insert(loadout);
    vec![format!(
        "give_item: added item {} and created {:?} slot for {:?}",
        item_id, equipment_type, equip_entity
    )]
}
