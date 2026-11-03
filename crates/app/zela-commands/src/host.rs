//! 宿主接缝：进度 / 取消 / 诊断 + 帧 sink。
//!
//! `FrameSink` 沿用 `run_frames` 既有的 `on_frame` 回调口径（不重造）：
//! 命令层只把帧交给 sink，sink 的宿主决定是写 PNG、喂 ffmpeg、还是贴到纹理上。
//! `doctor` / `list` 这类命令没有帧，只用 [`CommandHost`]。

use std::cell::{Cell, RefCell};

use zela_app::frame_loop::FrameView;
use zela_render_backends::RenderContext;

/// 诊断级别（GUI 要结构化事件，不是纯文本）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// 命令运行期间的宿主能力。
pub trait CommandHost {
    /// 进度：`done / total` 帧（或步骤）。
    fn progress(&self, done: u32, total: u32);
    /// 是否已请求取消。命令在每个可中断点查询（默认实现：不取消）。
    fn cancelled(&self) -> bool {
        false
    }
    /// 结构化诊断。
    fn diagnostic(&self, level: Level, message: &str);
}

/// 帧的落点。
///
/// `wants_rgba()` 决定主循环要不要做通道规范化（省一次全帧拷贝）。
pub trait FrameSink {
    fn wants_rgba(&self) -> bool {
        false
    }
    fn on_frame(&mut self, index: u32, frame: &FrameView<'_>) -> Result<(), String>;
    fn finish(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// 宿主建立 sink 的入口（CLI 选 PNG / mp4，GUI 选纹理，测试选收集器）。
///
/// 输出形态（`--frames` / `--out` / `--hash`）属于宿主关心项，所以 `options`
/// 原样交给工厂；命令层只负责渲染。
pub trait FrameSinkFactory {
    fn open(
        &mut self,
        ctx: &RenderContext,
        options: &crate::commands::render::RenderOptions,
    ) -> Result<Box<dyn FrameSink>, String>;
}

/// 测试 / 无头宿主：记录进度与诊断，可按帧数请求取消。
#[derive(Default)]
pub struct MemoryHost {
    progress: RefCell<Vec<(u32, u32)>>,
    diagnostics: RefCell<Vec<(Level, String)>>,
    cancel_after: Option<u32>,
    checks: Cell<u32>,
}

impl MemoryHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// 第 `limit` 次取消查询起返回 `true`（用于验证「取消保留已渲帧」）。
    pub fn cancel_after(limit: u32) -> Self {
        Self {
            cancel_after: Some(limit),
            ..Self::default()
        }
    }

    pub fn progress_log(&self) -> Vec<(u32, u32)> {
        self.progress.borrow().clone()
    }

    pub fn diagnostics(&self) -> Vec<(Level, String)> {
        self.diagnostics.borrow().clone()
    }
}

impl CommandHost for MemoryHost {
    fn progress(&self, done: u32, total: u32) {
        self.progress.borrow_mut().push((done, total));
    }

    fn cancelled(&self) -> bool {
        let seen = self.checks.get() + 1;
        self.checks.set(seen);
        self.cancel_after.is_some_and(|limit| seen >= limit)
    }

    fn diagnostic(&self, level: Level, message: &str) {
        self.diagnostics
            .borrow_mut()
            .push((level, message.to_string()));
    }
}

/// 只记录「收到了哪些帧」的 sink（不需要 GPU 读回，供命令层测试用）。
#[derive(Default)]
pub struct CollectSink {
    pub frames: Vec<(u32, (u32, u32))>,
    pub finished: bool,
}

impl CollectSink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FrameSink for CollectSink {
    fn wants_rgba(&self) -> bool {
        true
    }

    fn on_frame(&mut self, index: u32, frame: &FrameView<'_>) -> Result<(), String> {
        self.frames.push((index, frame.size));
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        self.finished = true;
        Ok(())
    }
}
