// Toon fragment for `ExtendedMaterial<StandardMaterial, ToonExtension>`.
//
// Bevy's real PBR pass runs first (shadows, sun, ambient, IBL, normal maps,
// emissive, AO — every StandardMaterial texture is honored). We then re-color
// the lit result through a **3-stop "anime ramp"** sampled at the lit-luminance
// instead of luminance-banding it. The ramp keeps the lit hue (so base-color
// textures, normal maps and shadow maps still drive *where* shading falls) but
// remaps the brightness curve into three discrete tones — deep cool shadow,
// warm core-shadow band, full lit — for the Genshin/Guilty Gear feel.
//
// Drop-in point for a real ramp texture: replace `anime_ramp(t)` with a
// `textureSample(ramp_tex, ramp_sampler, vec2(t, ramp_row)).rgb` call. Same
// inputs/outputs, no other shader change needed.
//
// Extension uniforms live at @group(MATERIAL_BIND_GROUP) @binding(100)
// (StandardMaterial owns 0..99).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, alpha_discard},
    pbr_types,
    forward_io::{VertexOutput, FragmentOutput},
}

struct ToonParams {
    rim_color: vec4<f32>,
    // Deep-shadow end of the ramp (rgb). `a` reserved.
    shadow_tint: vec4<f32>,
    // Warm "core shadow" mid-stop of the ramp (rgb). `a` reserved.
    core_shadow_color: vec4<f32>,
    rim_strength: f32,
    rim_power: f32,
    // Ramp transition positions along the lit-luminance axis (0..1).
    ramp_t_shadow: f32,
    ramp_t_lit: f32,
    // Smoothstep half-width at each transition (small = hard cel edge).
    ramp_softness: f32,
    // Per-entity effects (`crate::effects`):
    // hit_flash > 0 = additive warm-white pulse.
    hit_flash: f32,
    // dissolve 0..1 = noise-discard with hot orange burn edge above threshold.
    dissolve: f32,
    _pad: f32,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> toon: ToonParams;

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// 3-stop anime ramp: deep shadow → warm core shadow → lit. Returns an RGB
// multiplier in [shadow_tint..1+]. Replace this body with a ramp-texture
// sample later — same `t` in, same vec3 out.
fn anime_ramp(t: f32) -> vec3<f32> {
    let s = max(toon.ramp_softness, 1e-4);
    let deep = toon.shadow_tint.rgb;
    let core = toon.core_shadow_color.rgb;
    let lit  = vec3<f32>(1.0, 1.0, 1.0);
    let core_mix = smoothstep(toon.ramp_t_shadow - s, toon.ramp_t_shadow + s, t);
    let lit_mix  = smoothstep(toon.ramp_t_lit    - s, toon.ramp_t_lit    + s, t);
    return mix(mix(deep, core, core_mix), lit, lit_mix);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Dissolve: discard fragments whose noise sample is below the dissolve
    // amount. Sampled in mesh UV so the pattern is stable per-mesh.
    var edge_glow = 0.0;
    if (toon.dissolve > 0.0) {
        let n = hash21(in.uv * vec2<f32>(48.0));
        if (n < toon.dissolve) {
            discard;
        }
        // Hot edge band just above the threshold.
        edge_glow = 1.0 - smoothstep(toon.dissolve, toon.dissolve + 0.08, n);
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color =
        alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;

    if ((pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u) {
        out.color = pbr_input.material.base_color;
    } else {
        // Full PBR lit color — inherits base_color/normal/metal-rough/AO/
        // emissive textures and real shadow casting from StandardMaterial.
        let lit = apply_pbr_lighting(pbr_input);

        // Separate the lit color into hue * energy. `t` is the ramp coordinate
        // — luminance of the PBR-lit result, which folds in NdotL, shadows
        // and AO automatically (so the ramp boundary follows shadow shapes).
        let lum = max(dot(lit.rgb, vec3<f32>(0.299, 0.587, 0.114)), 1e-4);
        let hue = lit.rgb / lum;
        let t = saturate(lum);

        // Sample the anime ramp and recolor.
        var color = hue * anime_ramp(t);

        // Rim / Fresnel light along the silhouette.
        let rim = pow(1.0 - saturate(dot(pbr_input.N, pbr_input.V)), toon.rim_power)
            * toon.rim_strength;
        color += toon.rim_color.rgb * rim;

        // Hit flash: brief additive warm-white pulse over the shaded color.
        color += toon.hit_flash * vec3<f32>(1.0, 0.95, 0.8);

        // Dissolve edge glow — hot orange so it reads as "burning away".
        color += edge_glow * vec3<f32>(1.4, 0.55, 0.15);

        out.color = vec4<f32>(color, lit.a);
    }

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
