// Fullscreen post-processing pass: radial vignette + animated film grain.
// Runs after tonemapping so it darkens / grains the final displayed colors.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct PostFxSettings {
    vignette_strength: f32, // 0 = none, 1 = full corner blackout
    vignette_softness: f32, // how gradual the falloff is
    grain_strength: f32,    // 0 = no grain
    time: f32,              // wall-clock seconds, used to animate grain
}
@group(0) @binding(2) var<uniform> settings: PostFxSettings;

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(screen_texture, texture_sampler, in.uv);

    // Radial vignette darkening — pulls focus to the centre.
    let d = distance(in.uv, vec2<f32>(0.5));
    let v = smoothstep(0.5 - settings.vignette_softness, 0.5, d)
        * settings.vignette_strength;
    var rgb = color.rgb * (1.0 - v);

    // Film grain — luminance noise, seeded with time so it shimmers.
    let n = hash21(in.uv * vec2<f32>(1280.0, 720.0) + vec2<f32>(settings.time * 60.0));
    rgb += vec3<f32>((n - 0.5) * settings.grain_strength);

    return vec4<f32>(rgb, color.a);
}
