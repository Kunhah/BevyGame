//! Lightweight in-game performance overlay.
//!
//! Press **F7** to toggle a small FPS / frame-time readout in the top-right
//! corner. This is the measurement tool: before cutting anything for
//! performance, watch this number while reproducing the slow scene so the work
//! targets the real hotspot instead of a guess.
//!
//! The overlay is essentially free when hidden — the updater early-returns and
//! only the cheap `FrameTimeDiagnosticsPlugin` sampling keeps running.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

#[derive(Component)]
struct PerfOverlayText;

/// Whether the overlay is currently shown. Toggled by F7.
#[derive(Resource, Default)]
struct PerfOverlayVisible(bool);

pub struct PerfOverlayPlugin;

impl Plugin for PerfOverlayPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        }
        app.init_resource::<PerfOverlayVisible>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, (toggle_overlay, update_overlay));
    }
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new("FPS: --"),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        TextColor(Color::srgb(0.3, 1.0, 0.5)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(4.0),
            right: Val::Px(8.0),
            ..default()
        },
        // Sit above HUD/menus so it's always legible while profiling.
        GlobalZIndex(1000),
        Visibility::Hidden,
        PerfOverlayText,
    ));
}

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<PerfOverlayVisible>,
    mut q: Query<&mut Visibility, With<PerfOverlayText>>,
) {
    if !keys.just_pressed(KeyCode::F7) {
        return;
    }
    visible.0 = !visible.0;
    let v = if visible.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut vis in &mut q {
        *vis = v;
    }
}

fn update_overlay(
    visible: Res<PerfOverlayVisible>,
    diagnostics: Res<DiagnosticsStore>,
    mut q: Query<&mut Text, With<PerfOverlayText>>,
) {
    // Hidden overlay → don't even format a string.
    if !visible.0 {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let label = format!("FPS: {fps:.0}  ({frame_ms:.2} ms)");
    for mut text in &mut q {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}
