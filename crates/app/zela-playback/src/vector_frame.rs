//! 一帧内容的**矢量渲染**与路由特征（B11 Step 4/5）。
//!
//! # 为什么在 playback
//!
//! native（`zela-app::frame_loop`）与浏览器（`zela-web`）都要做同一件事：
//! 「读调度器 → 判 3D/2D → 喂矢量后端」。`zela-app` 带 Lua / Typst，不能进浏览器；
//! 本 crate 是两边唯一的公共层 ⇒ 快照与渲染都放这里，避免两份实现漂移。
//!
//! # 口径
//!
//! - 快照只读**当前帧**（调用方已 `step_frame` 推进）；
//! - 3D 网格记录在渲染时再取（`mesh_records` 对线框会显式报错，必须在路由**之后**）。

use ze_notion::{Path2D, Vec2};
use ze_scene_core::domain::LayerDescriptor;
use ze_scene_core::LayerKind;
use zela_render::draw::DrawQueue;
use zela_render_backends::routing::FrameContent;
use zela_render_backends::{Recorded2D, snapshot_queue};

use crate::Runtime;

/// 一帧的内容纹理层（PC 路线 A：**图层级**，不是整帧）。
///
/// 内容层的 draw 在 `ContentTextureQueue` 里（按 `LayerId` 索引），与视口 `DrawQueue`
/// 互不相交。`composite = true` 的层在视口之后按 contain 居中叠加；`composite = false`
/// 的层要被 3D 材质采样（PG Step 2 起走**路线 B**：作为相机视图先渲，网格按
/// `handle` 采样它）。
struct ContentSnapshot {
    /// 该层的纹理句柄 id —— 也就是主帧网格记录里的 `texture` 键。
    handle: u64,
    size: [u32; 2],
    composite: bool,
    blend: ze_render_ir::LayerBlend,
    opacity: f32,
    draws: Vec<Recorded2D>,
}

/// 书写动画项（B14）：`path` 已由录制层换算到**屏幕空间**，`style` 的宽度也已是像素。
///
/// 矢量后端没有 reveal 管线 ⇒ 这里按**归一化弧长**把 `[start, end]` 裁成开放折线再描边。
/// 与 wgpu 的差别只有"按弧长预细分 + 着色器丢弃" vs "CPU 裁剪"，可见部分是同一段几何。
struct RevealSnapshot {
    path: Path2D,
    start: f32,
    end: f32,
    style: ze_render_ir::Style2D,
}

/// 一帧的内容快照：路由读它的特征，矢量渲染复用同一份数据。
pub struct VectorFrame {
    layers: Vec<LayerDescriptor>,
    draws: Vec<Recorded2D>,
    reveals: Vec<RevealSnapshot>,
    /// **PH Step 1**：形变项（`MorphShape` 录制产物）。`from` / `to` 是屏幕空间路径，
    /// 逐帧变的只有 `t`；映射见 `zela_render_vector::morph_records`。
    morphs: Vec<zela_render::draw::MorphItem>,
    content: Vec<ContentSnapshot>,
    mesh_count: usize,
    has_line_meshes: bool,
    /// 本帧网格材质引用的纹理句柄（PG Step 2：路由判断"这些纹理有没有生产方"）。
    mesh_textures: Vec<u64>,
    /// 录制坐标系的原点（= 本帧 `ViewRect.min`）。
    ///
    /// 2D 记录是**绝对屏幕坐标**（egui 点）：GUI 的视口从工具条下方开始，min ≠ 0；
    /// 矢量画布只有视口那么大 ⇒ 取帧时必须把原点去掉，否则整幅内容按视口偏移平移、
    /// 底部被裁掉。headless 的 `ViewRect.min = 0` ⇒ 本字段为 0，基线逐位不变。
    origin: [f32; 2],
    /// **阶段 2（着色 codegen）**：本帧时钟（秒），按参数表注入。
    time: f32,
    /// 有外部注入参数 ⇒ 静止帧复用必须失效（参数可能每帧变）。
    shader_params: bool,
}

/// **阶段 2**：一个外部注入值（按槽类型写进 uniform 字节）。
enum ShaderValue {
    F32(f32),
    I32(i32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Mat4([[f32; 4]; 4]),
}

impl ShaderValue {
    fn write(self, bytes: &mut [u8], at: usize, kind: ze_render_ir::ShaderParamKind) {
        let put = |bytes: &mut [u8], at: usize, values: &[f32]| {
            for (i, value) in values.iter().enumerate() {
                let offset = at + i * 4;
                if offset + 4 <= bytes.len() {
                    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
        };
        match (self, kind) {
            (Self::F32(value), _) => put(bytes, at, &[value]),
            (Self::I32(value), _) => {
                if at + 4 <= bytes.len() {
                    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
            (Self::Vec2(value), _) => put(bytes, at, &value),
            (Self::Vec3(value), _) => put(bytes, at, &value),
            (Self::Mat4(matrix), _) => {
                for (row, values) in matrix.iter().enumerate() {
                    put(bytes, at + row * 16, values);
                }
            }
        }
    }
}

impl VectorFrame {
    /// **阶段 2**：外部注入的供给者表（通道名 → 值）。今天只有 	ime；
    /// 其余通道显式报"缺少供给者"（不填 0）。
    fn shader_value(&self, channel: &str) -> Option<ShaderValue> {
        match channel {
            "time" => Some(ShaderValue::F32(self.time)),
            _ => None,
        }
    }

    /// 在调度器借用期内取完（`DrawQueue` 不是 `Clone`）。
    pub fn take(runtime: &Runtime) -> Result<Self, String> {
        runtime
            .driver
            .with_scheduler(|scheduler| -> Result<_, String> {
                let view_options = zela_render::FrameOptions::default();
                let view = zela_render::frame::build_scene_view(
                    &zela_scene_ecs::contract::WorldView::new(scheduler.world()),
                    scheduler.resources(),
                    view_options,
                );
                let layers = view.layers.clone();
                // 录制原点：2D 记录是绝对屏幕坐标，矢量画布只有视口那么大（见 `origin`）。
                let origin = scheduler
                    .resources()
                    .get::<zela_render::ViewRect>()
                    .map(|rect| [rect.rect.min.x, rect.rect.min.y])
                    .unwrap_or([0.0, 0.0]);
                let queue = scheduler.resources().get::<DrawQueue>();
                let draws = queue
                    .map(zela_render_backends::snapshot_queue)
                    .unwrap_or_default();
                // PC：内容纹理层的 draw 在**独立队列**里（按 LayerId）。尺寸 / 合成开关 /
                // 混合来自图层声明；拿不到队列 ⇒ 空（该层什么都没画，不是静默丢内容）。
                let content_queues = scheduler
                    .resources()
                    .get::<zela_render::ContentTextureQueue>();
                let content: Vec<ContentSnapshot> = layers
                    .iter()
                    .filter_map(|layer| match &layer.kind {
                        LayerKind::ContentTexture {
                            size_px,
                            composite,
                            texture,
                            ..
                        } => Some(ContentSnapshot {
                            handle: texture.id(),
                            size: *size_px,
                            composite: *composite,
                            blend: layer.blend,
                            opacity: layer.opacity,
                            draws: content_queues
                                .and_then(|queues| queues.0.get(&layer.id))
                                .map(snapshot_queue)
                                .unwrap_or_default(),
                        }),
                        _ => None,
                    })
                    .collect();
                // B14：reveal 与 morph 是**独立队列**（不在 `DrawQueue` 里）。
                // 只取 DrawQueue ⇒ 网格线 / 摆杆 / 轨迹弧整片消失（X4-01 的真实宽度）。
                let reveals: Vec<RevealSnapshot> = scheduler
                    .resources()
                    .get::<zela_render::RevealQueue>()
                    .map(|queue| {
                        queue
                            .0
                            .iter()
                            .map(|item| RevealSnapshot {
                                path: item.path.clone(),
                                start: item.start,
                                end: item.end,
                                style: item.style.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let morphs = scheduler
                    .resources()
                    .get::<zela_render::MorphQueue>()
                    .map(|queue| queue.0.clone())
                    .unwrap_or_default();
                // ⚠️ B14：**不要在这里取灯**。`take()` 早于 3D 录制阶段运行，
                // 此时 `LightQueue` 资源还不存在（实测 `None`）⇒ 会静默退回默认灯。
                // 灯与网格一样，必须在 `render()`（渲染时）再取。
                let (mesh_count, has_line_meshes) = match queue {
                    Some(queue) => {
                        let mut count = 0usize;
                        let mut lines = false;
                        for mesh in queue.meshes() {
                            count += 1;
                            if mesh.geometry.topology
                                != ze_scene_core::components::PrimitiveTopology::Triangles
                            {
                                lines = true;
                            }
                        }
                        (count, lines)
                    }
                    None => (0, false),
                };
                // PG Step 2：材质引用的纹理句柄（没有 texture 的网格不进表）。
                let mesh_textures: Vec<u64> = queue
                    .map(|queue| {
                        queue
                            .meshes()
                            .filter_map(|mesh| mesh.material.texture.map(|handle| handle.id()))
                            .collect()
                    })
                    .unwrap_or_default();
                // **阶段 2**：本帧时钟 + 是否有外部注入参数。
                let time = scheduler
                    .resources()
                    .get::<ze_scene_core::TimeState>()
                    .map(|state| state.elapsed)
                    .unwrap_or(0.0);
                let shader_params = draws.iter().any(|draw| {
                    draw.style.paint.shader.as_ref().is_some_and(|shader| !shader.params.is_empty())
                });
                Ok(Self {
                    layers,
                    draws,
                    reveals,
                    morphs,
                    content,
                    mesh_count,
                    has_line_meshes,
                    mesh_textures,
                    origin,
                    time,
                    shader_params,
                })
            })
    }

    /// 两张**纯 2D** 帧是否逐项相同（同一作品、静止画面 ⇒ 可以直接复用上一帧的像素）。
    ///
    /// 只对不含 3D 相机的帧成立：3D 的网格 / 灯在**渲染时**才从队列读（不在快照里），
    /// 快照相同并不代表画面相同 —— 那种情况一律重渲，不冒险。
    pub fn same_2d_frame(&self, other: &Self) -> bool {
        // **阶段 2**：参数可能每帧变（时间 / 输入）⇒ 几何相同也必须重渲。
        if self.shader_params || other.shader_params {
            return false;
        }
        if self.content().has_3d_camera || other.content().has_3d_camera {
            return false;
        }
        same_draws(&self.draws, &other.draws)
            && self.reveals.len() == other.reveals.len()
            && self.reveals.iter().zip(&other.reveals).all(|(a, b)| {
                a.start == b.start && a.end == b.end && a.style == b.style && a.path == b.path
            })
            // **PH Step 1**：形变逐帧改 `t` ⇒ 不在缓存键里就等于把动画冻在第一帧。
            && self.morphs.len() == other.morphs.len()
            && self.morphs.iter().zip(&other.morphs).all(|(a, b)| {
                a.t == b.t
                    && a.samples == b.samples
                    && a.from == b.from
                    && a.to == b.to
                    && a.style == b.style
            })
            && self.content.len() == other.content.len()
            && self.content.iter().zip(&other.content).all(|(a, b)| {
                a.handle == b.handle
                    && a.size == b.size
                    && a.composite == b.composite
                    && a.blend == b.blend
                    && a.opacity == b.opacity
                    && same_draws(&a.draws, &b.draws)
            })
    }

    /// 路由输入：**只描述帧内容**（C1：不含负载 / 墙钟 / 随机数）。
    pub fn content(&self) -> FrameContent {
        let has_3d_camera = self
            .layers
            .iter()
            .any(|layer| matches!(layer.kind, LayerKind::Viewport3D { .. }));
        let has_content_texture = !self.content.is_empty();
        // **PG Step 2（路线 B）**：`composite = false` 的层由矢量侧当相机视图先渲、
        // 网格按句柄采样（[`ContentSnapshot::handle`]）。唯一画不了的情形是材质引用的
        // 纹理**没有生产方**（不是本帧的内容层）⇒ 显式拒绝，不静默贴白。
        let content_handles: Vec<u64> = self.content.iter().map(|layer| layer.handle).collect();
        let has_unserved_material_texture = self
            .mesh_textures
            .iter()
            .any(|texture| !content_handles.contains(texture));
        // PE Step 3：本帧用到多少**仅矢量支持**的绘制参数（wgpu 会忽略这些字段，
        // 路由必须把"忽略"写进决策理由，不得静默）。
        let vector_only_styles = self
            .draws
            .iter()
            .map(|recorded| recorded.style.paint.feature_bits())
            .chain(
                self.reveals
                    .iter()
                    .map(|reveal| reveal.style.paint.feature_bits()),
            )
            .fold(0u8, |bits, next| bits | next);
        FrameContent {
            has_3d_camera,
            // B14：reveal 也是 2D 图元（独立队列，但同样占画面）。
            // PH Step 1：形变项同样是 2D 图元（3D 帧里以 OVERLAY 叠加）。
            has_2d_primitives: self.draws.len() + self.reveals.len() + self.morphs.len(),
            has_meshes: self.mesh_count,
            has_line_meshes: self.has_line_meshes,
            has_content_texture,
            has_material_content: self.content.iter().any(|layer| !layer.composite),
            has_unserved_material_texture,
            has_morphs: !self.morphs.is_empty(),
            vector_only_styles,
        }
    }
}

/// 一帧的渲染输入：3D（view + 网格记录）或 2D（背景 + 记录），都已转成矢量后端的类型。
///
/// **B15**：同步 / 异步 / GPU 目标三条路径共用这一份准备，避免几何映射漂移。
enum Prepared {
    ThreeD {
        view: zela_render_vector::View3,
        /// PG Step 2：先渲的相机视图（内容层当纹理源）。空 ⇒ 与 PG 之前逐位相同。
        cameras: Vec<zela_render_vector::CameraView>,
        records: Vec<zela_render_vector::PathRecord>,
    },
    TwoD {
        background: [f32; 4],
        records: Vec<zela_render_vector::PathRecord>,
    },
}

impl VectorFrame {
    /// 准备一帧的矢量渲染输入。
    ///
    /// `scale` = **点 → 画布像素**（HiDPI；headless = 1）：2D 记录是布局点（egui points），
    /// 而矢量画布的坐标必须与画布像素一致 ⇒ 这里按它（a）去掉视口原点、（b）缩放几何与长度。
    #[allow(clippy::too_many_arguments)]
    fn prepare(
        &self,
        runtime: &Runtime,
        width: u32,
        height: u32,
        scale: f32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<Prepared, String> {
        let camera3d = self.layers.iter().find_map(|layer| match &layer.kind {
            LayerKind::Viewport3D { camera } => Some(*camera),
            _ => None,
        });
        match camera3d {
            Some(camera) => {
                let mut view =
                    crate::vector3d::view3(&camera, (width, height), background, params)?;
                // **PH Step 2**：线框细条需要"朝向眼睛"与 1 像素对应的世界长度
                // （焦距平面处；场景层没有 line_width ⇒ 宽度不可由作者控制）。
                let lines = crate::vector3d::LineStyle {
                    toward_eye: view.axes[2],
                    world_per_pixel: view.frame.1 / height.max(1) as f32,
                };
                // B14 / X4-02：网格与灯**都必须在渲染时取**（`take()` 太早，`LightQueue`
                // 那时还不存在 ⇒ 以前一直静默用默认灯，画面系统性偏暗）。
                let (mut records, lights) = runtime
                    .driver
                    .with_scheduler(|scheduler| -> Result<_, String> {
                        let lights = crate::vector3d::lights(
                            scheduler.resources().get::<zela_render::LightQueue>(),
                        );
                        let records = match scheduler.resources().get::<DrawQueue>() {
                            Some(queue) => crate::vector3d::mesh_records(queue, lines)?,
                            None => Vec::new(),
                        };
                        Ok((records, lights))
                    })?;
                // **PF Step 2（2D/3D 混排）**：同一帧里的 2D 记录不丢弃，而是作为
                // **屏幕空间 overlay** 追加进 3D 帧（E-3 已实测：OVERLAY 记录固定压在
                // 屏幕平面，z 不参与透视）。后端机制本来就有，这里只是接线。
                //
                let view_frame_scale = view.frame.0 / width as f32;
                // H3：放不进 overlay 的记录**带理由拒绝** —— 2D 记录不可能是网格
                // （`scene_records` 只产 Fill / Stroke）；若将来多出一种，必须在这里显式报错。
                records.extend(overlay_records(
                    self.two_d_records((width, height), scale)?,
                    view_frame_scale,
                )?);
                // **PG Step 2（路线 B）**：被 3D 材质采样的内容层作为**相机视图**先渲，
                // 网格记录里的 `texture` 就是这层的句柄（`handle`）⇒ 后端按 key 采样 canvas。
                // 内容层自己用矢量 2D 路径渲染（与 PC 路线 A 同一份 `content_records`），
                // 因此坐标口径（1 世界单位 = 1 层像素）完全一致。
                let mut cameras = Vec::new();
                for (index, layer) in self.content.iter().enumerate().filter(|(_, l)| !l.composite) {
                    cameras.push(zela_render_vector::CameraView::content_2d(
                        layer.handle,
                        layer.size[0],
                        layer.size[1],
                        [0.0; 4],
                        content_records(layer)
                            .map_err(|error| format!("内容层 #{index}（材质采样）：{error}"))?,
                    ));
                }
                view.lights = lights;
                // **PG Step 3**：环境图灯（`params.env` 或宿主开关 ⇒ 内置影棚）。
                // 必须排在 `view.lights` 之后：`with_sun` 会把环境图的太阳拆成**第 count 盏**
                // 方向光，槽位按 `count` 计算（见 `render.rs::with_sun`）。
                if let Some(environment) = crate::vector3d::environment_light(params) {
                    view.lights.push(environment);
                }
                Ok(Prepared::ThreeD {
                    view,
                    cameras,
                    records,
                })
            }
            None => {
                let records = self.two_d_records((width, height), scale)?;
                Ok(Prepared::TwoD {
                    background,
                    records,
                })
            }
        }
    }

    /// 本帧的 2D 记录（`DrawQueue` + reveal 队列 → 矢量记录），**屏幕空间**。
    ///
    /// 纯 2D 帧直接用；3D 帧由 [`overlay_records`] 打上 OVERLAY 位后叠加。
    /// B14：wgpu 把 reveal 排在 DrawQueue 全部几何**之后**（同一 z 的稳定序）
    /// ⇒ 这里也追加在末尾。样式宽度已是像素、路径已是屏幕空间（录制层换算过）。
    ///
    /// 输出坐标 = `(记录点 - 视口原点) × scale`：与矢量画布的像素坐标一致
    /// （GUI 的视口不在原点、HiDPI 下 scale ≠ 1；headless 两者都退化为恒等）。
    fn two_d_records(
        &self,
        canvas: (u32, u32),
        scale: f32,
    ) -> Result<Vec<zela_render_vector::PathRecord>, String> {
        // 先平移掉视口原点，再放大到画布像素（`compose` = 先 self 后 other）。
        let to_canvas = ze_notion::Transform2D::translate(ze_notion::Vec2::new(
            -self.origin[0],
            -self.origin[1],
        ))
        .compose(ze_notion::Transform2D::scale(ze_notion::Vec2::splat(scale)));
        let items: Vec<zela_render_vector::SceneItem<'_>> = self
            .draws
            .iter()
            .map(|recorded| {
                let mut style = recorded.style.clone();
                // 长度类参数是**点**，必须与几何一起换算（宽度 / 背景描边宽度）。
                style.stroke_width *= scale;
                style.paint.background_width *= scale;
                zela_render_vector::SceneItem {
                    shape: &recorded.shape,
                    transform: recorded.transform.compose(to_canvas),
                    style,
                }
            })
            .collect();
        let mut records =
            zela_render_vector::scene_records(&items, canvas).map_err(|error| error.to_string())?;
        // **阶段 2（归约）**：reveal 不再逐帧裁剪几何 —— 完整折线 + 曲线窗口，
        // 逐帧只变两个 f32（`curve_window` 的单位是曲线索引 + t）。
        // 与 wgpu 的 reveal 管线同语义（着色器丢弃区间外参数），只是换算发生在 CPU。
        for reveal in &self.reveals {
            let Some([lo, hi]) = reveal_window(&reveal.path, reveal.start, reveal.end) else {
                continue; // 空区间 / 退化路径 ⇒ 本帧没有可见段
            };
            let base = reveal_base(&reveal.path);
            let mut style = reveal.style.clone();
            style.stroke_width *= scale;
            style.paint.background_width *= scale;
            records.extend(
                zela_render_vector::window_records(&base, lo, hi, &style, to_canvas, canvas)
                    .map_err(|error| format!("reveal：{error}"))?,
            );
        }
        // **PH Step 1**：形变项排在 reveal 之后（两者同为 z = 0 的稳定序，与 wgpu 的
        // `MorphQueue` / `RevealQueue` 都在 `DrawQueue` 之后一致）。
        for (index, morph) in self.morphs.iter().enumerate() {
            let sampled = sampled_endpoints(morph);
            let (from, to) = &*sampled;
            let mut style = morph.style.clone();
            style.stroke_width *= scale;
            style.paint.background_width *= scale;
            records.extend(
                zela_render_vector::morph_records(
                    &from,
                    &to,
                    morph.t,
                    &style,
                    to_canvas,
                    canvas,
                )
                .map_err(|error| format!("形变项 #{index}：{error}"))?,
            );
        }
        // **阶段 2**：外部注入 —— 按参数表逐项向供给者取值（缺供给者 ⇒ 显式报错）。
        for record in &mut records {
            let zela_render_vector::PathRecord::Fill(fill) = record else { continue };
            let Some(paint) = fill.options.shader.as_ref() else { continue };
            let Some(shader) = fill.shader.as_mut() else { continue };
            let mut bytes = vec![0u8; paint.size as usize];
            for param in &paint.params {
                let Some(value) = self.shader_value(&param.channel) else {
                    return Err(format!("着色 codegen 缺少注入通道 {}（宿主未注册供给者）", param.channel));
                };
                value.write(&mut bytes, param.offset as usize, param.kind);
            }
            shader.params = bytes;
        }
        Ok(records)
    }

    /// 用矢量后端渲染当前帧，返回 RGBA8（native 同步读回）。
    ///
    /// 调用前必须已经 `step_frame`（或等价地推进过一帧）；
    /// 是否该用矢量由调用方按 [`Self::content`] + 路由决定（本函数不判断）。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render(
        &self,
        runtime: &Runtime,
        width: u32,
        height: u32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<Vec<u8>, String> {
        // headless / 对拍：`samples = 4`（与基线记录时一致，不能动）。
        self.render_with(runtime, width, height, 1.0, 4, params, background)
    }

    /// 同 [`Self::render`]，但显式给出**点 → 像素**系数（GUI 的 HiDPI 预览用）。
    ///
    /// `width` / `height` 是**画布像素**；2D 记录按 `scale` 从布局点换算。
    /// headless 传 `1.0`（`render` 即此），因此基线逐位不变。
    ///
    /// **预览不付 4×MSAA**：矢量后端的抗锯齿是解析式的（逐像素覆盖面积），预览用
    /// `samples = 1` 已经足够；4 倍采样只是把填充成本乘 4（实测 GUI 每帧省 ~4 ms）。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_at(
        &self,
        runtime: &Runtime,
        width: u32,
        height: u32,
        scale: f32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<Vec<u8>, String> {
        self.render_with(runtime, width, height, scale, 1, params, background)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn render_with(
        &self,
        runtime: &Runtime,
        width: u32,
        height: u32,
        scale: f32,
        samples: u32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<Vec<u8>, String> {
        let mut rgba = match self.prepare(runtime, width, height, scale, params, background)? {
            Prepared::ThreeD {
                view,
                cameras,
                records,
            } => zela_render_vector::render_3d_with(width, height, samples, &view, &cameras, &records)?,
            Prepared::TwoD {
                background,
                records,
            } => zela_render_vector::render_2d(width, height, samples, background, &records)?,
        };
        self.composite_content(width, height, &mut rgba)?;
        Ok(rgba)
    }

    /// **B15（甲）**：浏览器上的同一帧（异步回读）。
    #[cfg(target_arch = "wasm32")]
    pub async fn render_async(
        &self,
        runtime: &Runtime,
        width: u32,
        height: u32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<Vec<u8>, String> {
        let mut rgba = match self.prepare(runtime, width, height, 1.0, params, background)? {
            Prepared::ThreeD {
                view,
                cameras,
                records,
            } => {
                zela_render_vector::render_3d_with_async(width, height, 4, &view, &cameras, &records)
                    .await?
            }
            Prepared::TwoD {
                background,
                records,
            } => {
                zela_render_vector::render_2d_async(width, height, 4, background, &records).await?
            }
        };
        self.composite_content_async(width, height, &mut rgba).await?;
        Ok(rgba)
    }

    /// **B15（乙）**：把当前帧直接合成进 `target`（native 同步版），像素不经过 CPU。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_to_texture(
        &self,
        runtime: &Runtime,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<(), String> {
        self.render_to_texture_with(runtime, target, width, height, 1.0, 4, params, background)
    }

    /// 同 [`Self::render_to_texture`]，但显式给出**点 → 像素**系数与采样数。
    ///
    /// **GUI 预览用**：`scale` = 视口 ppp、`samples = 1`（与 [`Self::render_at`] 同口径，
    /// 只是输出位置从 CPU 内存换成 `target` 纹理）⇒ 像素不经 CPU 往返。
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub fn render_to_texture_at(
        &self,
        runtime: &Runtime,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
        scale: f32,
        samples: u32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<(), String> {
        self.render_to_texture_with(runtime, target, width, height, scale, samples, params, background)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    fn render_to_texture_with(
        &self,
        runtime: &Runtime,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
        scale: f32,
        samples: u32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<(), String> {
        self.refuse_zero_copy_with_content()?;
        match self.prepare(runtime, width, height, scale, params, background)? {
            Prepared::ThreeD {
                view,
                cameras,
                records,
            } => zela_render_vector::render_3d_to_texture(
                target, width, height, samples, &view, &cameras, &records,
            ),
            Prepared::TwoD {
                background,
                records,
            } => zela_render_vector::render_2d_to_texture(
                target, width, height, samples, background, &records,
            ),
        }
    }

    /// 本帧是否含 `composite = true` 的内容纹理层（叠加目前只在 CPU 像素上做）。
    pub fn has_composite_content(&self) -> bool {
        self.content.iter().any(|layer| layer.composite)
    }

    /// **B15（乙）**：同 [`Self::render_to_texture`]，浏览器上的异步版。
    #[cfg(target_arch = "wasm32")]
    pub async fn render_to_texture_async(
        &self,
        runtime: &Runtime,
        target: &wgpu::Texture,
        width: u32,
        height: u32,
        params: &zela_scene_bundle::RenderParams,
        background: [f32; 4],
    ) -> Result<(), String> {
        self.refuse_zero_copy_with_content()?;
        match self.prepare(runtime, width, height, 1.0, params, background)? {
            Prepared::ThreeD {
                view,
                cameras,
                records,
            } => {
                zela_render_vector::render_3d_to_texture_async(
                    target, width, height, 4, &view, &cameras, &records,
                )
                .await
            }
            Prepared::TwoD {
                background,
                records,
            } => {
                zela_render_vector::render_2d_to_texture_async(
                    target, width, height, 4, background, &records,
                )
                .await
            }
        }
    }

    /// 零拷贝路径（渲进调用方纹理）**不能**叠加内容层：叠加是在 CPU 像素上做的。
    ///
    /// 显式报错而不是"只画视口层"——静默少画一整层正是 PC §6.2 禁止的失败模式。
    fn refuse_zero_copy_with_content(&self) -> Result<(), String> {
        if self.content.iter().any(|layer| layer.composite) {
            return Err(
                "本帧含内容纹理图层：叠加走 CPU 路径（render / render_async），\
                 零拷贝的 render_to_texture 不支持（GAPS PC-01）"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// 把 `composite = true` 的内容层按声明顺序叠到视口像素上（native）。
    #[cfg(not(target_arch = "wasm32"))]
    fn composite_content(
        &self,
        width: u32,
        height: u32,
        rgba: &mut [u8],
    ) -> Result<(), String> {
        for (index, layer) in self.content.iter().enumerate().filter(|(_, l)| l.composite) {
            let pixels = content_raster::render(layer, index)?;
            composite_layer(rgba, width, height, pixels.as_slice(), layer)
                .map_err(|error| format!("内容层 #{index}：{error}"))?;
        }
        Ok(())
    }

    /// 同上（浏览器：内容层也用异步回读）。
    #[cfg(target_arch = "wasm32")]
    async fn composite_content_async(
        &self,
        width: u32,
        height: u32,
        rgba: &mut [u8],
    ) -> Result<(), String> {
        for (index, layer) in self.content.iter().enumerate().filter(|(_, l)| l.composite) {
            let records = content_records(layer)?;
            let pixels = zela_render_vector::render_2d_async(
                layer.size[0],
                layer.size[1],
                1,
                [0.0; 4],
                &records,
            )
            .await
            .map_err(|error| format!("内容层 #{index} 渲染失败：{error}"))?;
            composite_layer(rgba, width, height, &pixels, layer)
                .map_err(|error| format!("内容层 #{index}：{error}"))?;
        }
        Ok(())
    }
}

/// 逐个字段比（`Recorded2D` 只派生了 `Clone`，没有 `PartialEq`）。
fn same_draws(a: &[zela_render_backends::Recorded2D], b: &[zela_render_backends::Recorded2D]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(left, right)| {
            left.shape == right.shape
                && left.transform == right.transform
                && left.style == right.style
                && left.z == right.z
        })
}

/// **内容层光栅缓存**（native）：同一份 draw 序列 ⇒ 直接复用上一帧的像素。
///
/// 为什么需要：Surface 的内容（Typst 字形轮廓，动辄上万点）在矢量后端是**每帧**重新
/// 映射 + 三角化的 —— 1024×768 的文字层实测 ~50 ms/帧。GUI 预览每帧都在渲同一张
/// 静止内容时这是纯浪费。判据用 draw 序列的**逐项相等**（`PartialEq`，不做哈希近似）：
/// 内容真变了（动画公式 / 换作品）就一定重渲，不会贴旧图。
#[cfg(not(target_arch = "wasm32"))]
mod content_raster {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use zela_render_backends::Recorded2D;

    use super::{ContentSnapshot, content_records, same_draws};

    struct Entry {
        size: [u32; 2],
        draws: Vec<Recorded2D>,
        rgba: Arc<Vec<u8>>,
    }

    static CACHE: OnceLock<Mutex<HashMap<u64, Entry>>> = OnceLock::new();

    pub(super) fn render(
        layer: &ContentSnapshot,
        index: usize,
    ) -> Result<Arc<Vec<u8>>, String> {
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = cache
            .lock()
            .map_err(|_| "内容层缓存锁中毒".to_string())?;
        if let Some(entry) = guard.get(&layer.handle) {
            if entry.size == layer.size && same_draws(&entry.draws, &layer.draws) {
                return Ok(Arc::clone(&entry.rgba));
            }
        }

        let records = content_records(layer)?;
        let rgba = zela_render_vector::render_2d(
            layer.size[0],
            layer.size[1],
            1,
            [0.0; 4],
            &records,
        )
        .map_err(|error| format!("内容层 #{index} 渲染失败：{error}"))?;
        let rgba = Arc::new(rgba);

        // 多作品切换 / 多尺寸时不无限留垃圾（每层 ~3 MB）。
        if guard.len() > 8 {
            guard.clear();
        }
        guard.insert(
            layer.handle,
            Entry {
                size: layer.size,
                draws: layer.draws.clone(),
                rgba: Arc::clone(&rgba),
            },
        );
        Ok(rgba)
    }
}

/// 内容层的一份 draw 列表 → 矢量记录（尺寸 = 该层自己的纹理尺寸，与 wgpu 侧
/// `begin_frame_at` 的 geometry 同口径：视口 == 纹理、ppp = 1）。
fn content_records(layer: &ContentSnapshot) -> Result<Vec<zela_render_vector::PathRecord>, String> {
    let items: Vec<zela_render_vector::SceneItem<'_>> = layer
        .draws
        .iter()
        .map(|recorded| zela_render_vector::SceneItem {
            shape: &recorded.shape,
            transform: recorded.transform,
            style: recorded.style.clone(),
        })
        .collect();
    zela_render_vector::scene_records(&items, (layer.size[0], layer.size[1]))
        .map_err(|error| error.to_string())
}

/// 把一张内容纹理按 **contain + 居中**（与 `compositor::fit_rect` 同口径）叠到 `dst`。
///
/// 混合口径抄 `compositor::blend_state`：`Alpha` 是**预乘**（`One / OneMinusSrcAlpha`），
/// `Opaque` 是 REPLACE，`Additive` 是 `One / One`；`opacity` 乘在源上。
fn composite_layer(
    dst: &mut [u8],
    width: u32,
    height: u32,
    src: &[u8],
    layer: &ContentSnapshot,
) -> Result<(), String> {
    let [sw, sh] = layer.size;
    let needed = sw as usize * sh as usize * 4;
    if sw == 0 || sh == 0 || src.len() < needed {
        return Err(format!(
            "内容纹理尺寸非法：{sw}×{sh}（像素 {} 字节，至少需要 {needed}）",
            src.len()
        ));
    }
    let scale = (width as f32 / sw as f32).min(height as f32 / sh as f32);
    if !(scale > 0.0) || !scale.is_finite() {
        return Err(format!("内容纹理无法适配到 {width}×{height}"));
    }
    let (dw, dh) = (sw as f32 * scale, sh as f32 * scale);
    let (left, top) = ((width as f32 - dw) * 0.5, (height as f32 - dh) * 0.5);
    let opacity = layer.opacity.clamp(0.0, 1.0);

    // **采样表**：源索引与权重只依赖 x（或 y），逐像素再算 `floor` / 钳位是纯重复。
    // 1M 像素的小图叠加里这一项就是数十毫秒（GUI 每帧都走这条路）。
    // 一侧的 `(i0, i1, t)`；`None` = 该行列不在内容矩形内。
    let axis = |count: u32, size: f32, offset: f32, extent: f32, limit: u32| -> Vec<Option<(usize, usize, f32)>> {
        (0..count)
            .map(|i| {
                let f = (i as f32 + 0.5 - offset) / extent;
                if !(0.0..1.0).contains(&f) {
                    return None;
                }
                let at = f * size - 0.5;
                let i0 = at.floor();
                let t = at - i0;
                let clamp = |v: f32| (v as i32).clamp(0, limit as i32 - 1) as usize;
                Some((clamp(i0), clamp(i0 + 1.0), t))
            })
            .collect()
    };
    let columns = axis(width, sw as f32, left, dw, sw);
    let rows = axis(height, sh as f32, top, dh, sh);

    // `Opaque` 是 REPLACE ⇒ 透明像素也必须写（清成 0），不能早退；另两种混合 0 贡献 = 恒等。
    let can_skip_clear = layer.blend != ze_render_ir::LayerBlend::Opaque;
    let src_w = sw as usize;
    // 每源行是否**整行全零**（文字层的大部分行都是）⇒ 对应目标行可以整行跳过。
    let zero_rows: Vec<bool> = if can_skip_clear {
        (0..sh as usize)
            .map(|y| {
                let start = y * src_w * 4;
                src[start..start + src_w * 4].iter().all(|&byte| byte == 0)
            })
            .collect()
    } else {
        Vec::new()
    };
    for y in 0..height as usize {
        let Some((y0, y1, ty)) = rows[y] else {
            continue;
        };
        if can_skip_clear && zero_rows[y0] && zero_rows[y1] {
            continue;
        }
        for x in 0..width as usize {
            let Some((x0, x1, tx)) = columns[x] else {
                continue;
            };
            // **全零早退**：四邻整像素都是 0（RGBA 全 0）⇒ 双线性结果必为 0，
            // 非 REPLACE 混合下这一步是"什么都不写"（逐位等价）。字形/图形之外的像素
            // 占九成以上，省掉的是每像素的双线性 + 4 次写回。
            let pixel = |sy: usize, sx: usize| -> u32 {
                let i = (sy * src_w + sx) * 4;
                u32::from_le_bytes([src[i], src[i + 1], src[i + 2], src[i + 3]])
            };
            if can_skip_clear
                && (pixel(y0, x0) | pixel(y0, x1) | pixel(y1, x0) | pixel(y1, x1)) == 0
            {
                continue;
            }
            let row = |sy: usize| -> [f32; 4] {
                let a = (sy * sw as usize + x0) * 4;
                let b = (sy * sw as usize + x1) * 4;
                std::array::from_fn(|k| {
                    let left = src[a + k] as f32 / 255.0;
                    let right = src[b + k] as f32 / 255.0;
                    left + (right - left) * tx
                })
            };
            let [r0, g0, b0, a0] = row(y0);
            let [r1, g1, b1, a1] = row(y1);
            let mix = |a: f32, b: f32| a + (b - a) * ty;
            let (r, g, b, a) = (mix(r0, r1), mix(g0, g1), mix(b0, b1), mix(a0, a1));

            let index = (y * width as usize + x) * 4;
            let byte = |value: f32| (value * 255.0).round().clamp(0.0, 255.0) as u8;
            match layer.blend {
                ze_render_ir::LayerBlend::Alpha => {
                    let alpha = a * opacity;
                    let keep = 1.0 - alpha;
                    dst[index] = byte(r * opacity + dst[index] as f32 / 255.0 * keep);
                    dst[index + 1] = byte(g * opacity + dst[index + 1] as f32 / 255.0 * keep);
                    dst[index + 2] = byte(b * opacity + dst[index + 2] as f32 / 255.0 * keep);
                    dst[index + 3] = byte(alpha + dst[index + 3] as f32 / 255.0 * keep);
                }
                ze_render_ir::LayerBlend::Opaque => {
                    dst[index] = byte(r * opacity);
                    dst[index + 1] = byte(g * opacity);
                    dst[index + 2] = byte(b * opacity);
                    dst[index + 3] = byte(opacity);
                }
                ze_render_ir::LayerBlend::Additive => {
                    dst[index] = byte(r * opacity + dst[index] as f32 / 255.0);
                    dst[index + 1] = byte(g * opacity + dst[index + 1] as f32 / 255.0);
                    dst[index + 2] = byte(b * opacity + dst[index + 2] as f32 / 255.0);
                    dst[index + 3] = byte(a * opacity + dst[index + 3] as f32 / 255.0);
                }
            }
        }
    }
    Ok(())
}

/// 给一批 2D 记录打上 **OVERLAY** 位（屏幕平面），供 3D 帧叠加。
///
/// `scale = frame_w / width`：overlay 矩阵的坐标系是**3D 视图的 frame 单位**
/// （半幅 = frame/2），而 2D 记录是**屏幕像素偏移**（`scene_records` 只减画布中心 + 翻 y）。
/// 几何、描边宽度、渐变轴端点、背景宽度都是同一空间量 ⇒ 一起乘。
///
/// H3：**放不进 overlay 的记录带理由拒绝**。`scene_records` 目前只产 Fill / Stroke；
/// 若将来新增记录种类（点云 / 纹理），必须在这里决定它如何进 3D 帧 —— 不允许静默丢弃。
fn overlay_records(
    records: Vec<zela_render_vector::PathRecord>,
    scale: f32,
) -> Result<Vec<zela_render_vector::PathRecord>, String> {
    let scale_path = |points: &mut Vec<[f64; 3]>| {
        for point in points.iter_mut() {
            point[0] *= scale as f64;
            point[1] *= scale as f64;
        }
    };
    let scale_options = |options: &mut ze_render_ir::PathOptions| {
        if let Some(gradient) = &mut options.gradient {
            gradient.from[0] *= scale;
            gradient.from[1] *= scale;
            gradient.to[0] *= scale;
            gradient.to[1] *= scale;
        }
        options.background_width *= scale;
    };
    // **阶段 1**：形变 / 窗口的逐帧参数与首形状同一坐标系 ⇒ 差值几何一起缩放进 overlay。
    let scale_param = |param: &mut Option<zela_render_vector::ParamGeometry>| {
        if let Some(param) = param {
            scale_path(&mut param.delta);
        }
    };
    records
        .into_iter()
        .map(|record| match record {
            zela_render_vector::PathRecord::Fill(mut path) => {
                path.options.overlay = true;
                scale_path(&mut path.points);
                scale_param(&mut path.param);
                scale_options(&mut path.options);
                Ok(zela_render_vector::PathRecord::Fill(path))
            }
            zela_render_vector::PathRecord::Stroke(mut path) => {
                path.options.overlay = true;
                scale_path(&mut path.points);
                scale_param(&mut path.param);
                path.width *= scale;
                scale_options(&mut path.options);
                Ok(zela_render_vector::PathRecord::Stroke(path))
            }
            // PG Step 4：点云也是屏幕空间图元（点 + 半径一起缩放到 overlay 坐标系）。
            zela_render_vector::PathRecord::Points(mut cloud) => {
                scale_path(&mut cloud.points);
                cloud.radius *= scale;
                Ok(zela_render_vector::PathRecord::Points(cloud))
            }
            zela_render_vector::PathRecord::Mesh(_) => Err(
                "2D 记录里出现网格：网格不能作为屏幕平面 overlay（H3 拒绝，不静默丢弃）"
                    .to_string(),
            ),
        })
        .collect()
}

/// **阶段 2**：reveal 的几何基座 —— 完整折线（**只由路径决定，与 `[start, end]` 无关**）。
///
/// 与裁剪版的区别：这里**不截断**，可见部分交给 `curve_window` 在着色器里丢弃。
/// 同一份路径逐帧产出逐字节相同的几何（每段一条直线三次曲线）。
fn reveal_base(path: &Path2D) -> Path2D {
    Path2D::from_polyline(&path.flatten(0.25), false)
}

/// **阶段 2**：把 reveal 的归一化弧长 `[start, end]` 换算成**曲线窗口**（曲线索引 + t）。
///
/// 口径与后端 `params_of` 一致：把展开后的折线按弦长参数化，第 `i` 段占
/// `[i, i+1)`；区间端点落在段内就带小数。`None` = 本帧没有可见段。
fn reveal_window(path: &Path2D, start: f32, end: f32) -> Option<[f32; 2]> {
    let (start, end) = (start.clamp(0.0, 1.0), end.clamp(0.0, 1.0));
    if end <= start {
        return None;
    }
    let flat = path.flatten(0.25);
    if flat.len() < 2 {
        return None;
    }
    let mut cumulative = Vec::with_capacity(flat.len());
    let mut total = 0.0f32;
    cumulative.push(0.0);
    for pair in flat.windows(2) {
        total += (pair[1] - pair[0]).length();
        cumulative.push(total);
    }
    if total <= f32::EPSILON {
        return None;
    }
    let locate = |fraction: f32| -> f32 {
        let target = fraction * total;
        for i in 0..flat.len() - 1 {
            if cumulative[i + 1] >= target {
                let span = cumulative[i + 1] - cumulative[i];
                let t = if span > f32::EPSILON {
                    (target - cumulative[i]) / span
                } else {
                    0.0
                };
                return i as f32 + t.clamp(0.0, 1.0);
            }
        }
        (flat.len() - 1) as f32
    };
    let lo = locate(start);
    let hi = locate(end);
    (hi > lo).then_some([lo, hi])
}

/// **A/B 参考实现（仅测试）**：旧口径 —— 在 CPU 上按弧长裁成开放折线。
///
/// 保留它是为了 `reveal_window_matches_the_cpu_clip` 的像素对拍：新口径（窗口）与
/// 旧口径（裁剪）必须在容差内一致。生产路径不再调用它。
#[cfg(test)]
fn clip_reveal(path: &Path2D, start: f32, end: f32) -> Option<ze_render_ir::Shape2D> {
    let (start, end) = (start.clamp(0.0, 1.0), end.clamp(0.0, 1.0));
    if end <= start {
        return None;
    }
    let flat = path.flatten(0.25);
    if flat.len() < 2 {
        return None;
    }
    let mut cumulative = Vec::with_capacity(flat.len());
    let mut total = 0.0f32;
    cumulative.push(0.0);
    for pair in flat.windows(2) {
        total += (pair[1] - pair[0]).length();
        cumulative.push(total);
    }
    if total <= f32::EPSILON {
        return None;
    }
    let mut out: Vec<Vec2> = Vec::new();
    for i in 0..flat.len() - 1 {
        let (s0, s1) = (cumulative[i] / total, cumulative[i + 1] / total);
        if s1 <= start || s0 >= end {
            continue;
        }
        let span = (s1 - s0).max(f32::EPSILON);
        let t0 = ((start - s0) / span).clamp(0.0, 1.0);
        let t1 = ((end - s0) / span).clamp(0.0, 1.0);
        if out.is_empty() {
            out.push(flat[i] + (flat[i + 1] - flat[i]) * t0);
        }
        out.push(flat[i] + (flat[i + 1] - flat[i]) * t1);
    }
    if out.len() < 2 {
        return None;
    }
    Some(ze_render_ir::Shape2D::Path(Path2D::from_polyline(&out, false)))
}

/// **PH Step 1**：形变两端按弧长等点重采样，**按 `(from, to, samples)` 缓存**。
///
/// 为什么必须缓存：`delta = to − from` 只依赖两端与 `samples`、**与 `t` 无关**
/// ⇒ 同一条形变在动画期间只重采样一次，每帧只改记录的 `m2` 权重。
///
/// ⚠️ 缓存键**必须含 `samples` 与两端路径标识**：只比"有没有"不比"是哪一个"就是
/// GAPS PC-05（`Player` 缓存键不含 shadows）的同型 —— 换了 `samples` 或端点却命中旧几何，
/// 画面会停在上一段动画上。
fn sampled_endpoints(morph: &zela_render::draw::MorphItem) -> std::sync::Arc<(Path2D, Path2D)> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    static CACHE: OnceLock<Mutex<HashMap<(u64, u64, u32), Arc<(Path2D, Path2D)>>>> = OnceLock::new();
    let key = (path_digest(&morph.from), path_digest(&morph.to), morph.samples);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hit) = guard.get(&key) {
        return Arc::clone(hit);
    }
    // 作品切换 / 大量一次性形变时不无限留垃圾（每项 2 × samples 个点）。
    if guard.len() >= 64 {
        guard.clear();
    }
    let sampled = Arc::new(morph.sampled_endpoints());
    guard.insert(key, Arc::clone(&sampled));
    sampled
}

/// 路径内容摘要（`Path2D` 没有 `Hash`；这里只用于缓存键，不做跨进程稳定性承诺）。
fn path_digest(path: &Path2D) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.commands.len().hash(&mut hasher);
    let point = |p: Vec2, hasher: &mut std::collections::hash_map::DefaultHasher| {
        p.x.to_bits().hash(hasher);
        p.y.to_bits().hash(hasher);
    };
    for command in &path.commands {
        match command {
            ze_notion::PathCommand::MoveTo(p) => {
                0u8.hash(&mut hasher);
                point(*p, &mut hasher);
            }
            ze_notion::PathCommand::LineTo(p) => {
                1u8.hash(&mut hasher);
                point(*p, &mut hasher);
            }
            ze_notion::PathCommand::QuadTo { ctrl, end } => {
                2u8.hash(&mut hasher);
                point(*ctrl, &mut hasher);
                point(*end, &mut hasher);
            }
            ze_notion::PathCommand::CubicTo { ctrl1, ctrl2, end } => {
                3u8.hash(&mut hasher);
                point(*ctrl1, &mut hasher);
                point(*ctrl2, &mut hasher);
                point(*end, &mut hasher);
            }
            ze_notion::PathCommand::Arc {
                center,
                radii,
                rotation,
                start_angle,
                end_angle,
            } => {
                4u8.hash(&mut hasher);
                point(*center, &mut hasher);
                point(*radii, &mut hasher);
                rotation.to_bits().hash(&mut hasher);
                start_angle.to_bits().hash(&mut hasher);
                end_angle.to_bits().hash(&mut hasher);
            }
            ze_notion::PathCommand::Close => 5u8.hash(&mut hasher),
        }
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last_x(shape: &ze_render_ir::Shape2D) -> f32 {
        let ze_render_ir::Shape2D::Path(path) = shape else {
            panic!("reveal 裁剪结果应是 Path");
        };
        path.flatten(0.25).last().expect("至少一个点").x
    }

    /// 半程 reveal 的终点应落在路径中点（按弧长参数化，不是按顶点序号）。
    #[test]
    fn reveal_clip_keeps_the_parameterized_span() {
        let path = Path2D::from_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)], false);
        let full = clip_reveal(&path, 0.0, 1.0).expect("整段可见");
        assert!((last_x(&full) - 10.0).abs() < 1e-3, "整段终点应为 10");
        let half = clip_reveal(&path, 0.0, 0.5).expect("半段可见");
        assert!((last_x(&half) - 5.0).abs() < 1e-3, "半段终点应为 5，实测 {half:?}");
        // 区间倒置 / 空区间 ⇒ 不可见（不是"整段"）。
        assert!(clip_reveal(&path, 0.5, 0.5).is_none());
        assert!(clip_reveal(&path, 0.8, 0.2).is_none());
    }

    /// **阶段 2**：归一化弧长 → 曲线索引 + t（窗口口径）。
    #[test]
    fn reveal_window_maps_arc_length_to_curve_index() {
        let path = Path2D::from_polyline(
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(5.0, 0.0),
                Vec2::new(10.0, 0.0),
            ],
            false,
        );
        assert_eq!(reveal_window(&path, 0.0, 1.0), Some([0.0, 2.0]));
        let half = reveal_window(&path, 0.0, 0.5).expect("半段可见");
        assert!((half[1] - 1.0).abs() < 1e-3, "半程应落在第 1 段末尾，实测 {half:?}");
        assert!(reveal_window(&path, 0.5, 0.5).is_none());
        assert!(reveal_window(&path, 0.8, 0.2).is_none());
    }

    /// **E2 对拍（阶段 2）**：新口径（完整折线 + 窗口）与旧口径（CPU 裁剪）
    /// 必须在**容差内**一致 —— 判据是像素，不是"没有报错"。
    ///
    /// 允许差异来自两条路径的浮点运算顺序不同（着色器在 `t` 处求值 vs CPU 插值）；
    /// 视觉上不可见的 1–2 级通道差不算不一致。
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reveal_window_matches_the_cpu_clip() {
        use ze_notion::{Color32, Transform2D};
        use ze_render_ir::Style2D;

        let path = Path2D::from_polyline(
            &[
                Vec2::new(8.0, 32.0),
                Vec2::new(20.0, 18.0),
                Vec2::new(32.0, 46.0),
                Vec2::new(44.0, 22.0),
                Vec2::new(56.0, 34.0),
            ],
            false,
        );
        let style = Style2D {
            fill: None,
            stroke: Some(Color32::WHITE),
            stroke_width: 3.0,
            ..Default::default()
        };
        let canvas = (64u32, 64u32);
        let base = reveal_base(&path);
        let [lo, hi] = reveal_window(&path, 0.25, 0.75).expect("区间有效");
        let via_window = zela_render_vector::window_records(
            &base,
            lo,
            hi,
            &style,
            Transform2D::identity(),
            canvas,
        )
        .expect("窗口记录");
        let shape = clip_reveal(&path, 0.25, 0.75).expect("裁剪记录");
        let via_clip = zela_render_vector::scene_records(
            &[zela_render_vector::SceneItem {
                shape: &shape,
                transform: Transform2D::identity(),
                style: style.clone(),
            }],
            canvas,
        )
        .expect("裁剪映射");

        let clear = [0.0, 0.0, 0.0, 1.0];
        let a = zela_render_vector::render_2d(64, 64, 1, clear, &via_window).expect("窗口渲染");
        let b = zela_render_vector::render_2d(64, 64, 1, clear, &via_clip).expect("裁剪渲染");
        let mut max_diff = 0u8;
        let mut changed = 0usize;
        for (x, y) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            let diff = x.iter().zip(y).map(|(p, q)| p.abs_diff(*q)).max().unwrap_or(0);
            max_diff = max_diff.max(diff);
            changed += usize::from(diff > 0);
        }
        assert!(max_diff <= 4, "窗口与裁剪的最大通道差 {max_diff} 超出容差");
        assert!(
            changed < 200,
            "差异像素 {changed} 过多（应只在抗锯齿边缘的 1–2 级差异）"
        );
    }
}
