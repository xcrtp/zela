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
//! - [`ExecuteContext`] 只含 device / queue。
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
//! BR-3 先接管 compute 类里**自包含**的一个（泛光）；render 类需要把附件描述也搬进节点，
//! 与 BR 计划的分阶段一致。`AnyPass::Render` 变体与 [`record_compute`] 同形的 render 驱动
//! 留给 BR-4（见 GAPS 的 BR 段）。

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
pub struct ExecuteContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
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
    fn execute(&self, ctx: &ExecuteContext<'_>, pass: &mut AnyPass<'_>);
}

/// 驱动一个 **compute** pass：按声明序推进（`begin` 同时完成报数），
/// 然后 `begin_compute_pass` → `execute`。
///
/// 顺序不符 / 不在计划里 ⇒ 与手写路径同一个错误类型（`OutOfOrder` / `Undeclared`）。
pub fn record_compute(
    node: &mut dyn PassNode,
    prepare: &mut PrepareContext<'_>,
    execute: &ExecuteContext<'_>,
    schedule: &mut PassSchedule,
    label: &'static str,
) -> Result<(), ScheduleError> {
    schedule.begin(node.name())?;
    node.prepare(prepare);
    let pass = prepare
        .encoder
        .begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
    node.execute(execute, &mut AnyPass::Compute(pass));
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

        fn execute(&self, _ctx: &ExecuteContext<'_>, _pass: &mut AnyPass<'_>) {}
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
            let execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue };
            record_compute(&mut Nop { name: "a", log: &log }, &mut prepare, &execute, &mut schedule, "a")
                .expect("第一个");
            record_compute(&mut Nop { name: "b", log: &log }, &mut prepare, &execute, &mut schedule, "b")
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
            let execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue };
            let error = record_compute(&mut Nop { name: "b", log: &log }, &mut prepare, &execute, &mut schedule, "b")
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
            let execute = ExecuteContext { device: &gpu.device, queue: &gpu.queue };
            record_compute(&mut Nop { name: "a", log: &log }, &mut prepare, &execute, &mut schedule, "a")
                .expect("计划内的唯一 pass");
            let error = record_compute(&mut Nop { name: "nope", log: &log }, &mut prepare, &execute, &mut schedule, "nope")
                .unwrap_err();
            assert!(matches!(error, ScheduleError::Undeclared { .. }), "{error:?}");
            assert_eq!(*log.borrow(), vec!["a"], "表外节点不得执行");
        })
        .expect("GPU");
    }
}
