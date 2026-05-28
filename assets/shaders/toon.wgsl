// Toon fragment for `ExtendedMaterial<StandardMaterial, ToonExtension>`.
//
// Runs Bevy's real PBR lighting (so shadows, the directional sun, and ambient
// all apply), then bands the result into flat cel steps and adds an anime
// rim/Fresnel light along the silhouette. The banding keeps hue and clamps the
// darkest step to `shade_floor` so shadowed areas read dark-but-not-black — the
// moody "adult anime" base. Extension uniforms live at @group(2) @binding(100)
// (StandardMaterial owns 0..99).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, alpha_discard},
    pbr_types,
    forward_io::{VertexOutput, FragmentOutput},
}

struct ToonParams {
    rim_color: vec4<f32>,
    // rgb = multiplier applied to shadowed steps (cool/desaturated for the
    // adult-anime look); a = how strongly to apply it.
    shadow_tint: vec4<f32>,
    bands: f32,
    rim_strength: f32,
    rim_power: f32,
    shade_floor: f32,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> toon: ToonParams;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color =
        alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;

    if ((pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u) {
        out.color = pbr_input.material.base_color;
    } else {
        let lit = apply_pbr_lighting(pbr_input);

        // Cel banding: quantize luminance into `bands` steps, preserve hue.
        let lum = max(dot(lit.rgb, vec3<f32>(0.299, 0.587, 0.114)), 1e-4);
        let stepped = max(floor(lum * toon.bands) / toon.bands, toon.shade_floor);
        var color = lit.rgb * (stepped / lum);

        // Deepen + cool the shadowed steps (adult-anime signature).
        let shadow_mix = (1.0 - smoothstep(0.0, 0.55, stepped)) * toon.shadow_tint.a;
        color = mix(color, color * toon.shadow_tint.rgb, shadow_mix);

        // Rim / Fresnel light along the silhouette.
        let rim = pow(1.0 - saturate(dot(pbr_input.N, pbr_input.V)), toon.rim_power)
            * toon.rim_strength;
        color += toon.rim_color.rgb * rim;

        out.color = vec4<f32>(color, lit.a);
    }

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
