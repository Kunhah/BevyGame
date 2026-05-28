//! Phase 5: a small data-driven cutscene sequencer. A cutscene is a RON-loaded
//! timeline of `(at, action)` steps; the player advances by `Time::delta` each
//! frame and fires any steps whose `at` has elapsed. A `CameraTo` step starts
//! a smooth tween of the [`CameraRig`] (focus/yaw/pitch/zoom) over `duration`
//! seconds — so cinematic moves are just data.
//!
//! Triggering: load a `.cutscene.ron` via the asset server and call
//! [`CutscenePlayer::play`], or press the dev hotkey (`F1`) to play the bundled
//! `assets/cutscenes/intro.cutscene.ron`.
//!
//! While a cutscene is playing the camera is unlocked (`Global_Variables.
//! camera_locked = false`) so the rig isn't fighting follow; the prior lock
//! state is restored on `End`.

use bevy::prelude::*;
use bevy_common_assets::ron::RonAssetPlugin;
use serde::Deserialize;

use crate::core::Global_Variables;
use crate::render3d::CameraRig;

/// One scripted step. `at` is seconds from cutscene start.
#[derive(Deserialize, Debug, Clone)]
pub struct CutsceneStep {
    pub at: f32,
    pub action: CutsceneAction,
}

/// What a step does. Extend as new cutscene primitives are needed (dialogue,
/// animation triggers, etc.). Camera moves are smoothly tweened in
/// [`update_camera_tween`].
#[derive(Deserialize, Debug, Clone)]
pub enum CutsceneAction {
    /// Tween the camera rig to these targets (any `None` field is left alone)
    /// over `duration` seconds. `focus` is a world-space ground point (x, y).
    CameraTo {
        #[serde(default)]
        focus: Option<[f32; 2]>,
        #[serde(default)]
        yaw: Option<f32>,
        #[serde(default)]
        pitch: Option<f32>,
        #[serde(default)]
        zoom: Option<f32>,
        duration: f32,
    },
    /// Stop the cutscene and restore the previous camera-lock state.
    End,
}

#[derive(Asset, TypePath, Deserialize, Debug, Clone)]
pub struct CutsceneAsset {
    pub steps: Vec<CutsceneStep>,
}

/// Active tween from the snapshot rig values to the target.
#[derive(Default, Clone, Debug)]
pub struct CameraTween {
    pub from_focus: Vec2,
    pub from_yaw: f32,
    pub from_pitch: f32,
    pub from_zoom: f32,
    pub to_focus: Option<Vec2>,
    pub to_yaw: Option<f32>,
    pub to_pitch: Option<f32>,
    pub to_zoom: Option<f32>,
    pub elapsed: f32,
    pub duration: f32,
}

/// Player state. One cutscene at a time.
#[derive(Resource, Default)]
pub struct CutscenePlayer {
    pub asset: Option<Handle<CutsceneAsset>>,
    pub elapsed: f32,
    pub cursor: usize,
    pub tween: Option<CameraTween>,
    /// Camera-lock state to restore on `End`.
    saved_lock: bool,
    saved_lock_valid: bool,
}

impl CutscenePlayer {
    pub fn is_active(&self) -> bool {
        self.asset.is_some()
    }
    pub fn play(&mut self, handle: Handle<CutsceneAsset>) {
        self.asset = Some(handle);
        self.elapsed = 0.0;
        self.cursor = 0;
        self.tween = None;
    }
    fn stop(&mut self) {
        self.asset = None;
        self.elapsed = 0.0;
        self.cursor = 0;
        self.tween = None;
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Fire any steps whose `at` has elapsed since the last frame. Camera tweens
/// are started here and progressed by [`update_camera_tween`].
pub fn advance_cutscene(
    time: Res<Time>,
    mut player: ResMut<CutscenePlayer>,
    cutscenes: Res<Assets<CutsceneAsset>>,
    mut globals: ResMut<Global_Variables>,
    rig: Res<CameraRig>,
) {
    if !player.is_active() {
        return;
    }
    let Some(asset) = player.asset.as_ref().and_then(|h| cutscenes.get(h)) else {
        return;
    };
    let asset = asset.clone();
    player.elapsed += time.delta_secs();

    while player.cursor < asset.steps.len() {
        let step = &asset.steps[player.cursor];
        if step.at > player.elapsed {
            break;
        }
        match &step.action {
            CutsceneAction::CameraTo {
                focus,
                yaw,
                pitch,
                zoom,
                duration,
            } => {
                // Unlock the camera once at the first move so `drive_camera`
                // doesn't tug the focus back to the player.
                if !player.saved_lock_valid {
                    player.saved_lock = globals.0.camera_locked;
                    player.saved_lock_valid = true;
                    globals.0.camera_locked = false;
                }
                player.tween = Some(CameraTween {
                    from_focus: rig.focus,
                    from_yaw: rig.yaw,
                    from_pitch: rig.pitch,
                    from_zoom: rig.zoom,
                    to_focus: focus.map(|[x, y]| Vec2::new(x, y)),
                    to_yaw: *yaw,
                    to_pitch: *pitch,
                    to_zoom: *zoom,
                    elapsed: 0.0,
                    duration: duration.max(0.001),
                });
            }
            CutsceneAction::End => {
                if player.saved_lock_valid {
                    globals.0.camera_locked = player.saved_lock;
                    player.saved_lock_valid = false;
                }
                info!("cutscene: end");
                player.stop();
                return;
            }
        }
        player.cursor += 1;
    }
}

/// Progresses an active camera tween each frame, writing the smoothstep-eased
/// values into the [`CameraRig`].
pub fn update_camera_tween(
    time: Res<Time>,
    mut player: ResMut<CutscenePlayer>,
    mut rig: ResMut<CameraRig>,
) {
    let Some(tween) = player.tween.as_mut() else {
        return;
    };
    tween.elapsed += time.delta_secs();
    let t = smoothstep((tween.elapsed / tween.duration).clamp(0.0, 1.0));

    if let Some(target) = tween.to_focus {
        rig.focus = tween.from_focus.lerp(target, t);
    }
    if let Some(target) = tween.to_yaw {
        rig.yaw = lerp_f32(tween.from_yaw, target, t);
    }
    if let Some(target) = tween.to_pitch {
        rig.pitch = lerp_f32(tween.from_pitch, target, t);
    }
    if let Some(target) = tween.to_zoom {
        rig.zoom = lerp_f32(tween.from_zoom, target, t);
    }

    if tween.elapsed >= tween.duration {
        player.tween = None;
    }
}

/// Dev hotkey: press `F1` to play the bundled sample cutscene.
pub fn dev_trigger_cutscene(
    keys: Res<ButtonInput<KeyCode>>,
    asset_server: Res<AssetServer>,
    mut player: ResMut<CutscenePlayer>,
) {
    if keys.just_pressed(KeyCode::F1) && !player.is_active() {
        info!("cutscene: playing assets/cutscenes/intro.cutscene.ron");
        player.play(asset_server.load("cutscenes/intro.cutscene.ron"));
    }
}

pub struct CutscenePlugin;

impl Plugin for CutscenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RonAssetPlugin::<CutsceneAsset>::new(&["cutscene.ron"]))
            .init_resource::<CutscenePlayer>()
            .add_systems(Update, dev_trigger_cutscene)
            .add_systems(Update, advance_cutscene.after(dev_trigger_cutscene))
            .add_systems(Update, update_camera_tween.after(advance_cutscene));
    }
}
