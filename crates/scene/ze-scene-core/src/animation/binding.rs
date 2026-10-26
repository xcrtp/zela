// binding.rs — 显式绑定表
//
// 取代旧的组件组合查询（三者挂同一实体）。新增动画 = 加一行 Binding，不改 System 源码。
//
// P0：`Binding.target` 由**枚举**改为 `Arc<dyn Target>`（类型擦除）——
// 新增驱动目标不再需要改枚举、也不需要改 System 源码。五个目标类型见
// [`crate::animation::target`]（四个既有 + 2D 相机取景）。
//
// S3：一条绑定可以由**曲线**驱动，也可以由**组合子表达式**驱动。
// 表达式在装配期编译成后序程序（`CompiledExpr`），运行期由宿主单子 `M` 注入通道值；
// 这样「加一个关系」不必再加一个 `Curve` 变体，也不必每帧回脚本手算。

use std::sync::Arc;

use ze_func::CompiledExpr;
use ze_time::{AnimValue, ClockId, Curve};

use crate::animation::target::Target;

/// predicate 失效时的行为（`Reset.hold` / `reset` / `skip`）。
///
/// - `Hold`：保持最后一次生效时写入的值（**需要历史** ⇒ 引擎推导 `SeekKind::Replay`）；
/// - `Reset`：写回绑定生效前的**初值**（`commit` 时快照，无历史依赖）；
/// - `Skip`：不写（字段保持原样，可被其它绑定接管）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetBehavior {
    #[default]
    Hold,
    Reset,
    Skip,
}

/// seek 语义：当前全为 O1（纯函数、无记忆），弹簧等有记忆演化器需要 Replay 模式。
///
/// **本轮有意保留的方向标记**（§8.1）：`Replay` 从"预留未接线"升级为
/// "**Scene 不保证纯**"的正式记账——触发条件：接弹簧 / 数值积分 / 外部事件。
/// 它**不是**死代码，不得顺手删除。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekKind {
    O1,
    Replay,
}

/// 绑定的驱动源。
#[derive(Debug, Clone)]
pub enum BindingDriver {
    /// 手写 / 脚本曲线（既有全部绑定）。
    Curve(Curve),
    /// 组合子表达式（S3）：装配期编译，运行期只做线性扫描。
    Expr(CompiledExpr),
}

impl BindingDriver {
    pub fn as_curve(&self) -> Option<&Curve> {
        match self {
            BindingDriver::Curve(curve) => Some(curve),
            BindingDriver::Expr(_) => None,
        }
    }

    /// 可变曲线访问（参数面板滑块只改曲线绑定）。
    pub fn as_curve_mut(&mut self) -> Option<&mut Curve> {
        match self {
            BindingDriver::Curve(curve) => Some(curve),
            BindingDriver::Expr(_) => None,
        }
    }

    pub fn as_expr(&self) -> Option<&CompiledExpr> {
        match self {
            BindingDriver::Expr(program) => Some(program),
            BindingDriver::Curve(_) => None,
        }
    }
}

/// 函子库身份：进产物，声明文件与库版本都由它派生。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryRef {
    pub namespace: String,
    pub version: String,
}

/// 单条绑定：`(时钟, 驱动源, 目标)`。
///
/// `Binding: Clone` 是硬约束（`BindingApplySystem` 每帧 clone 整表），
/// 这也是 `target` 用 `Arc<dyn Target>` 而非 `Box<dyn Target>` 的原因
/// —— `Arc` 的 clone 只是引用计数（见 `target.rs` 头部）。
#[derive(Debug, Clone)]
pub struct Binding {
    pub clock: ClockId,
    /// 曲线或表达式（S3）。
    pub driver: BindingDriver,
    /// 驱动目标（类型擦除：`BindingTable` 要装异构的所有 target）。
    pub target: Arc<dyn Target>,
    /// **生效条件**（Bool 程序；`None` = 恒真）。与 `driver` 分开求值（硬约束 3）。
    pub predicate: Option<CompiledExpr>,
    /// predicate 失效时的行为。
    pub reset: ResetBehavior,
    /// `reset = Reset` 的初值（`commit` 时从场景快照；`None` = 读不到 / 不需要）。
    pub initial: Option<AnimValue>,
    pub seek_kind: SeekKind,
}

impl Binding {
    /// 便捷访问：曲线绑定时返回曲线（表达式绑定返回 `None`）。
    pub fn curve(&self) -> Option<&Curve> {
        self.driver.as_curve()
    }
}

#[derive(Debug, Default, Clone)]
pub struct BindingTable {
    pub bindings: Vec<Binding>,
}

impl BindingTable {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }
    pub fn add(&mut self, binding: Binding) {
        self.bindings.push(binding);
    }
    /// 用**类型擦除的目标**添加一条曲线绑定，`seek_kind` 取 [`SeekKind::O1`]
    /// （当前所有绑定都是纯函数 O1）。Timeline 的编译产物在此落地。
    pub fn bind_arc(&mut self, clock: ClockId, curve: Curve, target: Arc<dyn Target>) {
        self.add(Binding {
            clock,
            driver: BindingDriver::Curve(curve),
            target,
            predicate: None,
            reset: ResetBehavior::Hold,
            initial: None,
            seek_kind: SeekKind::O1,
        });
    }

    /// 添加一条**表达式绑定**（S3）：程序已在装配期编译。
    pub fn bind_expr(&mut self, clock: ClockId, program: CompiledExpr, target: Arc<dyn Target>) {
        self.bind_expr_gated(clock, program, target, None, ResetBehavior::Hold);
    }

    /// 带 **predicate / reset** 的表达式绑定。
    ///
    /// `seek_kind` 由引擎**推导**（不进脚本）：`predicate + hold` 需要"最后一次生效"的
    /// 历史 ⇒ 保守地记为 [`SeekKind::Replay`]；其余仍是 O1。
    /// （predicate 单调性分析尚未实现 —— 保守方向是"宁可重放，不猜 O1"。）
    pub fn bind_expr_gated(
        &mut self,
        clock: ClockId,
        program: CompiledExpr,
        target: Arc<dyn Target>,
        predicate: Option<CompiledExpr>,
        reset: ResetBehavior,
    ) {
        let seek_kind = if predicate.is_some() && reset == ResetBehavior::Hold {
            SeekKind::Replay
        } else {
            SeekKind::O1
        };
        self.add(Binding {
            clock,
            driver: BindingDriver::Expr(program),
            target,
            predicate,
            reset,
            initial: None,
            seek_kind,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}
