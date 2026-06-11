//! Live in-game tuning for the visual stack — toon shader, post-processing,
//! lighting, color grading. Toggle the inspector with **`F2`**.
//!
//! Architecture: one [`RenderTuning`] resource is the canonical source of
//! truth. The egui panel mutates it; the [`apply_render_tuning`] system mirrors
//! its fields into the live `PostFxSettings`, `Bloom`, `DistanceFog`,
//! `ColorGrading`, `AmbientLight`, `DirectionalLight` components, and into
//! every `ToonMaterial` asset, every frame. Mutating `Assets<T>` via
//! `iter_mut()` marks the assets dirty so the GPU sees the new uniforms next
//! frame.

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use crate::post_fx::PostFxSettings;
use crate::render3d::ToonMaterial;

/// Live tunable knobs for the whole render stack. Defaults match the committed
/// values in `render3d` / `post_fx` so opening the panel doesn't change the
/// look until you grab a slider.
#[derive(Resource, Clone)]
pub struct RenderTuning {
    // Lighting
    pub ambient_brightness: f32,
    pub ambient_color: [f32; 3],
    pub sun_dir: [f32; 3],
    pub sun_illuminance: f32,

    // Toon material — 3-stop anime ramp
    pub toon_rim_strength: f32,
    pub toon_rim_power: f32,
    pub toon_rim_color: [f32; 3],
    /// Deep-shadow end of the ramp.
    pub toon_shadow_tint: [f32; 3],
    /// Warm "core shadow" mid-stop.
    pub toon_core_shadow_color: [f32; 3],
    /// Deep→core transition position along lit-luminance (0..1).
    pub toon_ramp_t_shadow: f32,
    /// Core→lit transition position (0..1).
    pub toon_ramp_t_lit: f32,
    /// Smoothstep half-width — small = hard cel edges, large = smooth.
    pub toon_ramp_softness: f32,

    // Post-FX (vignette + grain)
    pub vignette_strength: f32,
    pub vignette_softness: f32,
    pub grain_strength: f32,

    // Bloom
    pub bloom_intensity: f32,

    // Fog
    pub fog_density: f32,
    pub fog_color: [f32; 3],

    // Color grading
    pub grade_exposure: f32,
    pub grade_temperature: f32,
    pub grade_tint: f32,
    pub grade_shadows_sat: f32,
    pub grade_midtones_sat: f32,
    pub grade_midtones_contrast: f32,
    pub grade_highlights_sat: f32,

    // UI
    pub panel_open: bool,
}

impl Default for RenderTuning {
    fn default() -> Self {
        Self {
            // Brighter fill so the ground reads as dark slate instead of black
            // (the old 120 + a strong vignette crushed it). Tune live with F2.
            ambient_brightness: 320.0,
            ambient_color: [0.6, 0.7, 1.0],
            sun_dir: [-0.85, 0.4, -0.75],
            sun_illuminance: 11_000.0,

            toon_rim_strength: 0.30,
            toon_rim_power: 3.5,
            toon_rim_color: [0.5, 0.65, 1.0],
            toon_shadow_tint: [0.22, 0.24, 0.36],
            toon_core_shadow_color: [0.62, 0.45, 0.50],
            toon_ramp_t_shadow: 0.18,
            toon_ramp_t_lit: 0.45,
            toon_ramp_softness: 0.04,

            vignette_strength: 0.38,
            vignette_softness: 0.40,
            grain_strength: 0.005,

            bloom_intensity: 0.08,

            fog_density: 0.00025,
            fog_color: [0.06, 0.08, 0.13],

            grade_exposure: -0.05,
            grade_temperature: -0.06,
            grade_tint: 0.0,
            grade_shadows_sat: 0.55,
            grade_midtones_sat: 0.90,
            grade_midtones_contrast: 1.10,
            grade_highlights_sat: 0.85,

            panel_open: false,
        }
    }
}

/// Toggle the tuning window with `F2`.
pub fn toggle_tuning_panel(keys: Res<ButtonInput<KeyCode>>, mut tuning: ResMut<RenderTuning>) {
    if keys.just_pressed(KeyCode::F2) {
        tuning.panel_open = !tuning.panel_open;
    }
}

/// Push the tuning resource into the live components/materials each frame.
#[allow(clippy::type_complexity)]
pub fn apply_render_tuning(
    tuning: Res<RenderTuning>,
    mut cam_q: Query<
        (
            &mut AmbientLight,
            &mut PostFxSettings,
            &mut Bloom,
            &mut DistanceFog,
            &mut ColorGrading,
        ),
        With<crate::core::MainCamera>,
    >,
    mut sun_q: Query<(&mut DirectionalLight, &mut Transform), Without<crate::core::MainCamera>>,
    mut toon_materials: ResMut<Assets<ToonMaterial>>,
) {
    // Camera-attached effects.
    if let Ok((mut ambient, mut post, mut bloom, mut fog, mut grading)) = cam_q.single_mut() {
        ambient.brightness = tuning.ambient_brightness;
        ambient.color = Color::srgb(
            tuning.ambient_color[0],
            tuning.ambient_color[1],
            tuning.ambient_color[2],
        );

        post.vignette_strength = tuning.vignette_strength;
        post.vignette_softness = tuning.vignette_softness;
        post.grain_strength = tuning.grain_strength;

        bloom.intensity = tuning.bloom_intensity;

        fog.color = Color::srgb(tuning.fog_color[0], tuning.fog_color[1], tuning.fog_color[2]);
        fog.falloff = FogFalloff::Exponential {
            density: tuning.fog_density,
        };

        grading.global = ColorGradingGlobal {
            exposure: tuning.grade_exposure,
            temperature: tuning.grade_temperature,
            tint: tuning.grade_tint,
            ..grading.global.clone()
        };
        grading.shadows = ColorGradingSection {
            saturation: tuning.grade_shadows_sat,
            ..grading.shadows.clone()
        };
        grading.midtones = ColorGradingSection {
            saturation: tuning.grade_midtones_sat,
            contrast: tuning.grade_midtones_contrast,
            ..grading.midtones.clone()
        };
        grading.highlights = ColorGradingSection {
            saturation: tuning.grade_highlights_sat,
            ..grading.highlights.clone()
        };
    }

    // Directional sun: there should be exactly one in our scene.
    if let Ok((mut sun, mut sun_tf)) = sun_q.single_mut() {
        sun.illuminance = tuning.sun_illuminance;
        let dir = Vec3::new(tuning.sun_dir[0], tuning.sun_dir[1], tuning.sun_dir[2])
            .try_normalize()
            .unwrap_or(Vec3::new(0.0, 0.0, -1.0));
        *sun_tf = Transform::default().looking_to(dir, Vec3::Z);
    }

    // Toon material assets — iter_mut() marks each handle dirty so the GPU
    // sees the updated uniform next frame.
    for (_id, mat) in toon_materials.iter_mut() {
        let p = &mut mat.extension.params;
        p.rim_strength = tuning.toon_rim_strength;
        p.rim_power = tuning.toon_rim_power;
        p.rim_color = Vec4::new(
            tuning.toon_rim_color[0],
            tuning.toon_rim_color[1],
            tuning.toon_rim_color[2],
            1.0,
        );
        p.shadow_tint = Vec4::new(
            tuning.toon_shadow_tint[0],
            tuning.toon_shadow_tint[1],
            tuning.toon_shadow_tint[2],
            1.0,
        );
        p.core_shadow_color = Vec4::new(
            tuning.toon_core_shadow_color[0],
            tuning.toon_core_shadow_color[1],
            tuning.toon_core_shadow_color[2],
            1.0,
        );
        p.ramp_t_shadow = tuning.toon_ramp_t_shadow;
        p.ramp_t_lit = tuning.toon_ramp_t_lit;
        p.ramp_softness = tuning.toon_ramp_softness;
    }
}

/// Egui inspector. Sliders mutate [`RenderTuning`]; the apply system pushes
/// the changes into the live render every frame.
pub fn render_tuning_panel(mut ctx: EguiContexts, mut tuning: ResMut<RenderTuning>) {
    if !tuning.panel_open {
        return;
    }
    let Ok(ctx) = ctx.ctx_mut() else {
        return;
    };

    let mut open = tuning.panel_open;
    egui::Window::new("Render Tuning (F2)")
        .open(&mut open)
        .default_width(360.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.collapsing("Lighting", |ui| {
                    ui.add(egui::Slider::new(&mut tuning.ambient_brightness, 0.0..=800.0).text("ambient brightness"));
                    ui.horizontal(|ui| {
                        ui.label("ambient color");
                        ui.color_edit_button_rgb(&mut tuning.ambient_color);
                    });
                    ui.add(egui::Slider::new(&mut tuning.sun_illuminance, 0.0..=30_000.0).text("sun illuminance"));
                    ui.label("sun direction (world)");
                    ui.add(egui::Slider::new(&mut tuning.sun_dir[0], -1.0..=1.0).text("x"));
                    ui.add(egui::Slider::new(&mut tuning.sun_dir[1], -1.0..=1.0).text("y"));
                    ui.add(egui::Slider::new(&mut tuning.sun_dir[2], -1.0..=1.0).text("z"));
                });

                ui.collapsing("Toon shader (anime ramp)", |ui| {
                    ui.label("3-stop ramp: deep shadow → warm core → lit");
                    ui.horizontal(|ui| {
                        ui.label("deep shadow");
                        ui.color_edit_button_rgb(&mut tuning.toon_shadow_tint);
                    });
                    ui.horizontal(|ui| {
                        ui.label("core shadow");
                        ui.color_edit_button_rgb(&mut tuning.toon_core_shadow_color);
                    });
                    ui.add(egui::Slider::new(&mut tuning.toon_ramp_t_shadow, 0.0..=1.0).text("ramp t: deep→core"));
                    ui.add(egui::Slider::new(&mut tuning.toon_ramp_t_lit, 0.0..=1.0).text("ramp t: core→lit"));
                    ui.add(egui::Slider::new(&mut tuning.toon_ramp_softness, 0.001..=0.3).text("ramp softness (small = hard cel)").logarithmic(true));
                    ui.separator();
                    ui.add(egui::Slider::new(&mut tuning.toon_rim_strength, 0.0..=2.0).text("rim strength"));
                    ui.add(egui::Slider::new(&mut tuning.toon_rim_power, 0.5..=8.0).text("rim power (tighter = higher)"));
                    ui.horizontal(|ui| {
                        ui.label("rim color");
                        ui.color_edit_button_rgb(&mut tuning.toon_rim_color);
                    });
                });

                ui.collapsing("Post-FX (vignette + grain)", |ui| {
                    ui.add(egui::Slider::new(&mut tuning.vignette_strength, 0.0..=1.5).text("vignette strength"));
                    ui.add(egui::Slider::new(&mut tuning.vignette_softness, 0.05..=0.5).text("vignette softness"));
                    ui.add(egui::Slider::new(&mut tuning.grain_strength, 0.0..=0.2).text("grain strength"));
                });

                ui.collapsing("Bloom", |ui| {
                    ui.add(egui::Slider::new(&mut tuning.bloom_intensity, 0.0..=0.5).text("bloom intensity"));
                });

                ui.collapsing("Fog", |ui| {
                    ui.add(egui::Slider::new(&mut tuning.fog_density, 0.0..=0.003).text("fog density").logarithmic(true));
                    ui.horizontal(|ui| {
                        ui.label("fog color");
                        ui.color_edit_button_rgb(&mut tuning.fog_color);
                    });
                });

                ui.collapsing("Color grading", |ui| {
                    ui.add(egui::Slider::new(&mut tuning.grade_exposure, -2.0..=2.0).text("exposure (stops)"));
                    ui.add(egui::Slider::new(&mut tuning.grade_temperature, -1.0..=1.0).text("temperature (cool/warm)"));
                    ui.add(egui::Slider::new(&mut tuning.grade_tint, -1.0..=1.0).text("tint (green/magenta)"));
                    ui.add(egui::Slider::new(&mut tuning.grade_shadows_sat, 0.0..=2.0).text("shadows saturation"));
                    ui.add(egui::Slider::new(&mut tuning.grade_midtones_sat, 0.0..=2.0).text("midtones saturation"));
                    ui.add(egui::Slider::new(&mut tuning.grade_midtones_contrast, 0.5..=2.0).text("midtones contrast"));
                    ui.add(egui::Slider::new(&mut tuning.grade_highlights_sat, 0.0..=2.0).text("highlights saturation"));
                });

                ui.separator();
                if ui.button("Reset to committed defaults").clicked() {
                    *tuning = RenderTuning::default();
                    tuning.panel_open = true;
                }
            });
        });
    tuning.panel_open = open;
}

pub struct RenderTuningPlugin;

impl Plugin for RenderTuningPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.init_resource::<RenderTuning>()
            .add_systems(Update, toggle_tuning_panel)
            // The tuning values only change when the F2 panel sliders move, so
            // there's no reason to rewrite every camera/fog/grading component and
            // mark every toon material dirty (GPU re-upload + render re-extract)
            // on frames where nothing was touched. Gate on actual resource change;
            // `resource_changed` is true on the first frame, so defaults still apply.
            .add_systems(
                Update,
                apply_render_tuning
                    .after(toggle_tuning_panel)
                    .run_if(resource_changed::<RenderTuning>),
            )
            .add_systems(EguiPrimaryContextPass, render_tuning_panel);
    }
}
