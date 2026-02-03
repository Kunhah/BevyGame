// === CONFIG ===
const MAX_STEPS: i32 = 32;
const OCCLUSION_THRESHOLD: f32 = 0.5;

// === BINDINGS ===
@group(2) @binding(0) var occlusion_tex: texture_2d<f32>;
@group(2) @binding(1) var occlusion_samp: sampler;

struct Params {
    light_uv: vec2<f32>,
    radius: f32,
    intensity: f32,
    color: vec4<f32>,
    occlusion_size: vec2<f32>,
    visibility: f32,
    debug_mode: f32,
};
@group(2) @binding(2) var<uniform> params: Params;

// === INPUT ===
struct FsIn {
    @location(2) uv: vec2<f32>,
    @builtin(position) pos: vec4<f32>,
};

// === FRAGMENT ===
@fragment
fn fragment(in: FsIn) -> @location(0) vec4<f32> {
    // Debug modes:
    // 1 = solid red overlay
    // 2 = UV gradient (R=uv.x, G=uv.y)
    // 3 = light_uv as color
    // 4 = radial gradient (blue)
    if (params.debug_mode > 3.5) {
        let center = vec2<f32>(0.5, 0.5);
        let d = distance(in.uv, center);
        let t = 1.0 - clamp(d * 2.0, 0.0, 1.0);
        return vec4(0.0, 0.0, t, 1.0);
    }
    if (params.debug_mode > 2.5) {
        return vec4(params.light_uv.x, params.light_uv.y, 0.0, 1.0);
    }
    if (params.debug_mode > 1.5) {
        return vec4(in.uv.x, in.uv.y, 0.0, 1.0);
    }
    if (params.debug_mode > 0.5) {
        return vec4(1.0, 0.0, 0.0, 1.0);
    }

    // Convert from normalized UV (0..1) to pixel space
    let to_px = params.occlusion_size;
    let pix_uv = in.uv;

    // Distance in pixels between this fragment and the light
    let d_px = distance(pix_uv * to_px, params.light_uv * to_px);

    // Ray-march from light to pixel to check occlusion
    let steps = i32(clamp(d_px / 8.0, 6.0, f32(MAX_STEPS))); // fewer samples for speed
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

    // Ambient + light factor to avoid pitch-black output
    // Slight ambient to avoid total black, but keep most darkness outside radius.
    let ambient = 0.0;
    var light = 0.0;
    if (!blocked) {
        // Pixel-space distance keeps the light circular regardless of aspect.
        let falloff = 1.0 - clamp(d_px / params.radius, 0.0, 1.0);
        light = smoothstep(0.0, 1.0, falloff) * params.intensity;
    }
    let light_factor = clamp(ambient + light * params.visibility, 0.0, 1.0);

    // Use alpha to darken the scene outside the radius; keep RGB black.
    // Cap alpha so the map stays faintly visible.
    let darkness = clamp(1.0 - light_factor, 0.0, 1.0);
    let alpha = darkness * 0.6;
    return vec4(0.0, 0.0, 0.0, alpha);
}
