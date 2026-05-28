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
//! Demo hotkeys (target the `ToonTestCapsule` north of spawn):
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
        flash.elapsed += dt;
        let t = (flash.elapsed / flash.duration).clamp(0.0, 1.0);
        // Quick rise then linear fade for a sharp "pop".
        let curve = (1.0 - t) * (1.0 - t);
        let value = flash.intensity * curve;
        if let Some(m) = materials.get_mut(&mat.0) {
            m.extension.params.hit_flash = value;
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
        diss.elapsed += dt;
        let t = (diss.elapsed / diss.duration).clamp(0.0, 1.0);
        if let Some(m) = materials.get_mut(&mat.0) {
            m.extension.params.dissolve = t;
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

/// Demo hotkeys — apply effects to the `ToonTestCapsule` reference entity.
pub fn demo_effect_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    q: Query<(Entity, &Name)>,
) {
    let target = if keys.just_pressed(KeyCode::F3) || keys.just_pressed(KeyCode::F4) {
        q.iter()
            .find(|(_, n)| n.as_str() == "ToonTestCapsule")
            .map(|(e, _)| e)
    } else {
        None
    };
    let Some(target) = target else {
        return;
    };
    if keys.just_pressed(KeyCode::F3) {
        info!("effects: HitFlash on ToonTestCapsule");
        commands.entity(target).insert(HitFlash::new(0.35, 1.4));
    }
    if keys.just_pressed(KeyCode::F4) {
        info!("effects: Dissolve (demo) on ToonTestCapsule");
        commands.entity(target).insert(Dissolve::demo(1.0));
    }
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (tick_hit_flash, tick_dissolve, demo_effect_hotkeys));
    }
}
