use bevy::prelude::*;

use super::runtime::{CurrentMusic, DialogueCatalog, DialogueRuntime};
use super::schema::{DialogueNode, NodeId, SceneAction};
use super::stage::{FadeOverlay, StageEntry, StageState};

/// Active scene-action timeline. While `active` is true, dialogue input is
/// blocked and `tick_scene_playback` advances through `actions`. When the
/// final action finishes, the runtime is jumped to `next_node`.
#[derive(Resource, Default)]
pub struct ScenePlayback {
    pub active: bool,
    pub actions: Vec<SceneAction>,
    pub idx: usize,
    /// Time remaining (seconds) on a timed action.
    pub time_remaining: f32,
    /// What kind of timed effect, so the tick can interpolate (fade alpha).
    pub timed_kind: TimedKind,
    /// The duration originally set, for normalising the interpolation.
    pub timed_duration: f32,
    pub next_node: Option<NodeId>,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum TimedKind {
    #[default]
    None,
    Wait,
    FadeOut,
    FadeIn,
    Shake,
}

impl ScenePlayback {
    pub fn reset(&mut self) {
        *self = ScenePlayback::default();
    }

    /// Returns true if dialogue input should be blocked this frame.
    pub fn blocking(&self) -> bool {
        self.active
    }
}

/// Begin a playback when the runtime is parked on a Scene node and no
/// playback is active. Called from advance and from the initial-display path
/// in place of the previous `skip_scene_nodes` helper.
pub fn start_scene_playback_if_needed(
    runtime: &DialogueRuntime,
    catalog: &DialogueCatalog,
    playback: &mut ScenePlayback,
) {
    if playback.active {
        return;
    }
    let Some(node) = runtime.current_node(catalog) else {
        return;
    };
    let DialogueNode::Scene(scene_node) = node else {
        return;
    };
    *playback = ScenePlayback {
        active: true,
        actions: scene_node.actions.clone(),
        idx: 0,
        time_remaining: 0.0,
        timed_kind: TimedKind::None,
        timed_duration: 0.0,
        next_node: scene_node.next.clone(),
    };
}

/// Per-frame system: drain instant actions, tick timed ones, and on completion
/// hand control back to the dialogue runtime.
pub fn tick_scene_playback(
    time: Res<Time>,
    mut playback: ResMut<ScenePlayback>,
    mut runtime: ResMut<DialogueRuntime>,
    catalog: Res<DialogueCatalog>,
    mut stage: ResMut<StageState>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut current_music: ResMut<CurrentMusic>,
    mut fade_q: Query<&mut BackgroundColor, With<FadeOverlay>>,
) {
    if !playback.active {
        return;
    }

    // Tick a running timed action first.
    if playback.timed_kind != TimedKind::None {
        let dt = time.delta_secs();
        playback.time_remaining = (playback.time_remaining - dt).max(0.0);

        // Interpolated visuals for fade actions.
        if let Ok(mut bg) = fade_q.single_mut() {
            let progress = if playback.timed_duration > 0.0 {
                1.0 - (playback.time_remaining / playback.timed_duration)
            } else {
                1.0
            };
            match playback.timed_kind {
                TimedKind::FadeOut => bg.0 = Color::srgba(0.0, 0.0, 0.0, progress.clamp(0.0, 1.0)),
                TimedKind::FadeIn => {
                    bg.0 = Color::srgba(0.0, 0.0, 0.0, (1.0 - progress).clamp(0.0, 1.0))
                }
                _ => {}
            }
        }

        if playback.time_remaining > 0.0 {
            return;
        }

        // Snap final state when the timer expires.
        if let Ok(mut bg) = fade_q.single_mut() {
            match playback.timed_kind {
                TimedKind::FadeOut => bg.0 = Color::srgba(0.0, 0.0, 0.0, 1.0),
                TimedKind::FadeIn => bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.0),
                _ => {}
            }
        }
        playback.timed_kind = TimedKind::None;
        playback.timed_duration = 0.0;
        playback.idx += 1;
    }

    // Drain instant actions until we hit either end-of-list or a timed action.
    while playback.idx < playback.actions.len() && playback.timed_kind == TimedKind::None {
        let action = playback.actions[playback.idx].clone();
        match action {
            SceneAction::EnterCharacter {
                name,
                slot,
                expression,
                transition_secs: _,
            } => {
                stage.place(slot, StageEntry { name, expression });
                playback.idx += 1;
            }
            SceneAction::ExitCharacter { name, transition_secs: _ } => {
                stage.remove_named(&name);
                playback.idx += 1;
            }
            SceneAction::SetExpression { name, expression } => {
                stage.set_expression(&name, expression);
                playback.idx += 1;
            }
            SceneAction::SetBackground(path) => {
                stage.background = path;
                playback.idx += 1;
            }
            SceneAction::PlayMusic(track) => {
                if let Some(prev) = current_music.0.take() {
                    commands.entity(prev).despawn();
                }
                if let Some(name) = track {
                    let path = format!("audio/music/{name}");
                    let entity = commands
                        .spawn((
                            AudioPlayer::new(asset_server.load(path)),
                            PlaybackSettings::LOOP,
                        ))
                        .id();
                    current_music.0 = Some(entity);
                }
                playback.idx += 1;
            }
            SceneAction::PlaySfx(name) => {
                let path = format!("audio/sfx/{name}");
                commands.spawn((
                    AudioPlayer::new(asset_server.load(path)),
                    PlaybackSettings::DESPAWN,
                ));
                playback.idx += 1;
            }
            SceneAction::Wait(secs) => {
                start_timed(&mut playback, TimedKind::Wait, secs);
                return;
            }
            SceneAction::FadeOut(secs) => {
                start_timed(&mut playback, TimedKind::FadeOut, secs);
                return;
            }
            SceneAction::FadeIn(secs) => {
                start_timed(&mut playback, TimedKind::FadeIn, secs);
                return;
            }
            SceneAction::ShakeScreen(secs) => {
                // Visual shake isn't wired yet; behaves as a Wait so authored
                // timing is preserved.
                start_timed(&mut playback, TimedKind::Shake, secs);
                return;
            }
        }
    }

    if playback.idx >= playback.actions.len() {
        // Timeline exhausted — hand control back to the runtime so the next
        // node (Line/Choice/another Scene) can render.
        let next = playback.next_node.take();
        let _ = catalog;
        playback.reset();
        runtime.goto(next);
    }
}

fn start_timed(playback: &mut ScenePlayback, kind: TimedKind, secs: f32) {
    let duration = secs.max(0.0);
    playback.timed_kind = kind;
    playback.timed_duration = duration;
    playback.time_remaining = duration;
}
