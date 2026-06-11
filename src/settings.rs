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

    // ---- GPU render-quality knobs (applied to the main camera) ----
    /// Bloom around bright highlights / emissives. Moderate GPU cost.
    #[serde(default = "default_true")]
    pub bloom: bool,
    /// Atmospheric distance fog. Cheap; mostly a mood/quality choice.
    #[serde(default = "default_true")]
    pub fog: bool,
    /// Screen-space ambient occlusion (contact shadows). The most expensive
    /// full-screen pass here — the first thing to drop on weak GPUs.
    #[serde(default = "default_true")]
    pub ssao: bool,
    /// Fullscreen vignette + film-grain post pass. Low–moderate cost.
    #[serde(default = "default_true")]
    pub post_fx: bool,
    /// Hide entity outlines beyond a distance from the camera. `bevy_mod_outline`
    /// renders extra geometry per outlined entity, so culling distant ones saves
    /// vertex work at the cost of far-away actors losing their ink line.
    #[serde(default)]
    pub cull_distant_outlines: bool,
    /// Which preset the above knobs currently match (or `Custom` if hand-tuned).
    #[serde(default)]
    pub quality: GraphicsQuality,
}

fn default_true() -> bool {
    true
}

/// Coarse one-click quality tiers. Cycling a tier overwrites every GPU knob
/// above; flipping any individual knob drops the tier to [`GraphicsQuality::Custom`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GraphicsQuality {
    /// Everything heavy off — maximum FPS on weak hardware.
    Low,
    /// Bloom + fog + post-fx, SSAO off, distant outlines culled.
    Medium,
    /// The full intended look — everything on.
    #[default]
    High,
    /// Knobs were adjusted individually and no longer match a tier.
    Custom,
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
            bloom: true,
            fog: true,
            ssao: true,
            post_fx: true,
            cull_distant_outlines: false,
            quality: GraphicsQuality::High,
        }
    }

    /// Overwrite every GPU knob to match a preset tier and record it.
    pub fn apply_quality(&mut self, quality: GraphicsQuality) {
        self.quality = quality;
        match quality {
            GraphicsQuality::Low => {
                self.bloom = false;
                self.fog = false;
                self.ssao = false;
                self.post_fx = false;
                self.cull_distant_outlines = true;
            }
            GraphicsQuality::Medium => {
                self.bloom = true;
                self.fog = true;
                self.ssao = false;
                self.post_fx = true;
                self.cull_distant_outlines = true;
            }
            GraphicsQuality::High => {
                self.bloom = true;
                self.fog = true;
                self.ssao = true;
                self.post_fx = true;
                self.cull_distant_outlines = false;
            }
            // `Custom` is a marker for hand-tuned knobs — applying it changes
            // nothing but the label.
            GraphicsQuality::Custom => {}
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
            GraphicsToggle::QualityPreset => match self.quality {
                GraphicsQuality::Low => "Quality: Low",
                GraphicsQuality::Medium => "Quality: Medium",
                GraphicsQuality::High => "Quality: High",
                GraphicsQuality::Custom => "Quality: Custom",
            },
            GraphicsToggle::Bloom => {
                if self.bloom { "Bloom: On" } else { "Bloom: Off" }
            }
            GraphicsToggle::Fog => {
                if self.fog { "Fog: On" } else { "Fog: Off" }
            }
            GraphicsToggle::Ssao => {
                if self.ssao { "Ambient occlusion: On" } else { "Ambient occlusion: Off" }
            }
            GraphicsToggle::PostFx => {
                if self.post_fx { "Post-FX (vignette/grain): On" } else { "Post-FX (vignette/grain): Off" }
            }
            GraphicsToggle::CullDistantOutlines => {
                if self.cull_distant_outlines { "Cull distant outlines: On" } else { "Cull distant outlines: Off" }
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
            // Cycle Low → Medium → High → Low; applying a preset overwrites the
            // individual knobs below.
            GraphicsToggle::QualityPreset => {
                let next = match self.quality {
                    GraphicsQuality::Low => GraphicsQuality::Medium,
                    GraphicsQuality::Medium => GraphicsQuality::High,
                    GraphicsQuality::High | GraphicsQuality::Custom => GraphicsQuality::Low,
                };
                self.apply_quality(next);
            }
            // Flipping any individual knob means we no longer match a tier.
            GraphicsToggle::Bloom => {
                self.bloom = !self.bloom;
                self.quality = GraphicsQuality::Custom;
            }
            GraphicsToggle::Fog => {
                self.fog = !self.fog;
                self.quality = GraphicsQuality::Custom;
            }
            GraphicsToggle::Ssao => {
                self.ssao = !self.ssao;
                self.quality = GraphicsQuality::Custom;
            }
            GraphicsToggle::PostFx => {
                self.post_fx = !self.post_fx;
                self.quality = GraphicsQuality::Custom;
            }
            GraphicsToggle::CullDistantOutlines => {
                self.cull_distant_outlines = !self.cull_distant_outlines;
                self.quality = GraphicsQuality::Custom;
            }
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsToggle {
    QualityPreset,
    Bloom,
    Fog,
    Ssao,
    PostFx,
    CullDistantOutlines,
    LightRaymarch,
    LightVisibilityCulling,
    VisualOccluderFade,
    LogOccluderMotion,
}

pub const GRAPHICS_TOGGLES: [GraphicsToggle; 10] = [
    GraphicsToggle::QualityPreset,
    GraphicsToggle::Bloom,
    GraphicsToggle::Fog,
    GraphicsToggle::Ssao,
    GraphicsToggle::PostFx,
    GraphicsToggle::CullDistantOutlines,
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
