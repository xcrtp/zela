// SPDX-FileCopyrightText: 2026 Academa, Inc.
// SPDX-License-Identifier: MIT
// Modified by Zela: BR 批新建 —— Typed Pass 节点（把"录制"从 `encode_frame` 搬进节点）。

//! Typed Pass 节点（BR 批）。
//!
//! # 本模块解决什么
//!
//! BN / BO / BP 三批让**声明**有了约束力（顺序、派发形状、绑定一致性），但录制仍是
//! `encode_frame` 里逐个手写 `begin_*_pass` + `note_pass`。本模块把「一个 pass 怎么录」
//! 收进 [`PassNode`]：驱动循环按 `PassPlan` 的声明序逐个调用，**顺序由声明给出**，
//! 报数（`PassSchedule::begin`）由驱动自动完成。
//!
//! # 为什么 prepare / execute 是两个签名
//!
//! `AnyPass` 借的是 `encoder` 的 `&mut`。所以 `execute` 时上下文里**不能再含 encoder**：
//!
//! - [`PrepareContext`] 含 `&mut CommandEncoder`（可做 clear / copy / 上传）；
//! - [`ExecuteContext`] 含 device / queue 与**调度游标**（节点绑定组时就地 `note_bind`）。
//!
//! 合成一个 `&mut PassContext` 会与 `begin_*_pass` 的借用冲突（BR 计划 §3.1）。
//!
//! # 逃生舱
//!
//! codegen 两个 pass（`fill` / `frag`）的管线键是 `hash(source)` 的**运行时**结果，
//! 强行节点化会锁死运行时管线缓存 ⇒ 它们继续用 [`AnyPass::record_raw`] 手写，
//! 记账留在 `note_pass` / `note_bind`。本模块只接管"能说清形状"的 pass。
//!
//! # 目前的接入面
//!
//! BR-3 接管了 4 个 compute pass（泛光 / 遮蔽 / base depth / 环境预滤波）；BR-4 起
//! render 类 pass 也走 [`record_render`]（附件由 [`PassNode::parts`] 给出）。

use crate::passes::{PassSchedule, ScheduleError};

/// 一个已开始的 pass（渲染或计算）。
///
/// wgpu 里 `RenderPass` 与 `ComputePass` **没有共同 trait**：都有 `set_bind_group` /
/// `set_pipeline`，但 `draw_indexed` 只在前者、`dispatch_workgroups` 只在后者。
/// 所以这里只统一两者共有的动作，其余由节点按变体自己调。
pub enum AnyPass<'a> {
    Render(wgpu::RenderPass<'a>),
    Compute(wgpu::ComputePass<'a>),
}

impl<'a> AnyPass<'a> {
    /// 设一个绑定组（两个变体同名同参）。
    pub fn set_bind_group(&mut self, index: u32, group: &wgpu::BindGroup, offsets: &[u32]) {
        match self {
            Self::Render(pass) => pass.set_bind_group(index, group, offsets),
            Self::Compute(pass) => pass.set_bind_group(index, group, offsets),
        }
    }

    /// 计算 pass（渲染节点拿到 `None` —— 形状不对是调用方的错误，节点自行决定怎么处理）。
    pub fn as_compute(&mut self) -> Option<&mut wgpu::ComputePass<'a>> {
        match self {
            Self::Compute(pass) => Some(pass),
            Self::Render(_) => None,
        }
    }

    /// 渲染 pass。
    pub fn as_render(&mut self) -> Option<&mut wgpu::RenderPass<'a>> {
        match self {
            Self::Render(pass) => Some(pass),
            Self::Compute(_) => None,
        }
    }

    /// **逃生舱**：在节点内手写录制（codegen 这类运行时管线键用）。
    pub fn record_raw(&mut self, f: impl FnOnce(&mut Self)) {
        f(self);
    }
}

/// 一次 pass 之前的上下文（含 encoder：clear / copy / 上传都在这里做）。
///
/// `device` 用于 prepare 里**建资源**（目标纹理、绑定组）——BR-3 的 base depth 需要。
pub struct PrepareContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
}

/// 一次 pass 正在录制时的上下文（**不含 encoder**：它已被 `AnyPass` 借走）。
///
/// `schedule` 让节点在 `set_bind_group` 的下一行**就地**记一笔绑定账（BP-2）——
/// 绑定事实由行为产生，节点不申报期望值；驱动在帧末统一核对。
pub struct ExecuteContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub schedule: &'a mut PassSchedule,
}

/// 一次 **render** pass 的附件描述（BR-4）。
///
/// 附件与管线一样是「一个 pass 是什么」的一部分，所以由节点给出，驱动只借用、
/// 不 `clone`、不改语义。生命周期跟着节点（`Canvas` / 目标纹理）：调用方必须保证
/// 它们活过这次录制（`encode_frame` 里即"活过本视图的循环体"）。
pub struct RenderParts<'a> {
    pub colors: Vec<Option<wgpu::RenderPassColorAttachment<'a>>>,
    pub depth: Option<wgpu::RenderPassDepthStencilAttachment<'a>>,
}

/// 一个 pass 的执行者。
///
/// 状态（管线、降采样层、组）由节点**自己**持有：这正是 Typed Pass 相对"中间件"的价值 ——
/// 每个 pass 的管线形状不同，不需要统一成一种。
pub trait PassNode {
    /// 与 `PassDecl.name` 相同（驱动按它推进声明序）。
    fn name(&self) -> &'static str;

    /// 录之前：写 uniform、清缓冲等（encoder 可用）。
    fn prepare(&mut self, ctx: &mut PrepareContext<'_>);

    /// 录：在已开始的 pass 上设管线 / 组 / 派发。
    ///
    /// 单 pass 的节点覆盖它；**多 pass 的节点**（[`Self::passes`] > 1）覆盖 [`Self::execute_at`]，
    /// 这里留默认空实现即可（驱动只调 `execute_at`）。
    fn execute(&self, _ctx: &mut ExecuteContext<'_>, _pass: &mut AnyPass<'_>) {}

    /// **render 类**节点的附件（compute 节点返回 `None`：默认实现）。
    ///
    /// 只借 `&self`：驱动在 `begin_render_pass` 前调它一次，之后同一份附件被
    /// pass 借用 —— 与 `execute(&self)` 的不可变借用可以共存。
    fn parts(&self) -> Option<RenderParts<'_>> {
        None
    }

    /// **一个逻辑 pass 要录几个 wgpu pass**（默认 1）。
    ///
    /// 有些逻辑 pass 的附件是**多张**、每次只能写一组（阴影的每层、光栅图集的每组），
    /// 而且两个 wgpu pass 之间还夹着别的 recorder（`vector::encode`）。这些都是"一个
    /// 逻辑 pass"的事实，所以由节点报数，驱动按序起 pass —— 而不是把逻辑 pass 拆成 N 个声明。
    fn passes(&self) -> usize {
        1
    }

    /// 第 `index` 个 wgpu pass 的附件（默认只有 0 号走 [`Self::parts`]）。
    fn parts_at(&self, index: usize) -> Option<RenderParts<'_>> {
        if index == 0 { self.parts() } else { None }
    }

    /// 第 `index` 个 wgpu pass 的录制（默认只有 0 号走 [`Self::execute`]）。
    fn execute_at(&self, index: usize, ctx: &mut ExecuteContext<'_>, pass: &mut AnyPass<'_>) {
        if index == 0 {
            self.execute(ctx, pass);
        }
    }

    /// 第 `index` 个 wgpu pass **结束之后**的 encoder 工作。
    ///
    /// 本逻辑 pass 里夹着别的 recorder 时用（例如阴影每层栅格之后、图集每组栅格之后，
    /// 再让 `vector::encode` 往同一张深度 / 同一张合成目标里写）。`prepare` 管"所有 pass
    /// 之前"，这里管"每个 pass 之后"。
    ///
    /// **BS-1**：多了 [`ExecuteContext`] —— 夹在中间的 recorder 也要就地 `note_bind`，
    /// 否则它绑的组进不了"声明 vs 实际"的比对（BP-05）。
    fn after(&mut self, _index: usize, _prepare: &mut PrepareContext<'_>, _execute: &mut ExecuteContext<'_>) {}

    /// **BS-2（逃生舱）**：节点自己起 pass（**数量由数据决定**时用）。
    ///
    /// `execute` 拿到的是已开始的 `AnyPass`，`encoder` 已被它借走 ⇒ 节点内部不能再起新
    /// pass。而一个 group 的 `vector::encode` 会起 1–4 个 compute pass（flatten / composite /
    /// keep / slab bounds），数量随数据变化 —— 这种形状没法用 `execute` 表达。
    ///
    /// 默认 `false` ⇒ 走驱动的自动 `begin`。开着它的节点必须在 GAPS 记账并写明理由
    /// （终检断言实现者只有 `VectorCompositeNode` 一个）。
    fn manual(&self) -> bool {
        false
    }

    /// `manual()` 为真时由驱动调用（**驱动已完成 `begin` / `prepare`**）。
    fn run(&mut self, _prepare: &mut PrepareContext<'_>, _execute: &mut ExecuteContext<'_>) {}
}

/// 驱动一个 **compute** pass：按声明序推进（`begin` 同时完成报数），
/// 然后 `begin_compute_pass` → `execute`。
///
/// 顺序不符 / 不在计划里 ⇒ 与手写路径同一个错误类型（`OutOfOrder` / `Undeclared`）。
pub fn record_compute(
    node: &mut dyn PassNode,
    prepare: &mut PrepareContext<'_>,
    execute: &mut ExecuteContext<'_>,
    label: &'static str,
) -> Result<(), ScheduleError> {
    execute.schedule.begin(node.name())?;
    node.prepare(prepare);
    if node.manual() {
        // 逃生舱（BS-2）：pass 的数量 / 形状由数据决定，节点自己起（`vector::encode`）。
        node.run(prepare, execute);
        return Ok(());
    }
    let pass = prepare
        .encoder
        .begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
    node.execute(execute, &mut AnyPass::Compute(pass));
    Ok(())
}

/// 驱动一个 **render** pass：按声明序推进（`begin` 同时完成报数），
/// 然后逐个子 pass：取附件 → `begin_render_pass` → `execute_at` → `after`。
///
/// 与 [`record_compute`] 的差别是附件来自 [`PassNode::parts_at`]（BR-4）：
/// 驱动不认识 `Canvas` / `Depth`，只认 [`RenderParts`] —— 新增一个 render pass
/// 不需要改驱动。子 pass 数由 [`PassNode::passes`] 给出（默认 1）：阴影每层、
/// 图集每组各起一个 wgpu pass，中间夹的 encoder 工作落在 [`PassNode::after`]。
pub fn record_render(
    node: &mut dyn PassNode,
    prepare: &mut PrepareContext<'_>,
    execute: &mut ExecuteContext<'_>,
    label: &'static str,
) -> Result<(), ScheduleError> {
    execute.schedule.begin(node.name())?;
    node.prepare(prepare);
    for index in 0..node.passes() {
        // 没有附件的子 pass 就不起 pass（例如某组没有光栅对象），但 `after` 照跑：
        // 它是"这一组的 encoder 工作"，与 pass 有无是两件事。
        if let Some(parts) = node.parts_at(index) {
            let RenderParts { colors, depth } = parts;
            let pass = prepare.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &colors,
                depth_stencil_attachment: depth,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            node.execute_at(index, execute, &mut AnyPass::Render(pass));
            // pass 在这里结束（附件借的是节点自己的纹理视图）。
        }
        node.after(index, prepare, execute);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passes::{Dispatch, EnabledWhen, PassDecl, PassPlan, PassStage};
    use std::cell::RefCell;

    const fn decl(name: &'static str) -> PassDecl {
        PassDecl {
            name,
            kind: PassStage::Compute,
            inputs: &[],
            outputs: &[],
            entry: name,
            dispatch: Dispatch::Fixed,
            enabled_when: EnabledWhen::ALWAYS,
        }
    }

    /// 记录自己被 prepare 的顺序；execute 不做任何 GPU 动作。
    struct Nop<'a> {
        name: &'static str,
        log: &'a RefCell<Vec<&'static str>>,
    }

    impl PassNode for Nop<'_> {
        fn name(&self) -> &'static str {
            self.name
        }

        fn prepare(&mut self, _ctx: &mut PrepareContext<'_>) {
            self.log.borrow_mut().push(self.name);
        }

        fn execute(&self, _ctx: &mut ExecuteContext<'_>, _pass: &mut AnyPass<'_>) {}
    }

    /// 驱动按声明序执行两个 pass，逐个报数，`finish` 通过。
    #[test]
    fn nodes_run_in_declaration_order() {
        crate::render::with_gpu(|gpu| {
            const PLAN: PassPlan = &[decl("a"), decl("b")];
            let mut schedule = PassSchedule::new(PLAN, 0);
            let log = RefCell::new(Vec::new());
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let mut prepare = PrepareContext { device: &gpu.device, queue: &gpu.queue, encoder: &mut encoder };
            let mut execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue, schedule: &mut schedule };
            record_compute(&mut Nop { name: "a", log: &log }, &mut prepare, &mut execute, "a")
                .expect("第一个");
            record_compute(&mut Nop { name: "b", log: &log }, &mut prepare, &mut execute, "b")
                .expect("第二个");
            assert_eq!(*log.borrow(), vec!["a", "b"]);
            assert_eq!(schedule.recorded(), &["a", "b"]);
            schedule.finish().expect("两个都记录了");
        })
        .expect("GPU");
    }

    /// 先驱动后一个 pass ⇒ `OutOfOrder`（顺序来自声明，不来自调用顺序）。
    #[test]
    fn a_node_out_of_order_is_red() {
        crate::render::with_gpu(|gpu| {
            const PLAN: PassPlan = &[decl("a"), decl("b")];
            let mut schedule = PassSchedule::new(PLAN, 0);
            let log = RefCell::new(Vec::new());
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let mut prepare = PrepareContext { device: &gpu.device, queue: &gpu.queue, encoder: &mut encoder };
            let mut execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue, schedule: &mut schedule };
            let error = record_compute(&mut Nop { name: "b", log: &log }, &mut prepare, &mut execute, "b")
                .unwrap_err();
            assert!(matches!(error, ScheduleError::OutOfOrder { .. }), "{error:?}");
            assert!(log.borrow().is_empty(), "顺序不符时不得执行");
        })
        .expect("GPU");
    }

    /// 计划已经推进完之后又驱动一个节点 ⇒ `Undeclared`。
    #[test]
    fn an_undeclared_node_is_red() {
        crate::render::with_gpu(|gpu| {
            const PLAN: PassPlan = &[decl("a")];
            let mut schedule = PassSchedule::new(PLAN, 0);
            let log = RefCell::new(Vec::new());
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let mut prepare = PrepareContext { device: &gpu.device, queue: &gpu.queue, encoder: &mut encoder };
            let mut execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue, schedule: &mut schedule };
            record_compute(&mut Nop { name: "a", log: &log }, &mut prepare, &mut execute, "a")
                .expect("计划内的唯一 pass");
            let error = record_compute(&mut Nop { name: "nope", log: &log }, &mut prepare, &mut execute, "nope")
                .unwrap_err();
            assert!(matches!(error, ScheduleError::Undeclared { .. }), "{error:?}");
            assert_eq!(*log.borrow(), vec!["a"], "表外节点不得执行");
        })
        .expect("GPU");
    }

    // ─── BR-4：render 驱动的三条同形用例 ────────────────────────────────

    const fn render_decl(name: &'static str) -> PassDecl {
        PassDecl {
            name,
            kind: PassStage::Render,
            inputs: &[],
            outputs: &[],
            entry: name,
            dispatch: Dispatch::Draw,
            enabled_when: EnabledWhen::ALWAYS,
        }
    }

    /// 与 `Nop` 同形，但给出附件（一个颜色目标、加载清空）；execute 不做 GPU 动作。
    struct RenderNop<'a> {
        name: &'static str,
        log: &'a RefCell<Vec<&'static str>>,
        target: &'a wgpu::TextureView,
    }

    impl PassNode for RenderNop<'_> {
        fn name(&self) -> &'static str {
            self.name
        }

        fn prepare(&mut self, _ctx: &mut PrepareContext<'_>) {
            self.log.borrow_mut().push(self.name);
        }

        fn parts(&self) -> Option<RenderParts<'_>> {
            Some(RenderParts {
                colors: vec![Some(wgpu::RenderPassColorAttachment {
                    view: self.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth: None,
            })
        }

        fn execute(&self, _ctx: &mut ExecuteContext<'_>, _pass: &mut AnyPass<'_>) {}
    }

    /// 测试用颜色目标（1×1、可作渲染附件）。
    fn target(device: &wgpu::Device) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("节点用例目标"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    /// 按声明序驱动两个 render pass，逐个报数，`finish` 通过。
    #[test]
    fn render_nodes_run_in_declaration_order() {
        crate::render::with_gpu(|gpu| {
            const PLAN: PassPlan = &[render_decl("a"), render_decl("b")];
            let mut schedule = PassSchedule::new(PLAN, 0);
            let log = RefCell::new(Vec::new());
            let view = target(&gpu.device);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let mut prepare = PrepareContext { device: &gpu.device, queue: &gpu.queue, encoder: &mut encoder };
            let mut execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue, schedule: &mut schedule };
            record_render(&mut RenderNop { name: "a", log: &log, target: &view }, &mut prepare, &mut execute, "a")
                .expect("第一个");
            record_render(&mut RenderNop { name: "b", log: &log, target: &view }, &mut prepare, &mut execute, "b")
                .expect("第二个");
            assert_eq!(*log.borrow(), vec!["a", "b"]);
            assert_eq!(schedule.recorded(), &["a", "b"]);
            schedule.finish().expect("两个都记录了");
        })
        .expect("GPU");
    }

    /// 先驱动后一个 render pass ⇒ `OutOfOrder`，且不得开 pass / 执行。
    #[test]
    fn a_render_node_out_of_order_is_red() {
        crate::render::with_gpu(|gpu| {
            const PLAN: PassPlan = &[render_decl("a"), render_decl("b")];
            let mut schedule = PassSchedule::new(PLAN, 0);
            let log = RefCell::new(Vec::new());
            let view = target(&gpu.device);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let mut prepare = PrepareContext { device: &gpu.device, queue: &gpu.queue, encoder: &mut encoder };
            let mut execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue, schedule: &mut schedule };
            let error = record_render(&mut RenderNop { name: "b", log: &log, target: &view }, &mut prepare, &mut execute, "b")
                .unwrap_err();
            assert!(matches!(error, ScheduleError::OutOfOrder { .. }), "{error:?}");
            assert!(log.borrow().is_empty(), "顺序不符时不得执行");
        })
        .expect("GPU");
    }

    /// 计划推进完之后再驱动一个 render 节点 ⇒ `Undeclared`。
    #[test]
    fn an_undeclared_render_node_is_red() {
        crate::render::with_gpu(|gpu| {
            const PLAN: PassPlan = &[render_decl("a")];
            let mut schedule = PassSchedule::new(PLAN, 0);
            let log = RefCell::new(Vec::new());
            let view = target(&gpu.device);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let mut prepare = PrepareContext { device: &gpu.device, queue: &gpu.queue, encoder: &mut encoder };
            let mut execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue, schedule: &mut schedule };
            record_render(&mut RenderNop { name: "a", log: &log, target: &view }, &mut prepare, &mut execute, "a")
                .expect("计划内的唯一 pass");
            let error = record_render(&mut RenderNop { name: "nope", log: &log, target: &view }, &mut prepare, &mut execute, "nope")
                .unwrap_err();
            assert!(matches!(error, ScheduleError::Undeclared { .. }), "{error:?}");
            assert_eq!(*log.borrow(), vec!["a"], "表外节点不得执行");
        })
        .expect("GPU");
    }
}
