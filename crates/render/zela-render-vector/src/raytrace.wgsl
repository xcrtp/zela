// SPDX-FileCopyrightText: 2026 Academa, Inc.
// SPDX-License-Identifier: MIT
// Modified by Zela (BV-3).

// BV-3：光追 pass（brute-force）。每个像素一条世界空间射线，按 (instance, triangle) 升序
// 遍历所有网格三角形（Möller–Trumbore），取最小 t（相等取 index 小者）；命中则重心插值
// 法线与材质，调 `light.wgsl` 的 `reflected()`（零导数）着色；未命中写 0（透明，让底图透出来）。
//
// 本文件与 `light.wgsl` 拼接成一个 compute 模块，只声明它要用的绑定：
//   group0 = 场景（与光栅同一份 scene 组：view / vertices / indices / instances / extras）
//   group2 = 光追自身（RayParams / ranges / 输出 storage 纹理）
//   group3 = 光照（shadow maps / DFG / 环境 / 遮蔽；`reflected()` 的输入）
//
// 确定性：遍历顺序只由实例下标与三角形下标决定 —— 与线程调度无关。

struct View {
    pixels: vec4<f32>,   // width, height (pixels), samples per pixel, unused
    light: vec4<f32>,    // CE's light position
    toward: vec4<f32>,   // unit vector toward the viewer (two-sided lighting); w: stroke depth bias
    eye: vec4<f32>,      // where it sees from (w = 1: a 3D view's perspective)
    lighting: vec4<f32>, // its lights (`light.wgsl`), how many; the exposure; the tone mapping
    lights: array<Light, 8>,
    harmonics: array<vec4<f32>, 9>, // its environment's diffuse light
    occlusion: vec4<f32>,
};

struct Instance {
    c1_0: vec4<f32>, c1_1: vec4<f32>, c1_2: vec4<f32>, c1_3: vec4<f32>,
    c2_0: vec4<f32>, c2_1: vec4<f32>, c2_2: vec4<f32>, c2_3: vec4<f32>,
    m1_0: vec4<f32>, m1_1: vec4<f32>, m1_2: vec4<f32>,
    m2_0: vec4<f32>, m2_1: vec4<f32>, m2_2: vec4<f32>,
    params: vec4<f32>,
    extra: vec4<f32>,
    ids: vec4<u32>,
    fill: vec4<f32>,
    stroke: vec4<f32>,
    background: vec4<f32>,
    gradient: vec4<f32>,
    brush: vec4<u32>,
    brush2: vec4<u32>,
    material: vec4<f32>, // metallic, roughness, reflectance (an instance with MATERIAL)
};

// 每视图一份（与 `views` 同槽位）：相机逆投影 + 眼睛 / 尺寸。
struct RayParams {
    inverse: mat4x4<f32>, // clip -> world
    eye: vec4<f32>,
    toward: vec4<f32>,
    size: vec4<f32>, // width, height, 0, 0
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> vertices: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> indices: array<u32>;
@group(0) @binding(4) var<storage, read> instances: array<Instance>;
@group(0) @binding(6) var<storage, read> extras: array<vec4<f32>>; // mesh vertex: normal xyz, u
@group(2) @binding(0) var<uniform> ray: RayParams;
@group(2) @binding(1) var<storage, read> ranges: array<vec2<u32>>; // (index_offset, index_count) per instance
@group(2) @binding(2) var out_color: texture_storage_2d<rgba16float, write>;
@group(3) @binding(0) var shadow_maps: texture_depth_2d_array;
@group(3) @binding(1) var shadow_compare: sampler_comparison;
@group(3) @binding(2) var dfg_table: texture_2d<f32>;
@group(3) @binding(3) var dfg_linear: sampler;
@group(3) @binding(4) var environment: texture_cube<f32>;
@group(3) @binding(5) var occlusion: texture_2d<f32>;

fn light_count() -> u32 {
    return u32(view.lighting.x);
}

fn light_at(i: u32) -> Light {
    return view.lights[i];
}

fn shadow_compare_at(uv: vec2<f32>, layer: i32, depth: f32) -> f32 {
    return textureSampleCompareLevel(shadow_maps, shadow_compare, uv, layer, depth);
}

fn shadow_texel() -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(shadow_maps));
}

fn dfg_at(NoV: f32, perceptual: f32) -> vec2<f32> {
    return textureSampleLevel(dfg_table, dfg_linear, vec2<f32>(NoV, perceptual), 0.0).xy;
}

fn environment_at(d: vec3<f32>, level: f32) -> vec3<f32> {
    return textureSampleLevel(environment, dfg_linear, d, level).rgb;
}

fn harmonic(i: u32) -> vec3<f32> {
    return view.harmonics[i].xyz;
}

/// 顶点的世界位置：实例的世界矩阵（3 行 × vec4）乘齐次位置。
fn world_of(i: u32, index: u32) -> vec3<f32> {
    let h = vec4<f32>(vertices[index].xyz, 1.0);
    return vec3<f32>(dot(instances[i].m1_0, h), dot(instances[i].m1_1, h), dot(instances[i].m1_2, h));
}

/// 顶点的世界法线（实例矩阵只取 3×3 的旋转部分；本批不做逆转置）。
fn normal_of(i: u32, index: u32) -> vec3<f32> {
    let n = extras[index].xyz;
    let m = instances[i];
    return vec3<f32>(dot(m.m1_0.xyz, n), dot(m.m1_1.xyz, n), dot(m.m1_2.xyz, n));
}

/// Möller–Trumbore：返回 (t, u, v)；未命中 t < 0。u、v 是三角形重心坐标的另两维。
fn intersect(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, o: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let h = cross(d, e2);
    let det = dot(e1, h);
    if (abs(det) < 1e-9) {
        return vec3<f32>(-1.0, 0.0, 0.0); // 平行（或退化三角形）：不算命中
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = dot(s, h) * inv;
    if (u < 0.0 || u > 1.0) {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if (v < 0.0 || u + v > 1.0) {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    return vec3<f32>(dot(e2, q) * inv, u, v);
}

@compute @workgroup_size(8, 8, 1)
fn raytrace(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(ray.size.xy);
    let xy = vec2<i32>(id.xy);
    if (any(xy >= size)) {
        return;
    }
    // 像素中心 → NDC（y 向上：纹理 y = 0 是画面顶行）→ 世界射线
    let ndc = vec2<f32>(2.0 * (f32(id.x) + 0.5) / ray.size.x - 1.0, 1.0 - 2.0 * (f32(id.y) + 0.5) / ray.size.y);
    let eye = ray.eye.xyz;
    // `z = 0.5` 而不是 0：本视图是**反投影 Z**（`view_3d` 里 z / w = NEAR / 距离），
    // `z = 0` 是无穷远点 ⇒ 逆变换后 `w = 0`，除法得到 inf / NaN（实测：一条命中也打不到）。
    // 任何落在 (0, 1) 的深度都给出同一条射线（透视下同一像素的所有深度共线）。
    let far = ray.inverse * vec4<f32>(ndc, 0.5, 1.0);
    let dir = normalize(far.xyz / far.w - eye);

    var best = 1e30;
    var best_instance = 0u;
    var best_triangle = 0u;
    var best_bary = vec2<f32>(0.0);
    // 实例范围由 uniform 给：`ranges` 缓冲按容量分配（跨作品常驻），`arrayLength` 会带出
    // 上一件作品残留的条目 ⇒ 只能用本帧写进去的 `[base, base + count)`。
    let base = u32(ray.size.w);
    let count = u32(ray.size.z);
    for (var i = base; i < base + count; i = i + 1u) {
        let r = ranges[i];
        let triangles = r.y / 3u;
        for (var t = 0u; t < triangles; t = t + 1u) {
            let at = r.x + t * 3u;
            let a = world_of(i, indices[at]);
            let b = world_of(i, indices[at + 1u]);
            let c = world_of(i, indices[at + 2u]);
            let hit = intersect(a, b, c, eye, dir);
            // 严格小于：t 相等时保留先到的（= 下标更小）—— 平局由顺序消除
            if (hit.x > 1e-4 && hit.x < best) {
                best = hit.x;
                best_instance = i;
                best_triangle = t;
                best_bary = vec2<f32>(hit.y, hit.z);
            }
        }
    }
    if (best >= 1e30) {
        // 未命中：不画（alpha 0 ⇒ 合成只叠命中像素；环境底色仍由底图给出）
        textureStore(out_color, xy, vec4<f32>(0.0));
        return;
    }
    let r = ranges[best_instance];
    let at = r.x + best_triangle * 3u;
    let u = best_bary.x;
    let v = best_bary.y;
    let w = 1.0 - u - v;
    // 重心插值法线（每顶点法线在 extras），已转到世界
    let n = normalize(normal_of(best_instance, indices[at]) * w + normal_of(best_instance, indices[at + 1u]) * u + normal_of(best_instance, indices[at + 2u]) * v);
    let p = eye + dir * best;
    let m = instances[best_instance].material;
    let albedo = instances[best_instance].fill.rgb;
    let vdir = select(view.toward.xyz, normalize(eye - p), ray.eye.w > 0.5);
    let radiance = reflected(albedo, m.x, m.y, m.z, n, p, vdir, 1.0);
    textureStore(out_color, xy, vec4<f32>(radiance * view.lighting.y, 1.0));
}
