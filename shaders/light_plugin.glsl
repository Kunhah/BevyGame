#version 450

// =====================================================
// COMMON
// =====================================================

#define INVALID vec2(-1.0, -1.0)

// =====================================================
// SEED PASS
// =====================================================
#ifdef SEED_PASS

layout(local_size_x = 8, local_size_y = 8) in;

layout(push_constant) uniform Push {
    int step;
    vec2 tex_size;
    vec2 light_uv;
    float radius;
    float intensity;
    float visibility;
} push;

layout(set = 0, binding = 0) uniform sampler2D occlusion_tex;
layout(set = 0, binding = 1, rgba32f) writeonly uniform image2D out_img;

void main() {
    ivec2 coord = ivec2(gl_GlobalInvocationID.xy);
    if (coord.x >= int(push.tex_size.x) ||
        coord.y >= int(push.tex_size.y)) return;

    vec2 uv = (vec2(coord) + 0.5) / push.tex_size;
    float occ = texture(occlusion_tex, uv).r;

    if (occ > 0.5)
        imageStore(out_img, coord, vec4(uv, 0.0, 1.0));
    else
        imageStore(out_img, coord, vec4(INVALID, 0.0, 1.0));
}

#endif

// =====================================================
// JFA PASS
// =====================================================
#ifdef JFA_PASS

layout(local_size_x = 8, local_size_y = 8) in;

layout(push_constant) uniform Push {
    int step;
    vec2 tex_size;
    vec2 light_uv;
    float radius;
    float intensity;
    float visibility;
} push;

layout(set = 0, binding = 0) uniform sampler2D input_tex;
layout(set = 0, binding = 1, rgba32f) writeonly uniform image2D out_img;

float dist2(vec2 a, vec2 b) {
    vec2 d = a - b;
    return dot(d, d);
}

void main() {
    ivec2 coord = ivec2(gl_GlobalInvocationID.xy);
    if (coord.x >= int(push.tex_size.x) ||
        coord.y >= int(push.tex_size.y)) return;

    vec2 uv = (vec2(coord) + 0.5) / push.tex_size;

    vec2 best = texture(input_tex, uv).rg;
    float bestDist = 1e20;

    if (best.x >= 0.0)
        bestDist = dist2(best, uv);

    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {

            vec2 offset = vec2(x, y) * float(push.step);
            vec2 suv = (vec2(coord) + offset + 0.5) / push.tex_size;

            if (suv.x < 0.0 || suv.y < 0.0 ||
                suv.x > 1.0 || suv.y > 1.0) continue;

            vec2 candidate = texture(input_tex, suv).rg;
            if (candidate.x < 0.0) continue;

            float d = dist2(candidate, uv);

            if (d < bestDist) {
                bestDist = d;
                best = candidate;
            }
        }
    }

    imageStore(out_img, coord, vec4(best, 0.0, 1.0));
}

#endif

// =====================================================
// DISTANCE PASS
// =====================================================
#ifdef DIST_PASS

layout(local_size_x = 8, local_size_y = 8) in;

layout(push_constant) uniform Push {
    int step;
    vec2 tex_size;
    vec2 light_uv;
    float radius;
    float intensity;
    float visibility;
} push;

layout(set = 0, binding = 0) uniform sampler2D seed_tex;
layout(set = 0, binding = 1, r32f) writeonly uniform image2D sdf_img;

void main() {
    ivec2 coord = ivec2(gl_GlobalInvocationID.xy);
    if (coord.x >= int(push.tex_size.x) ||
        coord.y >= int(push.tex_size.y)) return;

    vec2 uv = (vec2(coord) + 0.5) / push.tex_size;
    vec2 seed = texture(seed_tex, uv).rg;

    if (seed.x < 0.0) {
        imageStore(sdf_img, coord, vec4(1e6));
        return;
    }

    float d = length((seed - uv) * push.tex_size);
    imageStore(sdf_img, coord, vec4(d));
}

#endif

// =====================================================
// LIGHTING PASS (HYBRID)
// =====================================================
#ifdef LIGHT_PASS

layout(location = 2) in vec2 in_uv;
layout(location = 0) out vec4 out_color;

layout(set = 2, binding = 0) uniform texture2D occlusion_tex;
layout(set = 2, binding = 1) uniform sampler occlusion_samp;

layout(set = 2, binding = 2) uniform Params {
    vec2 light_uv;
    float radius;
    float intensity;
    vec4 color;
    vec2 occlusion_size;
    float visibility;
    float debug_mode;
    float raymarch_steps;
    float _pad0;
    float _pad1;
    float _pad2;
} params;

bool raymarch(vec2 uv, vec2 light_uv, int steps) {
    vec2 dir = (uv - light_uv) / float(steps);
    vec2 p = light_uv;

    for (int i = 0; i < steps; i++) {
        float occ = texture(sampler2D(occlusion_tex, occlusion_samp), p).r;
        if (occ > 0.5) return true;
        p += dir;
    }
    return false;
}

void main() {
    vec2 uv = in_uv;

    if (params.debug_mode > 3.5) {
        vec2 center = vec2(0.5, 0.5);
        float d = distance(uv, center);
        float t = 1.0 - clamp(d * 2.0, 0.0, 1.0);
        out_color = vec4(0.0, 0.0, t, 1.0);
        return;
    }
    if (params.debug_mode > 2.5) {
        out_color = vec4(params.light_uv.x, params.light_uv.y, 0.0, 1.0);
        return;
    }
    if (params.debug_mode > 1.5) {
        out_color = vec4(uv.x, uv.y, 0.0, 1.0);
        return;
    }
    if (params.debug_mode > 0.5) {
        out_color = vec4(1.0, 0.0, 0.0, 1.0);
        return;
    }

    vec2 to_px = params.occlusion_size;
    vec2 diff = uv * to_px - params.light_uv * to_px;

    float d2 = dot(diff, diff);
    float radius2 = params.radius * params.radius;

    if (d2 > radius2) {
        out_color = vec4(0.0, 0.0, 0.0, 0.6);
        return;
    }

    float d = sqrt(d2);

    float light = 0.0;
    int steps = int(max(params.raymarch_steps, 0.0));
    bool blocked = (steps > 0) ? raymarch(uv, params.light_uv, steps) : false;
    if (!blocked) {
        float falloff = 1.0 - clamp(d / params.radius, 0.0, 1.0);
        light = smoothstep(0.0, 1.0, falloff) * params.intensity;
    }

    float light_factor = clamp(light * params.visibility, 0.0, 1.0);

    float darkness = 1.0 - light_factor;
    float alpha = darkness * 0.6;

    out_color = vec4(0.0, 0.0, 0.0, alpha);
}

#endif
