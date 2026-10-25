// routing.rs — 后端路由（B11 Step 4）。
//
// # 为什么在 backends（而不是宿主）
//
// 「用哪个后端」是后端的装配问题，不是某个宿主的问题：native `zela-app` 与浏览器
// `zela-web` 都必须用**同一份**决策（否则两边会漂成两套规则）。
//
// # 硬约束
//
// - C1 动态 ≠ 不确定：输入只有帧内容与 adapter 事实，没有负载 / 墙钟 / 随机数；
// - C4 不支持即报错，不得静默选一个：顺序回退时**带理由**，全部被拒则报错。

use crate::registry::{AdapterFacts, BackendKind};

/// 出帧后端 / 路由偏好（`zela-render --backend` 同义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopBackend {
    /// 按能力自动选，作品级（第一帧决定，全程一致）。
    Auto,
    /// 自动选，但每帧重算（同一作品内可能换画风，留作诊断开关）。
    AutoFrame,
    /// 显式 wgpu（现状基线路径；不做回退）。
    Wgpu,
    /// **偏好**矢量；矢量拒绝该帧时回退 wgpu，并打印理由（B11 §6.3）。
    Vector,
}

impl LoopBackend {
    /// 具体后端（`Auto*` 未决前为 `None`）。
    pub fn concrete(self) -> Option<BackendKind> {
        match self {
            LoopBackend::Auto | LoopBackend::AutoFrame => None,
            LoopBackend::Wgpu => Some(BackendKind::Wgpu),
            LoopBackend::Vector => Some(BackendKind::Vector),
        }
    }

    /// 解析 `--backend` 的取值。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(LoopBackend::Auto),
            "auto-frame" => Some(LoopBackend::AutoFrame),
            "wgpu" => Some(LoopBackend::Wgpu),
            "vector" => Some(LoopBackend::Vector),
            _ => None,
        }
    }
}

/// 一帧"画什么"的特征（**只描述内容**，不含任何后端偏好）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameContent {
    pub has_3d_camera: bool,
    /// 2D 图元条数（`DrawQueue` 里的 2D 记录）。
    pub has_2d_primitives: usize,
    pub has_meshes: usize,
    /// 网格里是否含非三角形（线框）。
    pub has_line_meshes: bool,
    /// 是否含内容纹理 / Surface 图层。
    pub has_content_texture: bool,
    /// 是否有**被 3D 材质采样**的内容层（`composite = false`，PG Step 2 的路线 B）。
    pub has_material_content: bool,
    /// 网格材质引用的纹理里，是否有**没有生产方**的（不是本帧的内容层）。
    ///
    /// **PG Step 2（路线 B）**：`composite = false` 的内容层由矢量侧当相机视图先渲、
    /// 网格按句柄采样 ⇒ 不再因"内容纹理被材质采样"整帧拒绝。仍然拒绝的只剩
    /// "纹理不是本帧内容层"（图像资产上传入口还没开，见 Step 4）。
    pub has_unserved_material_texture: bool,
    /// 是否含形变（Morph）项（B14）：矢量后端尚未映射 ⇒ 显式拒绝，不静默丢。
    pub has_morphs: bool,
    /// 本帧用到的**仅矢量支持**的绘制参数（位掩码，见 `ze_render_ir::PathOptions`）。
    ///
    /// PE Step 3：wgpu 后端不消费这些字段（渐变 / 虚线 / 双 paint / 背景描边 / 曲线窗口）。
    /// 选到 wgpu 时**必须**在决策理由里写明"已忽略"，不得静默丢效果（B12 同口径）。
    pub vector_only_styles: u8,
}

impl FrameContent {
    /// 选择了 wgpu 时的显式说明（没有用到仅矢量特性 ⇒ `None`）。
    fn ignored_style_note(&self) -> Option<String> {
        if self.vector_only_styles == 0 {
            return None;
        }
        Some(format!(
            "wgpu 忽略本帧仅矢量支持的样式：{}",
            ze_render_ir::PathOptions::feature_names(self.vector_only_styles)
        ))
    }
}

/// 路由决定（可观测面；下一批"可观测性专题"接成结构化输出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingDecision {
    pub backend: BackendKind,
    /// 为什么选它（人可读）。
    pub reason: String,
    /// 谁被拒、为什么（按尝试顺序；被选中的不在其中）。
    pub rejected: Vec<(BackendKind, String)>,
}

impl RoutingDecision {
    /// 显式 `--backend wgpu` 的决策。
    ///
    /// 该路径**不取帧快照**（省一次构图），因此没有内容事实可路由；但决策仍要记录一条，
    /// 否则默认门禁路径在 B13 的决策日志里会缺条目（看起来像"没做决策"）。
    pub fn explicit_wgpu() -> Self {
        Self {
            backend: BackendKind::Wgpu,
            reason: "显式指定 wgpu：不取帧快照、不做回退".to_string(),
            rejected: Vec::new(),
        }
    }

    /// 一行可读报告（B11 §6.4：选了谁、为什么、谁被拒、拒绝理由）。
    pub fn report(&self) -> String {
        let mut out = format!("选择 {:?}（{}）", self.backend, self.reason);
        for (kind, why) in &self.rejected {
            out.push_str(&format!("；{kind:?} 被拒：{why}"));
        }
        out
    }

    /// 结构化一行（B12 Step 2）：可 diff、可归档，B13 对拍时作为"第一现场"。
    ///
    /// 只含决策本身的事实，不含时间戳 / 运行次数 —— 同一输入必须产出同一行（C1）。
    pub fn json(&self, work: &str, pass: &str) -> String {
        format!("[decision] {}", self.object_json(work, pass))
    }

    /// 决策对象本体（B12 Step 3：结构化报告要把决策嵌进同一个 JSON 对象）。
    pub fn object_json(&self, work: &str, pass: &str) -> String {
        let rejected: Vec<String> = self
            .rejected
            .iter()
            .map(|(kind, why)| {
                format!(
                    "{{\"backend\":\"{kind:?}\",\"reason\":\"{}\"}}",
                    escape_json(why)
                )
            })
            .collect();
        format!(
            "{{\"work\":\"{}\",\"pass\":\"{}\",\"backend\":\"{:?}\",\
             \"reason\":\"{}\",\"rejected\":[{}]}}",
            escape_json(work),
            escape_json(pass),
            self.backend,
            escape_json(&self.reason),
            rejected.join(",")
        )
    }

    /// C4：拒绝理由不得为空（空理由 = 静默决策），且"被选中的后端不得同时出现在拒绝里"。
    pub fn validate(&self) -> Result<(), String> {
        if self.reason.trim().is_empty() {
            return Err(format!(
                "{:?} 的选择理由为空 ⇒ 静默决策（B12 Step 2 / C4）",
                self.backend
            ));
        }
        for (kind, why) in &self.rejected {
            if why.trim().is_empty() {
                return Err(format!(
                    "{kind:?} 的拒绝理由为空 ⇒ 静默决策（B12 Step 2 / C4）"
                ));
            }
            if *kind == self.backend {
                return Err(format!("{kind:?} 既被选中又被拒 ⇒ 决策自相矛盾"));
            }
        }
        Ok(())
    }
}

/// JSON 字符串转义（拒绝理由里有中文与 `⇒`，不能假设安全字符集）。
fn escape_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 按偏好得到尝试顺序。
fn order_of(preference: LoopBackend) -> &'static [BackendKind] {
    match preference {
        LoopBackend::Wgpu => &[BackendKind::Wgpu],
        // 偏好矢量：先矢量，被拒才回退（回退**打印理由**，不是静默）。
        LoopBackend::Vector => &[BackendKind::Vector, BackendKind::Wgpu],
        // B19 Step 3：自动默认矢量 —— B10~B18 建的整套东西（解析 AA、着色栈、资源常驻）
        // 终于走默认路径；被拒（内容纹理 / 混排 / 线框 / morph / 能力不足）才回退冻结的 wgpu，
        // 且回退**必带理由**。基线不受影响：`cargo verify --step hashes` 用显式 (作品, 后端)
        // 对表，根本不经过这里。
        LoopBackend::Auto | LoopBackend::AutoFrame => &[BackendKind::Vector, BackendKind::Wgpu],
    }
}

/// 某后端拒绝该帧的理由（能画 ⇒ `None`）。
fn reject_reason(
    kind: BackendKind,
    content: FrameContent,
    facts: Option<AdapterFacts>,
) -> Option<String> {
    match kind {
        // wgpu 三角化不依赖 adapter feature；内容纹理 / 混排 / 网格形态都由它自己的
        // 后端实现覆盖（那是今天的默认路径）。
        BackendKind::Wgpu => None,
        BackendKind::Vector => {
            // 1) adapter 硬前置（B11 Step 0）。
            match facts {
                Some(facts) => {
                    if let Some(why) = kind.unavailable_reason_for(facts) {
                        return Some(why);
                    }
                }
                None => {
                    return Some(
                        "上下文没有 adapter ⇒ 无法确认 FLOAT32_BLENDABLE / DEPTH32FLOAT_STENCIL8（C4 不允许假设可用）"
                            .to_string(),
                    );
                }
            }
            // 2) 内容能力（与 frame_loop 里原先"硬报错"的判据同一组事实）。
            // PC（路线 A）解了 `composite = true` 的图层级叠加；PG Step 2（路线 B）解了
            // `composite = false` 的材质采样（内容层当相机视图先渲）。仍画不了的只有
            // "材质的纹理不是本帧内容层" ⇒ 显式拒绝。
            if content.has_unserved_material_texture {
                return Some(
                    "本帧有网格材质的纹理不是内容层（没有生产方）⇒ 矢量侧缺少该纹理的\
                     上传入口（GAPS PG Step 4：纹理资产）"
                        .to_string(),
                );
            }
            // **PF Step 2**：2D 与 3D 混排不再是拒绝理由 —— 2D 记录以 OVERLAY 位
            // （屏幕平面）叠加进 3D 帧，后端机制早已具备（E-3 实测）。见 GAPS X1-06。
            if content.has_3d_camera && content.has_meshes == 0 {
                return Some("本帧有 3D 相机但没有任何网格".to_string());
            }
            if content.has_line_meshes {
                return Some("网格含线框（矢量后端只接受三角形面，MeshForm::Tri）".to_string());
            }
            // B14：reveal 已映射（按弧长裁剪），morph 还没有 ⇒ 显式拒绝，不静默丢画面。
            if content.has_morphs {
                return Some(
                    "本帧含形变（Morph）项，矢量后端尚未映射（B14 记账）".to_string(),
                );
            }
            None
        }
    }
}

/// 按给定顺序路由（测试可传入自定义顺序，生产用 [`route`]）。
pub fn route_with_order(
    order: &[BackendKind],
    preference: LoopBackend,
    content: FrameContent,
    facts: Option<AdapterFacts>,
) -> Result<RoutingDecision, String> {
    let mut rejected: Vec<(BackendKind, String)> = Vec::new();
    for kind in order {
        match reject_reason(*kind, content, facts) {
            None => {
                let reason = if rejected.is_empty() {
                    match preference {
                        LoopBackend::Wgpu => "显式指定 wgpu".to_string(),
                        LoopBackend::Vector => "显式指定矢量".to_string(),
                        LoopBackend::Auto | LoopBackend::AutoFrame => {
                            format!("自动路由：{:?} 能力覆盖本帧", kind)
                        }
                    }
                } else {
                    let (who, _) = &rejected[0];
                    format!("首选 {who:?} 被拒 ⇒ 回退到 {kind:?}")
                };
                // PC（S1-5）：后端组合方式必须可观测 —— 含内容层时不是"纯矢量"。
                // PG Step 2：材质采样的层叫"采样"，不是"叠加"（两者语义不同，不能混写）。
                let reason = if *kind == BackendKind::Vector && content.has_material_content {
                    format!("{reason}（图层级：内容层由矢量渲染、3D 材质按纹理采样）")
                } else if *kind == BackendKind::Vector && content.has_content_texture {
                    format!("{reason}（图层级：内容层由矢量渲染后叠加）")
                } else {
                    reason
                };
                // PF Step 2：2D/3D 混排同样是"图层级组合"，不是纯 3D。
                let reason = if *kind == BackendKind::Vector
                    && content.has_3d_camera
                    && content.has_2d_primitives > 0
                {
                    format!(
                        "{reason}（图层级：{} 条 2D 记录以屏幕平面叠加进 3D 帧）",
                        content.has_2d_primitives
                    )
                } else {
                    reason
                };
                // PE Step 3：wgpu 不消费的绘制参数必须**显式**说明被忽略，
                // 否则就是"填了没效果也不说"（V-01 / V-02 的同型）。
                let reason = match (*kind, content.ignored_style_note()) {
                    (BackendKind::Wgpu, Some(note)) => format!("{reason}（{note}）"),
                    _ => reason,
                };
                return Ok(RoutingDecision {
                    backend: *kind,
                    reason,
                    rejected,
                });
            }
            Some(reason) => rejected.push((*kind, reason)),
        }
    }
    Err(format!(
        "没有后端能画这一帧：{}",
        rejected
            .iter()
            .map(|(kind, why)| format!("{kind:?}：{why}"))
            .collect::<Vec<_>>()
            .join("；")
    ))
}

/// 按偏好路由（生产入口）。
pub fn route(
    preference: LoopBackend,
    content: FrameContent,
    facts: Option<AdapterFacts>,
) -> Result<RoutingDecision, String> {
    route_with_order(order_of(preference), preference, content, facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capable() -> Option<AdapterFacts> {
        Some(AdapterFacts {
            features: wgpu::Features::FLOAT32_BLENDABLE
                | wgpu::Features::DEPTH32FLOAT_STENCIL8,
            blends_float32: true,
        })
    }

    fn plain_2d() -> FrameContent {
        FrameContent {
            has_2d_primitives: 3,
            ..Default::default()
        }
    }

    /// 默认（自动）路径：矢量能画 ⇒ 选矢量，且没有后端被拒。
    ///
    /// B19 Step 3 把默认顺序从 `[Wgpu, Vector]` 换成 `[Vector, Wgpu]`：B10~B18 的能力
    /// 必须走默认路径才算生效。25 条基线**不受影响**——`cargo verify --step hashes`
    /// 用显式的 (作品, 后端) 对表，不经过 Auto 路由。
    #[test]
    fn auto_prefers_vector_now_that_it_is_the_default() {
        let decision = route(LoopBackend::Auto, plain_2d(), capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Vector);
        assert!(decision.rejected.is_empty());
        assert!(decision.reason.contains("自动"));
    }

    /// §6.3 的核心增量 + **PG Step 2**：材质的纹理**没有生产方** ⇒ 回退 wgpu，**理由非空**。
    #[test]
    fn vector_falls_back_to_wgpu_with_a_reason() {
        let content = FrameContent {
            has_content_texture: true,
            has_unserved_material_texture: true,
            ..plain_2d()
        };
        let decision = route(LoopBackend::Vector, content, capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Wgpu);
        assert_eq!(decision.rejected.len(), 1);
        assert_eq!(decision.rejected[0].0, BackendKind::Vector);
        assert!(!decision.rejected[0].1.is_empty(), "拒绝必须带理由");
        assert!(decision.reason.contains("回退"));
    }

    /// **PC 路线 A**：只有 `composite = true` 的内容层 ⇒ 矢量接受（图层级叠加）。
    #[test]
    fn composited_content_is_no_longer_rejected() {
        let content = FrameContent {
            has_content_texture: true,
            has_unserved_material_texture: false,
            ..plain_2d()
        };
        let decision = route(LoopBackend::Vector, content, capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Vector);
        assert!(decision.rejected.is_empty(), "图层级内容不应被拒");
    }

    /// **PG Step 2（路线 B）**：被材质采样、但纹理由本帧内容层生产的层 ⇒ 矢量接受
    /// （内容层当相机视图先渲，网格按句柄采样）。
    #[test]
    fn material_sampled_content_is_served_by_a_camera_view() {
        let content = FrameContent {
            has_3d_camera: true,
            has_meshes: 1,
            has_content_texture: true,
            has_material_content: true,
            has_unserved_material_texture: false,
            ..plain_2d()
        };
        let decision = route(LoopBackend::Auto, content, capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Vector);
        assert!(decision.rejected.is_empty(), "有生产方的材质纹理不应被拒");
        assert!(
            decision.reason.contains("纹理采样"),
            "理由必须写明是材质采样：{}",
            decision.reason
        );
    }

    /// 显式矢量 + 能力齐备 + 单纯 2D ⇒ 真的用矢量（不回退）。
    #[test]
    fn vector_is_used_when_it_can_draw_the_frame() {
        let decision = route(LoopBackend::Vector, plain_2d(), capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Vector);
        assert!(decision.rejected.is_empty());
    }

    /// adapter 缺 feature ⇒ 矢量被拒（理由来自 Step 0 的自述），回退 wgpu。
    #[test]
    fn adapter_without_features_rejects_vector() {
        let weak = Some(AdapterFacts {
            features: wgpu::Features::empty(),
            blends_float32: false,
        });
        let decision = route(LoopBackend::Vector, plain_2d(), weak).unwrap();
        assert_eq!(decision.backend, BackendKind::Wgpu);
        assert!(decision.rejected[0].1.contains("R32Float"));
    }

    /// 没有 adapter ⇒ 不假设矢量可用（C4）。
    #[test]
    fn missing_adapter_is_not_assumed_available() {
        let decision = route(LoopBackend::Vector, plain_2d(), None).unwrap();
        assert_eq!(decision.backend, BackendKind::Wgpu);
        assert!(decision.rejected[0].1.contains("adapter"));
    }

    /// B14：形变项尚未映射 ⇒ 矢量**显式拒绝**（不是静默丢图层），回退 wgpu 并带理由。
    #[test]
    fn morph_items_reject_the_vector_backend() {
        let content = FrameContent {
            has_2d_primitives: 2,
            has_morphs: true,
            ..Default::default()
        };
        let decision = route(LoopBackend::Vector, content, capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Wgpu);
        assert_eq!(decision.rejected[0].0, BackendKind::Vector);
        assert!(decision.rejected[0].1.contains("Morph"), "{:?}", decision.rejected);
    }

    /// **PF Step 2**：2D + 3D 混排不再被拒；决策理由写明"屏幕平面叠加"。
    #[test]
    fn mixed_2d_and_3d_routes_to_vector_with_a_layering_note() {
        let content = FrameContent {
            has_3d_camera: true,
            has_meshes: 1,
            has_2d_primitives: 2,
            ..Default::default()
        };
        let decision = route(LoopBackend::Vector, content, capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Vector);
        assert!(decision.rejected.is_empty(), "混排不应再被拒绝");
        assert!(
            decision.reason.contains("屏幕平面") && decision.reason.contains('2'),
            "理由必须写明图层级组合：{}",
            decision.reason
        );
        // 纯 3D 帧不得凭空出现"2D 记录"的说法（避免误报）。
        let pure_3d = route(
            LoopBackend::Vector,
            FrameContent {
                has_3d_camera: true,
                has_meshes: 1,
                ..Default::default()
            },
            capable(),
        )
        .unwrap();
        assert!(!pure_3d.reason.contains("2D 记录"), "{}", pure_3d.reason);
    }

    /// 显式 wgpu 不做回退：顺序里只有 wgpu。
    #[test]
    fn explicit_wgpu_has_no_fallback() {
        let decision = route(LoopBackend::Wgpu, plain_2d(), capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Wgpu);
        assert!(decision.rejected.is_empty());
    }

    /// **PE Step 3（可证伪）**：用了仅矢量支持的样式又选到 wgpu ⇒ 决策理由必须
    /// **点名**被忽略的特性；否则就是"填了没效果也不说"。
    #[test]
    fn wgpu_must_say_which_vector_only_styles_it_ignores() {
        use ze_render_ir::PathOptions;
        let content = FrameContent {
            vector_only_styles: PathOptions::FEATURE_GRADIENT | PathOptions::FEATURE_DASH,
            ..plain_2d()
        };
        let decision = route(LoopBackend::Wgpu, content, capable()).unwrap();
        assert_eq!(decision.backend, BackendKind::Wgpu);
        assert!(
            decision.reason.contains("忽略") && decision.reason.contains("渐变")
                && decision.reason.contains("虚线"),
            "理由必须点名被忽略的特性：{}",
            decision.reason
        );
        // 反向判据：没有这些特性时不得凭空加"忽略"（避免误报）。
        let clean = route(LoopBackend::Wgpu, plain_2d(), capable()).unwrap();
        assert!(!clean.reason.contains("忽略"), "{}", clean.reason);
        // 选到矢量时同样不该出现"忽略"（矢量全支持）。
        let vector = route(LoopBackend::Vector, content, capable()).unwrap();
        assert_eq!(vector.backend, BackendKind::Vector);
        assert!(!vector.reason.contains("忽略"), "{}", vector.reason);
    }

    /// 回退路径同样要写明：矢量因 adapter 被拒 ⇒ 回退 wgpu 且说明忽略了什么。
    #[test]
    fn fallback_to_wgpu_also_reports_ignored_styles() {
        use ze_render_ir::PathOptions;
        let content = FrameContent {
            vector_only_styles: PathOptions::FEATURE_SECOND,
            ..plain_2d()
        };
        let decision = route(LoopBackend::Vector, content, None).unwrap();
        assert_eq!(decision.backend, BackendKind::Wgpu);
        assert!(decision.reason.contains("回退"));
        assert!(decision.reason.contains("双 paint"), "{}", decision.reason);
    }

    /// 全部后端都拒绝 ⇒ 显式错误，且**每条**拒绝理由都非空（A4）。
    #[test]
    fn all_rejected_is_an_explicit_error_with_reasons() {
        let content = FrameContent {
            has_line_meshes: true,
            has_meshes: 1,
            has_3d_camera: true,
            ..Default::default()
        };
        let error = route_with_order(
            &[BackendKind::Vector],
            LoopBackend::Vector,
            content,
            capable(),
        )
        .unwrap_err();
        assert!(error.contains("没有后端能画这一帧"));
        assert!(error.contains("线框"));
    }

    /// C1：同一输入跑两次，决定逐字段相同。
    #[test]
    fn routing_is_repeatable() {
        let content = FrameContent {
            has_3d_camera: true,
            has_meshes: 7,
            ..Default::default()
        };
        let a = route(LoopBackend::Vector, content, capable()).unwrap();
        let b = route(LoopBackend::Vector, content, capable()).unwrap();
        assert_eq!(a, b);
        // B12 Step 2：可重复也包括**落盘行**逐字节相同（不含时间 / 次数）。
        assert_eq!(a.json("work", "script"), b.json("work", "script"));
    }

    /// B12 Step 2：生产路径产生的每条决策都能通过校验（拒绝理由非空）。
    #[test]
    fn every_production_decision_passes_validation() {
        let weak = Some(AdapterFacts {
            features: wgpu::Features::empty(),
            blends_float32: false,
        });
        let contents = [
            plain_2d(),
            FrameContent {
                has_content_texture: true,
                has_unserved_material_texture: true,
                ..plain_2d()
            },
            FrameContent {
                has_3d_camera: true,
                has_meshes: 5,
                ..Default::default()
            },
            FrameContent {
                has_3d_camera: true,
                has_2d_primitives: 2,
                has_meshes: 1,
                ..Default::default()
            },
            FrameContent {
                has_3d_camera: true,
                has_meshes: 1,
                has_line_meshes: true,
                ..Default::default()
            },
        ];
        for content in contents {
            for preference in [
                LoopBackend::Auto,
                LoopBackend::AutoFrame,
                LoopBackend::Wgpu,
                LoopBackend::Vector,
            ] {
                let decision = route(preference, content, capable()).unwrap();
                decision
                    .validate()
                    .unwrap_or_else(|e| panic!("{preference:?} / {content:?}：{e}"));
            }
            // adapter 不足 / 缺失也必须带理由（不能"悄悄回退"）。
            for facts in [weak, None] {
                let decision = route(LoopBackend::Vector, content, facts).unwrap();
                decision.validate().unwrap();
            }
        }
        RoutingDecision::explicit_wgpu().validate().unwrap();
    }

    /// 空理由 = 静默决策 ⇒ 校验必须报错（不是警告）。
    #[test]
    fn empty_reason_fails_validation() {
        let mut decision = RoutingDecision::explicit_wgpu();
        decision.reason.clear();
        assert!(decision.validate().unwrap_err().contains("静默决策"));

        let mut decision = RoutingDecision::explicit_wgpu();
        decision.rejected.push((BackendKind::Vector, "  ".to_string()));
        assert!(decision.validate().unwrap_err().contains("静默决策"));

        let mut decision = RoutingDecision::explicit_wgpu();
        decision.rejected.push((BackendKind::Wgpu, "不该同时出现".to_string()));
        assert!(decision.validate().unwrap_err().contains("自相矛盾"));
    }

    /// 落盘行是合法 JSON 形状：转义引号后仍能定位到拒绝理由。
    #[test]
    fn decision_json_escapes_and_carries_the_rejection() {
        let content = FrameContent {
            has_content_texture: true,
            has_unserved_material_texture: true,
            ..plain_2d()
        };
        let decision = route(LoopBackend::Vector, content, capable()).unwrap();
        let line = decision.json("surface_matrix", "script");
        assert!(line.starts_with("[decision] {"));
        assert!(line.contains("\"backend\":\"Wgpu\""));
        assert!(line.contains("\"backend\":\"Vector\",\"reason\":\""));
        assert!(line.contains("纹理"));
        assert!(!line.contains('\n'));
    }
}
