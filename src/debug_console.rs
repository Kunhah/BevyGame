use std::collections::VecDeque;

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::combat_plugin::{
    CharacterId, CombatStats, Equipment, EquipmentSlots, Experience, Health, Level, Magic,
    Name as CombatName, Stamina,
};
use crate::core::Player;

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
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.88)),
            BorderRadius::all(Val::Px(10.0)),
            BorderColor::all(Color::srgba(0.1, 0.14, 0.2, 0.9)),
            DebugConsoleRoot,
            Visibility::Hidden,
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        parent.spawn((
            Text::new(""),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.9, 0.96)),
            DebugConsoleOutput,
        ));

        parent.spawn((
            Text::new("> "),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            TextColor(Color::srgb(0.96, 0.96, 0.96)),
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

fn handle_console_input(
    mut commands: Commands,
    mut state: ResMut<DebugConsoleState>,
    key_input: Res<ButtonInput<KeyCode>>,
    asset_server: Res<AssetServer>,
    mut counter: ResMut<DebugEntityCounter>,
    player_q: Query<Entity, With<Player>>,
    name_q: Query<(Entity, &CombatName)>,
    id_q: Query<(Entity, &CharacterId)>,
    mut transforms: Query<&mut Transform>,
    mut health_q: Query<&mut Health>,
    mut magic_q: Query<&mut Magic>,
    mut stamina_q: Query<&mut Stamina>,
    mut stats_q: Query<&mut CombatStats>,
    mut xp_q: Query<&mut Experience>,
    mut level_q: Query<&mut Level>,
    mut slots_q: Query<&mut EquipmentSlots>,
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
                &asset_server,
                &mut counter,
                &player_q,
                &name_q,
                &id_q,
                &mut transforms,
                &mut health_q,
                &mut magic_q,
                &mut stamina_q,
                &mut stats_q,
                &mut xp_q,
                &mut level_q,
                &mut slots_q,
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
    asset_server: &AssetServer,
    counter: &mut DebugEntityCounter,
    player_q: &Query<Entity, With<Player>>,
    name_q: &Query<(Entity, &CombatName)>,
    id_q: &Query<(Entity, &CharacterId)>,
    transforms: &mut Query<&mut Transform>,
    health_q: &mut Query<&mut Health>,
    magic_q: &mut Query<&mut Magic>,
    stamina_q: &mut Query<&mut Stamina>,
    stats_q: &mut Query<&mut CombatStats>,
    xp_q: &mut Query<&mut Experience>,
    level_q: &mut Query<&mut Level>,
    slots_q: &mut Query<&mut EquipmentSlots>,
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
                    entity, name_q, health_q, magic_q, stamina_q, stats_q, xp_q, level_q,
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
                                health_q,
                                magic_q,
                                stamina_q,
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
                                health_q,
                                magic_q,
                                stamina_q,
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
        "give_item" | "give" => {
            let item_id = parts.next();
            let maybe_slot = parts.next();
            let maybe_target = parts.next();
            match item_id.and_then(|id| id.parse::<u32>().ok()) {
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
                                    lethality: 0,
                                    hit: 0,
                                    armor: 0,
                                    agility: 0,
                                    mind: 0,
                                    morale: 0,
                                })
                                .id();
                            equip_item(slot, entity, equip_entity, commands, slots_q)
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
                        asset_server,
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
        _ => vec![format!(
            "unknown command `{}` (try `help`)",
            cmd
        )],
    }
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
        "  give_item|give <item_id> [slot] [target]",
        "  spawn_entity|spawn [name] <x> <y>",
        "",
        "Targets: `player`, `name:<Name>`, `id:<Number>` (default is player).",
        "Stats: hp, hpmax, mp, mpmax, stam, stammax, lethality, hit, armor, agility, mind, morale, xp, level.",
        "Slots: weapon, armor, accessory.",
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
    asset_server: &AssetServer,
    counter: &mut DebugEntityCounter,
    name: String,
    x: i32,
    y: i32,
) -> Entity {
    counter.0 = counter.0.saturating_add(1);
    commands
        .spawn((
            Sprite {
                image: asset_server.load("character.png"),
                color: Color::srgb(0.2, 0.6, 0.9),
                custom_size: Some(Vec2::new(32.0, 32.0)),
                ..default()
            },
            Transform::from_xyz(x as f32, y as f32, 0.0),
            CombatName(name),
            CharacterId(counter.0),
            Health {
                current: 100,
                max: 100,
                regen: 0,
            },
            Magic {
                current: 50,
                max: 50,
                regen: 0,
            },
            Stamina {
                current: 50,
                max: 50,
                regen: 0,
            },
            CombatStats {
                base_lethality: 10,
                base_hit: 50,
                base_armor: 0,
                base_agility: 5,
                base_mind: 5,
                base_morale: 50,
                movement: 5,
            },
        ))
        .id()
}

fn format_status(
    entity: Entity,
    name_q: &Query<(Entity, &CombatName)>,
    health_q: &mut Query<&mut Health>,
    magic_q: &mut Query<&mut Magic>,
    stamina_q: &mut Query<&mut Stamina>,
    stats_q: &mut Query<&mut CombatStats>,
    xp_q: &mut Query<&mut Experience>,
    level_q: &mut Query<&mut Level>,
) -> String {
    let name = name_q
        .iter()
        .find(|(e, _)| *e == entity)
        .map(|(_, n)| n.0.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let hp = health_q
        .get_mut(entity)
        .ok()
        .map(|h| format!("{}/{}", h.current, h.max))
        .unwrap_or_else(|| "N/A".to_string());
    let mp = magic_q
        .get_mut(entity)
        .ok()
        .map(|m| format!("{}/{}", m.current, m.max))
        .unwrap_or_else(|| "N/A".to_string());
    let st = stamina_q
        .get_mut(entity)
        .ok()
        .map(|s| format!("{}/{}", s.current, s.max))
        .unwrap_or_else(|| "N/A".to_string());
    let stats = stats_q.get_mut(entity).ok().map(|s| {
        format!(
            "lethality {} hit {} armor {} agility {} mind {} morale {}",
            s.base_lethality, s.base_hit, s.base_armor, s.base_agility, s.base_mind, s.base_morale
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

    let mut line = format!("status {}: hp {} mp {} stam {} xp {} lvl {}", name, hp, mp, st, xp, level);
    if let Some(stats) = stats {
        line.push_str(" | ");
        line.push_str(&stats);
    }
    line
}

#[derive(Clone, Copy)]
enum StatOp {
    Set(i32),
    Add(i32),
}

fn apply_stat(
    op: StatOp,
    stat: &str,
    entity: Entity,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    magic_q: &mut Query<&mut Magic>,
    stamina_q: &mut Query<&mut Stamina>,
    stats_q: &mut Query<&mut CombatStats>,
    xp_q: &mut Query<&mut Experience>,
    level_q: &mut Query<&mut Level>,
) -> String {
    let stat = stat.to_ascii_lowercase();
    match stat.as_str() {
        "hp" | "health" => {
            let (current, max, regen) = match health_q.get_mut(entity) {
                Ok(mut h) => {
                    if h.max < 0 {
                        h.max = 0;
                    }
                    match op {
                        StatOp::Set(value) => h.current = value,
                        StatOp::Add(delta) => h.current += delta,
                    }
                    h.current = h.current.clamp(0, h.max);
                    return format!("set hp = {} (max {})", h.current, h.max);
                }
                Err(_) => {
                    let value = match op {
                        StatOp::Set(value) => value,
                        StatOp::Add(delta) => delta,
                    };
                    let value = value.max(0);
                    (value, value, 0)
                }
            };
            commands.entity(entity).insert(Health {
                current,
                max,
                regen,
            });
            format!("set hp = {} (max {})", current, max)
        }
        "hpmax" | "healthmax" | "maxhp" => {
            if let Ok(mut h) = health_q.get_mut(entity) {
                match op {
                    StatOp::Set(value) => h.max = value,
                    StatOp::Add(delta) => h.max += delta,
                }
                if h.max < 0 {
                    h.max = 0;
                }
                if h.current > h.max {
                    h.current = h.max;
                }
                return format!("set hpmax = {}", h.max);
            }
            let value = match op {
                StatOp::Set(value) => value,
                StatOp::Add(delta) => delta,
            };
            let value = value.max(0);
            commands.entity(entity).insert(Health {
                current: value,
                max: value,
                regen: 0,
            });
            format!("set hpmax = {}", value)
        }
        "mp" | "mana" | "magic" => {
            let (current, max, regen) = match magic_q.get_mut(entity) {
                Ok(mut m) => {
                    if m.max < 0 {
                        m.max = 0;
                    }
                    match op {
                        StatOp::Set(value) => m.current = value,
                        StatOp::Add(delta) => m.current += delta,
                    }
                    m.current = m.current.clamp(0, m.max);
                    return format!("set mp = {} (max {})", m.current, m.max);
                }
                Err(_) => {
                    let value = match op {
                        StatOp::Set(value) => value,
                        StatOp::Add(delta) => delta,
                    };
                    let value = value.max(0);
                    (value, value, 0)
                }
            };
            commands.entity(entity).insert(Magic {
                current,
                max,
                regen,
            });
            format!("set mp = {} (max {})", current, max)
        }
        "mpmax" | "magicmax" | "maxmp" => {
            if let Ok(mut m) = magic_q.get_mut(entity) {
                match op {
                    StatOp::Set(value) => m.max = value,
                    StatOp::Add(delta) => m.max += delta,
                }
                if m.max < 0 {
                    m.max = 0;
                }
                if m.current > m.max {
                    m.current = m.max;
                }
                return format!("set mpmax = {}", m.max);
            }
            let value = match op {
                StatOp::Set(value) => value,
                StatOp::Add(delta) => delta,
            };
            let value = value.max(0);
            commands.entity(entity).insert(Magic {
                current: value,
                max: value,
                regen: 0,
            });
            format!("set mpmax = {}", value)
        }
        "stam" | "stamina" => {
            let (current, max, regen) = match stamina_q.get_mut(entity) {
                Ok(mut s) => {
                    if s.max < 0 {
                        s.max = 0;
                    }
                    match op {
                        StatOp::Set(value) => s.current = value,
                        StatOp::Add(delta) => s.current += delta,
                    }
                    s.current = s.current.clamp(0, s.max);
                    return format!("set stamina = {} (max {})", s.current, s.max);
                }
                Err(_) => {
                    let value = match op {
                        StatOp::Set(value) => value,
                        StatOp::Add(delta) => delta,
                    };
                    let value = value.max(0);
                    (value, value, 0)
                }
            };
            commands.entity(entity).insert(Stamina {
                current,
                max,
                regen,
            });
            format!("set stamina = {} (max {})", current, max)
        }
        "stammax" | "staminamax" | "maxstam" => {
            if let Ok(mut s) = stamina_q.get_mut(entity) {
                match op {
                    StatOp::Set(value) => s.max = value,
                    StatOp::Add(delta) => s.max += delta,
                }
                if s.max < 0 {
                    s.max = 0;
                }
                if s.current > s.max {
                    s.current = s.max;
                }
                return format!("set stamina max = {}", s.max);
            }
            let value = match op {
                StatOp::Set(value) => value,
                StatOp::Add(delta) => delta,
            };
            let value = value.max(0);
            commands.entity(entity).insert(Stamina {
                current: value,
                max: value,
                regen: 0,
            });
            format!("set stamina max = {}", value)
        }
        "lethality" | "hit" | "armor" | "agility" | "mind" | "morale" => {
            if let Ok(mut stats) = stats_q.get_mut(entity) {
                apply_stat_to_combat_stats(op, &stat, &mut stats);
                return format!("set {} = {}", stat, combat_stat_value(&stat, &stats));
            }
            let mut stats = CombatStats {
                base_lethality: 0,
                base_hit: 0,
                base_armor: 0,
                base_agility: 0,
                base_mind: 0,
                base_morale: 0,
                movement: 0,
            };
            apply_stat_to_combat_stats(op, &stat, &mut stats);
            let value = combat_stat_value(&stat, &stats);
            commands.entity(entity).insert(stats);
            format!("set {} = {}", stat, value)
        }
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

fn apply_stat_to_combat_stats(op: StatOp, stat: &str, stats: &mut CombatStats) {
    let apply = |current: &mut i32| match op {
        StatOp::Set(value) => *current = value,
        StatOp::Add(delta) => *current += delta,
    };
    match stat {
        "lethality" => apply(&mut stats.base_lethality),
        "hit" => apply(&mut stats.base_hit),
        "armor" => apply(&mut stats.base_armor),
        "agility" => apply(&mut stats.base_agility),
        "mind" => apply(&mut stats.base_mind),
        "morale" => apply(&mut stats.base_morale),
        _ => {}
    }
}

fn combat_stat_value(stat: &str, stats: &CombatStats) -> i32 {
    match stat {
        "lethality" => stats.base_lethality,
        "hit" => stats.base_hit,
        "armor" => stats.base_armor,
        "agility" => stats.base_agility,
        "mind" => stats.base_mind,
        "morale" => stats.base_morale,
        _ => 0,
    }
}

fn is_slot_name(slot: &str) -> bool {
    matches!(slot, "weapon" | "armor" | "accessory")
}

fn equip_item(
    slot: Option<&str>,
    entity: Entity,
    equip_entity: Entity,
    commands: &mut Commands,
    slots_q: &mut Query<&mut EquipmentSlots>,
) -> Vec<String> {
    let slot = slot.unwrap_or("weapon");
    let mut slots = match slots_q.get_mut(entity) {
        Ok(slots) => slots,
        Err(_) => {
            commands.entity(entity).insert(EquipmentSlots::default());
            match slots_q.get_mut(entity) {
                Ok(slots) => slots,
                Err(_) => {
                    return vec!["give_item: failed to attach equipment slots".to_string()];
                }
            }
        }
    };

    match slot {
        "weapon" => {
            slots.weapon = Some(equip_entity);
            vec![format!("give_item: equipped weapon {:?}", equip_entity)]
        }
        "armor" => {
            slots.armor = Some(equip_entity);
            vec![format!("give_item: equipped armor {:?}", equip_entity)]
        }
        "accessory" => {
            slots.accessories.push(equip_entity);
            vec![format!("give_item: added accessory {:?}", equip_entity)]
        }
        _ => vec![format!("give_item: unknown slot `{}`", slot)],
    }
}
