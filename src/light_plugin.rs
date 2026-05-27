//! Transitional stub for the 2D→3D port.
//!
//! The original 2D raymarched lighting system (offscreen occlusion pass +
//! fullscreen light quad in WGSL) was removed during the Bevy 0.18 upgrade: its
//! render-target / `Material2d` code did not survive the migration, and the
//! whole approach is replaced by Bevy's built-in 3D lighting in the 3D port.
//!
//! This stub keeps only the small components that many spawn sites still attach
//! (`Occluder`, `LightSensitive`) plus an empty `LightPlugin`, so the rest of
//! the codebase compiles unchanged. These are retired as spawns are converted
//! to 3D in Phase 1.

use bevy::prelude::*;

/// Occluder footprint (carried over from the 2D shadow system).
///
/// `size` is the rectangular footprint in world units; `offset` is added to the
/// entity's translation to position that footprint. Currently inert — kept so
/// existing spawn sites compile until 3D shadow casters replace them.
#[derive(Component, Debug, Clone, Copy)]
pub struct Occluder {
    pub size: Vec2,
    pub offset: Vec2,
}

impl Occluder {
    pub fn new(size: Vec2) -> Self {
        Self { size, offset: Vec2::ZERO }
    }
    pub fn with_offset(size: Vec2, offset: Vec2) -> Self {
        Self { size, offset }
    }
}

/// Marker for entities that the old 2D light system could hide below a light
/// threshold. Currently inert; retained so spawn sites compile.
#[derive(Component, Clone, Copy)]
pub struct LightSensitive {
    pub threshold: f32,
}

/// Empty plugin placeholder. Lighting is provided by Bevy's 3D pipeline in the
/// port; this is removed once no longer registered in `lib.rs`.
pub struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, _app: &mut App) {}
}
