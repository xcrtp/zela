//! `zela-commands`：**命令帧体（唯一实现）**。
//!
//! # 为什么有这一层
//!
//! 同一个命令要被两种宿主消费：`zela`（clap 适配）与 GUI / 未来编辑器（egui 适配）。
//! 若把命令写在 CLI 里，GUI 就只能 parse stdout —— 抽象白做。所以命令逻辑放在这里，
//! 宿主只负责「怎么拿到参数」与「怎么展示结果」。
//!
//! 这与项目既有的**「帧体 + 适配层」**模式（`scene_callback::prepare_frame`）同构：
//! egui 路径与 headless 路径都调同一个帧体 ⇒ 两条路径逐位一致是**结构保证**。
//!
//! # 四个接缝
//!
//! - [`host::FrameSink`]：帧去往哪里（现成的 `run_frames` 回调抽象）；
//! - [`host::CommandHost`]：进度 / 取消 / 诊断（GUI 要结构化事件，不是 `eprintln!`）；
//! - [`response::Response`]：结构化返回值（CLI 负责格式化，GUI 负责显示）；
//! - [`spec::CommandSpec`]：声明式参数描述（CLI → clap，GUI → 表单控件）。
//!
//! # 硬约束
//!
//! `Cargo.toml` 里不得出现 `clap` / `egui` / `eframe`（§0.1 判据）。

pub mod commands;
pub mod host;
pub mod response;
pub mod spec;

pub use host::{CommandHost, FrameSink, FrameSinkFactory, Level, MemoryHost, CollectSink};
pub use response::{DemoRow, Fact, ListSummary, RenderSummary, Response};
pub use spec::{ArgSpec, ArgType, ArgValue, CommandArgs, CommandSpec, Validator};
