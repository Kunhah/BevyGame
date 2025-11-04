// === CONFIG ===
const MAX_STEPS: i32 = 64;
const OCCLUSION_THRESHOLD: f32 = 0.5;

// === BINDINGS ===
@group(1) @binding(0) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(1) var occlusion_samp: sampler;

struct Params {
    light_uv: vec2<f32>;
    radius: f32;
    intensity: f32;
    color: vec4<f32>;
    occlusion_size: vec2<f32>;
    visibility: f32;
};
@group(1) @binding(2) var<uniform> params: Params;

// === INPUT ===
struct FsIn {
    @location(0) uv: vec2<f32>;
    @builtin(position) pos: vec4<f32>;
};

// === FRAGMENT ===
@fragment
fn fragment(in: FsIn) -> @location(0) vec4<f32> {
    // Convert from normalized UV (0..1) to pixel space
    let to_px = params.occlusion_size;
    let pix_uv = in.uv;

    // Distance in pixels between this fragment and the light
    let d_px = distance(pix_uv * to_px, params.light_uv * to_px);

    // Early discard if outside light radius
    if d_px > params.radius {
        return vec4<f32>(0.0);
    }

    // Ray-march from light to pixel to check occlusion
    let steps = i32(clamp(d_px / 4.0, 8.0, f32(MAX_STEPS))); // adaptive number of samples
    var blocked = false;
    let dir = (pix_uv - params.light_uv) / f32(steps);
    var p = params.light_uv;

    for (var i = 0; i < MAX_STEPS; i = i + 1) {
        if (i >= steps) { break; }
        let occ = textureSample(occlusion_tex, occlusion_samp, p).r;
        if (occ > OCCLUSION_THRESHOLD) {
            blocked = true;
            break;
        }
        p = p + dir;
    }

    // If ray hits occluder, fragment is dark
    if (blocked) {
        return vec4<f32>(0.0);
    }

    // Smooth distance falloff
    let t = 1.0 - clamp(d_px / params.radius, 0.0, 1.0);
    let falloff = smoothstep(0.0, 1.0, t) * params.intensity;

    // Apply overall visibility fade (CPU-controlled)
    let final_intensity = falloff * params.visibility;

    // Final light color
    return vec4<f32>(params.color.rgb * final_intensity, params.color.a * final_intensity);
}