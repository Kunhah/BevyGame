use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

const SETTINGS_PATH: &str = "saves/settings.ron";

/// User-tweakable graphics & perf settings exposed in the in-game settings menu.
///
/// Keep this type cheap to clone (Copy) and keep the surface narrow — every
/// flag here is something the player can flip from the pause menu, so each
/// one needs an obvious behavioural meaning.
#[derive(Resource, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GraphicsSettings {
    /// Run the shader-side shadow raymarch loop. Disabling makes lighting
    /// ignore occluders entirely (much cheaper on the GPU).
    pub light_raymarch: bool,
    /// Run the per-frame CPU `apply_light_visibility` system, which performs
    /// an O(entities × occluders) line-of-sight check to hide light-sensitive
    /// entities. Disabling means light-sensitive entities are always visible
    /// in range.
    pub light_visibility_culling: bool,
    /// Run the per-frame `update_visual_occluders` fade system. Disabling
    /// keeps occluder sprites at their stored alpha (no fade-when-covered).
    pub visual_occluder_fade: bool,
    /// Periodic logging of occluder motion (debug-only). Off by default.
    pub log_occluder_motion: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self::load_from_disk().unwrap_or_else(Self::initial_defaults)
    }
}

impl GraphicsSettings {
    fn initial_defaults() -> Self {
        Self {
            light_raymarch: true,
            light_visibility_culling: true,
            visual_occluder_fade: true,
            log_occluder_motion: false,
        }
    }

    fn load_from_disk() -> Option<Self> {
        let contents = fs::read_to_string(SETTINGS_PATH).ok()?;
        match ron::de::from_str::<GraphicsSettings>(&contents) {
            Ok(settings) => Some(settings),
            Err(err) => {
                warn!("Failed to parse {}: {err}", SETTINGS_PATH);
                None
            }
        }
    }

    fn save_to_disk(&self) {
        if let Some(parent) = std::path::Path::new(SETTINGS_PATH).parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                warn!("Failed to create settings dir {}: {err}", parent.display());
                return;
            }
        }
        let pretty = ron::ser::PrettyConfig::new()
            .indentor("    ".to_string())
            .struct_names(false);
        match ron::ser::to_string_pretty(self, pretty) {
            Ok(text) => {
                if let Err(err) = fs::write(SETTINGS_PATH, text) {
                    warn!("Failed to write {}: {err}", SETTINGS_PATH);
                }
            }
            Err(err) => warn!("Failed to serialize settings: {err}"),
        }
    }
}

impl GraphicsSettings {
    pub fn label(&self, item: GraphicsToggle) -> &'static str {
        match item {
            GraphicsToggle::LightRaymarch => {
                if self.light_raymarch { "Lighting raymarch: On" } else { "Lighting raymarch: Off" }
            }
            GraphicsToggle::LightVisibilityCulling => {
                if self.light_visibility_culling { "Light visibility CPU: On" } else { "Light visibility CPU: Off" }
            }
            GraphicsToggle::VisualOccluderFade => {
                if self.visual_occluder_fade { "Occluder fade: On" } else { "Occluder fade: Off" }
            }
            GraphicsToggle::LogOccluderMotion => {
                if self.log_occluder_motion { "Occluder motion log: On" } else { "Occluder motion log: Off" }
            }
        }
    }

    pub fn toggle(&mut self, item: GraphicsToggle) {
        match item {
            GraphicsToggle::LightRaymarch => self.light_raymarch = !self.light_raymarch,
            GraphicsToggle::LightVisibilityCulling => {
                self.light_visibility_culling = !self.light_visibility_culling
            }
            GraphicsToggle::VisualOccluderFade => {
                self.visual_occluder_fade = !self.visual_occluder_fade
            }
            GraphicsToggle::LogOccluderMotion => {
                self.log_occluder_motion = !self.log_occluder_motion
            }
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsToggle {
    LightRaymarch,
    LightVisibilityCulling,
    VisualOccluderFade,
    LogOccluderMotion,
}

pub const GRAPHICS_TOGGLES: [GraphicsToggle; 4] = [
    GraphicsToggle::LightRaymarch,
    GraphicsToggle::LightVisibilityCulling,
    GraphicsToggle::VisualOccluderFade,
    GraphicsToggle::LogOccluderMotion,
];

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GraphicsSettings>()
            .add_systems(Update, persist_graphics_settings);
    }
}

/// Persist settings whenever the resource changes. `is_changed()` is true on
/// the frame after `init_resource`, so the first persist happens on startup
/// (which writes the on-disk default if the file did not exist yet).
fn persist_graphics_settings(graphics: Res<GraphicsSettings>) {
    if graphics.is_changed() {
        graphics.save_to_disk();
    }
}
