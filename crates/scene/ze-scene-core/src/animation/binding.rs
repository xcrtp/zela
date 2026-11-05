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

use ze_comb::CompiledExpr;
use ze_time::{AnimValue, ClockId, Curve};

use crate::animation::predicate::MultiIntervalSet;
use crate::animation::scope::ScopeId;
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

/// seek 语义的**代价谱系**（BY-a4）：档位由装配期识别器推导，不是手填。
///
/// | 档位 | 含义 | 推导条件 |
/// |---|---|---|
/// | `O1` | 单帧纯函数，常数代价 | 无状态、无区间门槛 |
/// | `OPerInterval` | 按 predicate 区间分段，段内常数代价 | 可分析 predicate（`active` 有值） |
/// | `OLogN` | 帧号驱动的 `State`，矩阵快速幂 `O(k³ log n)` | 含至少一个仿射 `State` 计划 |
/// | `EventReplay` | **L2**：事件 predicate 的 `State`，**只重放事件帧**（稀疏） | 计划依赖 `event` 通道 |
/// | `Replay` | 需要历史重放（离散层 / 历史 payload 的 Hold） | `requires_replay`，AI-07 装配期拒绝 |
///
/// 优先级：`Replay > EventReplay > OLogN > OPerInterval > O1`（越"贵"越先命中）。
/// `EventReplay` **不**触发 AI-07（它的重放范围由事件日志界定，不是任意历史）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekKind {
    O1,
    OLogN,
    OPerInterval,
    /// **BZ-c / L2**：事件 predicate 驱动的 State —— 只需按事件帧推进
    /// （`seek = O(事件数 × log 间隔)`），帧间是 identity。
    EventReplay,
    /// **AI-07**：引擎没有重放路径 ⇒ 装配期被 [`BindingTable::reject_replay`] 拒绝。
    /// 它**不是**死代码，不得顺手删除（提案阶段 7 落地重放后才放开）。
    Replay,
}

/// 装配期拒绝：`SeekKind::Replay` 是 AI-07 红线（不向作者开放任意历史重放）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "绑定 `{target}` 需要历史重放（SeekKind::Replay：历史 payload 的 predicate + Hold，\
     或含 NatRec 的离散层）；引擎不提供 `Replay`（AI-07 红线），seek 到中间帧会拿到错值，\
     故在装配期显式拒绝"
)]
pub struct ReplayUnsupported {
    pub target: String,
}

/// 表达式驱动：**lowered 恒有、typed 可选**（定义在 `ze-comb`，**领域无关**）。
///
/// 场景层不自己实现"要不要单态化、失败怎么回落"—— 那是内核的编译策略
/// （[`ze_comb::ExprProgram`]）；这里只把它接到绑定表上。
pub use ze_comb::ExprProgram;

/// 绑定的驱动源。
#[derive(Debug, Clone)]
pub enum BindingDriver {
    /// 手写 / 脚本曲线（既有全部绑定）。
    Curve(Curve),
    /// 组合子表达式（S3）：装配期编译，运行期只做线性扫描。
    Expr(ExprProgram),
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
            BindingDriver::Expr(program) => Some(program.lowered()),
            BindingDriver::Curve(_) => None,
        }
    }
}

/// 函子库身份：进产物，声明文件与库版本都由它派生。
///
/// **第 4 批**：`op_revisions` 是**细粒度**的算子版本记录（`name → (since, changed)`），
/// 与粗粒度的 `version` 并存。加载时逐个比对 ⇒ 只作废**用到变了语义的算子**的产物，
/// 而不是整库作废。老 payload 没有这个字段 ⇒ `#[serde(default)]` 解成空表
/// （回放路径不做推导，缺记录是合法状态）。
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LibraryRef {
    pub namespace: String,
    pub version: String,
    #[serde(default)]
    pub op_revisions: std::collections::BTreeMap<String, (u32, u32)>,
}

/// 单条绑定：`(时钟, 驱动源, 目标)`。
///
/// `Binding: Clone` 是硬约束（`BindingApplySystem` 每帧 clone 整表），
/// 这也是 `target` 用 `Arc<dyn Target>` 而非 `Box<dyn Target>` 的原因
/// —— `Arc` 的 clone 只是引用计数（见 `target.rs` 头部）。
#[derive(Debug, Clone)]
pub struct Binding {
    pub clock: ClockId,
    /// **CC-a**：所属作用域（`ScopeId::ROOT` = 既有脚本 / 根作用域）。
    ///
    /// 作用域只做**归属**；时间轴来自 `clock`（已接在作用域时钟之下）。
    pub scope: ScopeId,
    /// **CC-c**：字段级仲裁优先级（大的胜；同优先级 ⇒ 后声明者胜）。
    ///
    /// 声明**就近放在绑定侧**（不集中成表，避免第二个真源）；脚本装配默认 0。
    pub priority: u8,
    /// 曲线或表达式（S3）。
    pub driver: BindingDriver,
    /// 驱动目标（类型擦除：`BindingTable` 要装异构的所有 target）。
    pub target: Arc<dyn Target>,
    /// **生效条件**（Bool 程序；`None` = 恒真）。与 `driver` 分开求值（硬约束 3）。
    pub predicate: Option<CompiledExpr>,
    /// predicate → **局部时间区间集**（AH-06）：`Some` 时生效判定与 Hold 反算都是
    /// 纯函数（不再逐帧求值 predicate）；`None` = 不可分析，运行期回退。
    pub active: Option<MultiIntervalSet>,
    /// predicate 失效时的行为。
    pub reset: ResetBehavior,
    /// `reset = Reset` 的初值（`commit` 时从场景快照；`None` = 读不到 / 不需要）。
    pub initial: Option<AnimValue>,
    /// **BY**：该绑定里各 `State` 的闭式计划（下标 = `Step::State.id`）。
    ///
    /// 派生量：每次 `commit` 重算，**不进 payload**。运行期由 `SceneMono` 消费。
    pub state_plans: Vec<crate::animation::affine::StatePlan>,
    pub seek_kind: SeekKind,
}

impl Binding {
    /// 便捷访问：曲线绑定时返回曲线（表达式绑定返回 `None`）。
    pub fn curve(&self) -> Option<&Curve> {
        self.driver.as_curve()
    }
}

#[derive(Debug, Clone)]
pub struct BindingTable {
    pub bindings: Vec<Binding>,
    /// 作用域路径表（下标 = [`ScopeId`] 的序号；`[0]` 恒为空串 = 根作用域）。
    ///
    /// **只进装配期 + 参数地址显示**：声明顺序即序号，产物里只出现路径。
    scopes: Vec<String>,
}

impl Default for BindingTable {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingTable {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            scopes: vec![String::new()],
        }
    }
    pub fn add(&mut self, binding: Binding) {
        self.bindings.push(binding);
    }

    /// 登记一条作用域路径（首次出现即分配序号；重复路径复用同一序号）。
    ///
    /// 空路径 = 根作用域（恒为 0）。
    pub fn declare_scope(&mut self, path: &str) -> ScopeId {
        if path.is_empty() {
            return ScopeId::ROOT;
        }
        if let Some(index) = self.scopes.iter().position(|known| known == path) {
            return ScopeId(index as u32);
        }
        self.scopes.push(path.to_string());
        ScopeId((self.scopes.len() - 1) as u32)
    }

    /// 作用域的对外路径（根作用域返回空串）。
    pub fn scope_path(&self, id: ScopeId) -> &str {
        self.scopes
            .get(id.0 as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    /// 全部作用域路径（下标 = 序号）。
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// 用**类型擦除的目标**添加一条曲线绑定，`seek_kind` 取 [`SeekKind::O1`]
    /// （曲线绑定是纯函数）。Timeline 的编译产物在此落地。
    pub fn bind_arc(&mut self, clock: ClockId, curve: Curve, target: Arc<dyn Target>) {
        self.add(Binding {
            clock,
            scope: ScopeId::ROOT,
            priority: 0,
            driver: BindingDriver::Curve(curve),
            target,
            predicate: None,
            active: None,
            reset: ResetBehavior::Hold,
            initial: None,
            state_plans: Vec::new(),
            seek_kind: SeekKind::O1,
        });
    }

    /// 添加一条**表达式绑定**（S3）：程序已在装配期编译。
    ///
    /// 传 [`CompiledExpr`] 会自动包成"仅 lowered"的 [`ExprProgram`]（测试 / 手写装配）；
    /// 走脚本装配时由 [`ExprProgram::compile`] 顺带做单态化。
    pub fn bind_expr(
        &mut self,
        clock: ClockId,
        program: impl Into<ExprProgram>,
        target: Arc<dyn Target>,
    ) {
        self.bind_expr_gated(clock, program, target, None, None, ResetBehavior::Hold);
    }

    /// 带 **predicate / reset** 的表达式绑定。
    ///
    /// `active` 是 predicate 在**局部时间**上的区间集（[`crate::animation::predicate`]）：
    /// - `Some` ⇒ 生效判定与 `Hold` 都是纯函数 `f(last_active(t))` ⇒ `SeekKind::O1`；
    /// - `None` ⇒ predicate 不可分析：运行期逐帧求值。脚本装配期已拒绝
    ///   `Hold + 不可分析`（[`crate::scene_script::ExprBindError::UnanalyzableHold`]），
    ///   因此这里只可能来自**历史 payload**；`Hold` 仍有历史依赖，保守记 `Replay`。
    ///
    /// `Replay` 本身只为"有记忆的 root"（弹簧 / 数值积分）保留 —— 本批不实现，
    /// 但**不得删除**（AH-07）。
    pub fn bind_expr_gated(
        &mut self,
        clock: ClockId,
        program: impl Into<ExprProgram>,
        target: Arc<dyn Target>,
        predicate: Option<CompiledExpr>,
        active: Option<MultiIntervalSet>,
        reset: ResetBehavior,
    ) {
        self.bind_expr_gated_with_states(
            clock,
            program,
            target,
            predicate,
            active,
            reset,
            Vec::new(),
        );
    }

    /// **BY**：带 `State` 闭式计划的表达式绑定（计划下标 = `Step::State.id`）。
    pub fn bind_expr_gated_with_states(
        &mut self,
        clock: ClockId,
        program: impl Into<ExprProgram>,
        target: Arc<dyn Target>,
        predicate: Option<CompiledExpr>,
        active: Option<MultiIntervalSet>,
        reset: ResetBehavior,
        state_plans: Vec<crate::animation::affine::StatePlan>,
    ) {
        self.bind_expr_gated_with_states_scoped(
            clock,
            program,
            target,
            predicate,
            active,
            reset,
            state_plans,
            ScopeId::ROOT,
        );
    }

    /// **CC-a**：与 [`bind_expr_gated_with_states`](Self::bind_expr_gated_with_states)
    /// 相同，但显式指定作用域。
    #[allow(clippy::too_many_arguments)]
    pub fn bind_expr_gated_with_states_scoped(
        &mut self,
        clock: ClockId,
        program: impl Into<ExprProgram>,
        target: Arc<dyn Target>,
        predicate: Option<CompiledExpr>,
        active: Option<MultiIntervalSet>,
        reset: ResetBehavior,
        state_plans: Vec<crate::animation::affine::StatePlan>,
        scope: ScopeId,
    ) {
        let program = program.into();
        let seek_kind = seek_kind_for(
            program.requires_replay(),
            predicate.is_some(),
            reset,
            active.is_some(),
            !state_plans.is_empty(),
            state_plans.iter().any(|plan| plan.event_gated()),
        );
        self.add(Binding {
            clock,
            scope,
            priority: 0,
            driver: BindingDriver::Expr(program),
            target,
            predicate,
            active,
            reset,
            initial: None,
            state_plans,
            seek_kind,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// **装配期门禁（AI-07）**：`Replay` 是显式红线（引擎实现的是
    /// `O1 / OPerInterval / OLogN / EventReplay`）。带 `SeekKind::Replay` 的绑定
    /// seek 到中间帧会拿到错值 ⇒ 在绑定表交付前显式拒绝。
    ///
    /// 现状零生产 `Replay`（可分析的 Hold 是纯函数，脚本侧不可分析的 Hold 已在
    /// [`crate::scene_script::ExprBindError::UnanalyzableHold`] 拒绝），因此这道
    /// 门禁只会挡住**历史 payload** 与 `NatRec` 这类真正需要历史的绑定。
    pub fn reject_replay(&self) -> Result<(), ReplayUnsupported> {
        match self
            .bindings
            .iter()
            .find(|binding| binding.seek_kind == SeekKind::Replay)
        {
            Some(binding) => Err(ReplayUnsupported {
                target: binding.target.name().to_string(),
            }),
            None => Ok(()),
        }
    }
}

/// `SeekKind` 推导（计划 §0.4：`pointwise` 报告终于接到这里）。
///
/// 两条独立的历史依赖来源：
/// 1. **离散层**（`NatRec` / 递归叶子）⇒ 需要整段重放（由 `CompiledExpr` 的
///    `requires_replay` 提供，是 `infer::PointwiseReport` 在 kernel 侧的投影）；
/// 2. `Hold + 不可分析的 predicate` ⇒ 运行期逐帧求值，保守记 `Replay`。
///
/// 连续层的读通道 / 算子调用仍是 O1（单帧纯函数）。
fn seek_kind_for(
    requires_replay: bool,
    predicate_present: bool,
    reset: ResetBehavior,
    active_known: bool,
    has_state_plan: bool,
    has_event_state: bool,
) -> SeekKind {
    if requires_replay || (predicate_present && reset == ResetBehavior::Hold && !active_known) {
        SeekKind::Replay
    } else if has_event_state {
        // **BZ-c**：事件 predicate 的 State ⇒ L2（只重放事件帧，帧间 identity）。
        SeekKind::EventReplay
    } else if has_state_plan {
        // 帧号驱动的仿射 State：矩阵快速幂 O(k³ log n)。
        SeekKind::OLogN
    } else if active_known {
        // 可分析 predicate：按区间分段，段内单帧纯函数。
        SeekKind::OPerInterval
    } else {
        SeekKind::O1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_expressions_stay_o1() {
        assert_eq!(
            seek_kind_for(false, false, ResetBehavior::Hold, false, false, false),
            SeekKind::O1
        );
        assert_eq!(
            seek_kind_for(false, true, ResetBehavior::Hold, true, false, false),
            SeekKind::OPerInterval,
            "可分析 predicate ⇒ 按区间分段"
        );
    }

    #[test]
    fn discrete_layer_requires_replay() {
        assert_eq!(
            seek_kind_for(true, false, ResetBehavior::Hold, false, false, false),
            SeekKind::Replay,
            "含 NatRec 的绑定必须重放"
        );
    }

    #[test]
    fn unanalyzable_hold_stays_replay() {
        assert_eq!(
            seek_kind_for(false, true, ResetBehavior::Hold, false, false, false),
            SeekKind::Replay
        );
        assert_eq!(
            seek_kind_for(false, true, ResetBehavior::Skip, false, false, false),
            SeekKind::O1,
            "非 Hold 的失效行为不需要历史"
        );
    }

    /// **BY-a4**：档位由识别结果推导 —— 有计划 ⇒ `OLogN`，且优先于区间档。
    #[test]
    fn state_plan_derives_ologn() {
        assert_eq!(
            seek_kind_for(false, false, ResetBehavior::Hold, false, true, false),
            SeekKind::OLogN
        );
        assert_eq!(
            seek_kind_for(false, true, ResetBehavior::Hold, true, true, false),
            SeekKind::OLogN,
            "State 计划优先于 OPerInterval"
        );
        assert_eq!(
            seek_kind_for(true, false, ResetBehavior::Hold, false, true, false),
            SeekKind::Replay,
            "Replay 仍最优先"
        );
        // **BZ-c**：事件 predicate 的 State ⇒ L2（只重放事件帧），且不触发 AI-07。
        assert_eq!(
            seek_kind_for(false, false, ResetBehavior::Hold, false, true, true),
            SeekKind::EventReplay
        );
        assert_eq!(
            seek_kind_for(true, false, ResetBehavior::Hold, false, true, true),
            SeekKind::Replay,
            "真正的 Replay 仍最优先"
        );
    }

    /// 结构判据：`NatRec` 编译产物携带 `requires_replay`，普通表达式不携带。
    #[test]
    fn compiled_program_knows_about_discrete_nodes() {
        let continuous = ze_comb::Expr::call(
            ze_comb::OpRef::new("zela.math", "sin"),
            [ze_comb::Expr::pure(1.0)],
        );
        assert!(
            !ze_comb::CompiledExpr::compile(&continuous)
                .expect("可编译")
                .requires_replay()
        );

        let discrete = ze_comb::Expr::new(ze_comb::Node::NatRec {
            count: Box::new(ze_comb::Expr::pure(3i64)),
            init: Box::new(ze_comb::Expr::pure(0.0)),
            step: Box::new(ze_comb::Expr::rec_prev()),
        });
        assert!(
            ze_comb::CompiledExpr::compile(&discrete)
                .expect("NatRec 可编译")
                .requires_replay()
        );
    }
}
