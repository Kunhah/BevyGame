//! Per-entity shader effects driven by component markers — the starter
//! toolkit for attacks, hits, and spells that combat systems will later
//! trigger. Each effect is a small component you attach to an entity that
//! already has a [`crate::render3d::ToonMaterial`]; a tick system animates
//! the material's uniform over `duration` seconds and removes itself when
//! done.
//!
//! Add a new effect by extending the WGSL `ToonParams` block, adding a field
//! to the Rust `ToonParams`, and writing a tick system here.
//!
//! Demo hotkeys — applied to **every toon-shaded entity in the scene** (player
//! + enemies + the test capsule) so the effect is visible regardless of where
//! the camera is pointed:
//! - **F3** — `HitFlash` (brief warm-white pulse).
//! - **F4** — `Dissolve` (1-second burn-away with hot edge, then re-forms so
//!   the demo is repeatable).

use bevy::prelude::*;

use crate::render3d::ToonMaterial;

/// Brief additive warm-white pulse on the toon material — for "hit", "damage
/// number popped", "power-up" feedback. Intensity ramps from `intensity` down
/// to 0 over `duration` seconds; component then removes itself.
#[derive(Component, Clone, Copy, Debug)]
pub struct HitFlash {
    pub elapsed: f32,
    pub duration: f32,
    pub intensity: f32,
}

impl HitFlash {
    pub fn new(duration: f32, intensity: f32) -> Self {
        Self {
            elapsed: 0.0,
            duration,
            intensity,
        }
    }
}

/// Animated dissolve from 0 (solid) → 1 (fully gone) over `duration`. If
/// `despawn_when_done` is true the entity is despawned at the end (combat use:
/// enemy disintegrates and goes away); otherwise the dissolve uniform is
/// cleared and the component removed so the entity re-forms (demo use).
#[derive(Component, Clone, Copy, Debug)]
pub struct Dissolve {
    pub elapsed: f32,
    pub duration: f32,
    pub despawn_when_done: bool,
}

impl Dissolve {
    /// Permanent dissolve — fades out then despawns.
    pub fn die(duration: f32) -> Self {
        Self {
            elapsed: 0.0,
            duration,
            despawn_when_done: true,
        }
    }
    /// Demo dissolve — fades out then re-forms (resets the uniform).
    pub fn demo(duration: f32) -> Self {
        Self {
            elapsed: 0.0,
            duration,
            despawn_when_done: false,
        }
    }
}

/// Animate `hit_flash` on the entity's toon material; remove the component
/// (and zero the uniform) when the duration elapses.
pub fn tick_hit_flash(
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<ToonMaterial>>,
    mut q: Query<(Entity, &mut HitFlash, &MeshMaterial3d<ToonMaterial>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut flash, mat) in &mut q {
        let first_tick = flash.elapsed == 0.0;
        flash.elapsed += dt;
        let t = (flash.elapsed / flash.duration).clamp(0.0, 1.0);
        // Quick rise then linear fade for a sharp "pop".
        let curve = (1.0 - t) * (1.0 - t);
        let value = flash.intensity * curve;
        let wrote = if let Some(m) = materials.get_mut(&mat.0) {
            m.extension.params.hit_flash = value;
            true
        } else {
            false
        };
        if first_tick {
            info!(
                "tick_hit_flash: entity={:?} value={:.2} wrote_material={}",
                entity, value, wrote
            );
        }
        if t >= 1.0 {
            if let Some(m) = materials.get_mut(&mat.0) {
                m.extension.params.hit_flash = 0.0;
            }
            commands.entity(entity).remove::<HitFlash>();
        }
    }
}

/// Animate `dissolve` 0 → 1 over `duration`; despawn or re-form when complete.
pub fn tick_dissolve(
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<ToonMaterial>>,
    mut q: Query<(Entity, &mut Dissolve, &MeshMaterial3d<ToonMaterial>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut diss, mat) in &mut q {
        let first_tick = diss.elapsed == 0.0;
        diss.elapsed += dt;
        let t = (diss.elapsed / diss.duration).clamp(0.0, 1.0);
        let wrote = if let Some(m) = materials.get_mut(&mat.0) {
            m.extension.params.dissolve = t;
            true
        } else {
            false
        };
        if first_tick {
            info!(
                "tick_dissolve: entity={:?} t={:.2} wrote_material={}",
                entity, t, wrote
            );
        }
        if t >= 1.0 {
            if diss.despawn_when_done {
                commands.entity(entity).despawn();
            } else {
                if let Some(m) = materials.get_mut(&mat.0) {
                    m.extension.params.dissolve = 0.0;
                }
                commands.entity(entity).remove::<Dissolve>();
            }
        }
    }
}

/// Demo hotkeys — apply effects to **every** toon-shaded entity so the demo is
/// always visible (player + enemies + test capsule), regardless of camera focus.
pub fn demo_effect_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    q: Query<Entity, With<MeshMaterial3d<ToonMaterial>>>,
) {
    let f3 = keys.just_pressed(KeyCode::F3);
    let f4 = keys.just_pressed(KeyCode::F4);
    if !f3 && !f4 {
        return;
    }
    let mut count = 0;
    for e in &q {
        if f3 {
            commands.entity(e).insert(HitFlash::new(0.5, 2.5));
        }
        if f4 {
            commands.entity(e).insert(Dissolve::demo(1.5));
        }
        count += 1;
    }
    if f3 {
        info!("effects: HitFlash applied to {} toon entities", count);
    }
    if f4 {
        info!("effects: Dissolve(demo) applied to {} toon entities", count);
    }
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (tick_hit_flash, tick_dissolve, demo_effect_hotkeys));
    }
}
