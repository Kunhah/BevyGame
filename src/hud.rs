use bevy::prelude::*;

use crate::city_data::CityCatalog;
use crate::constants::TIMESTAMP_TICKS_PER_HOUR;
use crate::core::{GameState, Game_State, Timestamp};
use crate::economy::PlayerWallet;
use crate::map::CurrentArea;
use crate::ui_style::{font_size, palette, radius, spacing};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_exploration_hud)
            .add_systems(Update, toggle_exploration_hud_visibility)
            .add_systems(Update, update_coins_text)
            .add_systems(Update, update_area_text)
            .add_systems(Update, update_time_text);
    }
}

#[derive(Component)]
struct ExplorationHudRoot;

#[derive(Component)]
struct CoinsValue;

#[derive(Component)]
struct AreaValue;

#[derive(Component)]
struct TimeValue;

const ROW_MIN_WIDTH: f32 = 180.0;

fn spawn_exploration_hud(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(spacing::MD),
                right: Val::Px(spacing::MD),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(spacing::XS),
                padding: UiRect::all(Val::Px(spacing::MD)),
                border: UiRect::all(Val::Px(1.0)),
                min_width: Val::Px(220.0),
                border_radius: BorderRadius::all(Val::Px(radius::MD)),
                ..default()
            },
            BackgroundColor(palette::BG_PANEL),
            BorderColor::all(palette::BORDER_SUBTLE),
            Visibility::Hidden,
            ExplorationHudRoot,
        ))
        .with_children(|root| {
            spawn_stat_row(root, "Area", "--", palette::TEXT_HEADING, AreaValue);
            spawn_stat_row(root, "Time", "--", palette::TEXT_PRIMARY, TimeValue);
            spawn_stat_row(root, "Money", "--", palette::ACCENT_WARNING, CoinsValue);
        });
}

fn spawn_stat_row<M: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    initial_value: &str,
    value_color: Color,
    marker: M,
) {
    parent
        .spawn(Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: Val::Px(spacing::MD),
            min_width: Val::Px(ROW_MIN_WIDTH),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label.to_string()),
                TextFont {
                    font_size: font_size::LABEL,
                    ..default()
                },
                TextColor(palette::TEXT_SECONDARY),
            ));
            row.spawn((
                Text::new(initial_value.to_string()),
                TextFont {
                    font_size: font_size::BODY,
                    ..default()
                },
                TextColor(value_color),
                marker,
            ));
        });
}

fn toggle_exploration_hud_visibility(
    game_state: Res<GameState>,
    mut q: Query<&mut Visibility, With<ExplorationHudRoot>>,
) {
    let visible = matches!(
        game_state.0,
        Game_State::Exploring
            | Game_State::Interacting
            | Game_State::MapOpen
            | Game_State::Traveling
            | Game_State::Shopping,
    );
    for mut vis in &mut q {
        let desired = if visible { Visibility::Visible } else { Visibility::Hidden };
        if *vis != desired {
            *vis = desired;
        }
    }
}

fn update_coins_text(
    wallet: Res<PlayerWallet>,
    mut q: Query<&mut Text, With<CoinsValue>>,
) {
    for mut text in &mut q {
        let formatted = wallet.coins.format_short();
        if text.0 != formatted {
            text.0 = formatted;
        }
    }
}

fn update_area_text(
    current_area: Res<CurrentArea>,
    catalog: Res<CityCatalog>,
    mut q: Query<&mut Text, With<AreaValue>>,
) {
    let label = match catalog.0.get(&current_area.0) {
        Some(city) => city.name.clone(),
        None => format!("Region #{}", current_area.0),
    };
    for mut text in &mut q {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

fn update_time_text(timestamp: Res<Timestamp>, mut q: Query<&mut Text, With<TimeValue>>) {
    let total_hours = timestamp.0 / TIMESTAMP_TICKS_PER_HOUR;
    let day = total_hours / 24 + 1;
    let hour = total_hours % 24;
    let minute_ticks = timestamp.0 % TIMESTAMP_TICKS_PER_HOUR;
    let minute = (minute_ticks * 60 / TIMESTAMP_TICKS_PER_HOUR) % 60;
    let formatted = format!("Day {} · {:02}:{:02}", day, hour, minute);
    for mut text in &mut q {
        if text.0 != formatted {
            text.0 = formatted.clone();
        }
    }
}

#[allow(dead_code)]
fn format_thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}
