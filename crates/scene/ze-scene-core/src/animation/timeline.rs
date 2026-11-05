// timeline.rs — 表达式绑定的**编排容器**（时间区间由每条绑定自己的时钟决定）。
//
// # 语义（5b 退役后）
//
// - 旧的 `play(curve,target,dur)` / `parallel` / `wait` 已删除：区间不再由
//   "顺序游标"隐式分配，而是每条绑定**显式**声明 `clock` + `predicate`；
// - 顺序 = `clock.shift(起点)`，并行 = 共用同一 clock（缺省状态）⇒ **没有并行操作**；
// - `Step::Play` / `Step::Wait` 与 `Action` / `ActionTarget::Ready` 一并删除。
//
// # 向后的唯一形状
//
// `Step::Expr`（老 payload 里带 `duration` 的那些仍可解码，用旧口径占一段）。
//
// # 关键原则
//
// **求值层零改动** —— `ClockForest` / `eval_local` 一行都不动；本模块只把
// 时钟链展开成 `ClockNode` 链、把 predicate 编译成区间集。

use std::sync::Arc;

use ze_comb::{CompiledExpr, Expr, Node};
use ze_scene_contract::ObjectId;
use ze_time::{ClockForest, ClockNode, ClockProgram, PlayMode};

use crate::animation::affine::{StateAnalysis, StateUnsupported};
use crate::animation::binding::{BindingTable, ExprProgram, LibraryRef, ResetBehavior};
use crate::animation::scope::ScopeSpec;
use crate::animation::target::Target;

/// **静止时刻报告**（阶段 2）：把"推导不出来"的**原因**显式分开。
///
/// - `at = Some(t)`：可推导的静止时刻（`still_time()` 的返回值）；
/// - `unanalyzable`：predicate 形状不在区间代数里（事件 / 非字面量边界 / 负速率时钟）；
/// - `non_time_dependency`：predicate 依赖**非时间**连续变量（`x` / 组件字段读）；
/// - `never_static`：恒生效（画面永远在变）。
///
/// 后两者都会让 `at = None`，但修法不同 —— 因此不得合并成一个 `Option`。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StillTimeReport {
    pub at: Option<f64>,
    pub unanalyzable: bool,
    pub non_time_dependency: bool,
    pub never_static: bool,
}

/// **声明期待解析**的绑定目标（场景脚本：`commit` 时按句柄求 id）。
///
/// `make` 是 `TargetCtor::make` 的**函数指针**（不捕获任何环境 ⇒ 天然 `Send + Sync`）；
/// `name` 是 target 的稳定语义名（持久化替身，序列化只写它）；
/// `handle` 是**本段脚本内**的句柄下标（`append` 时整体偏移）。
#[derive(Clone)]
struct PendingTarget {
    make: fn(ObjectId) -> Arc<dyn Target>,
    name: &'static str,
    handle: usize,
}

impl serde::Serialize for PendingTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Target", 3)?;
        state.serialize_field("kind", "pending")?;
        state.serialize_field("ctor", self.name)?;
        state.serialize_field("handle", &self.handle)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for PendingTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Repr {
            Pending { ctor: String, handle: usize },
        }

        let Repr::Pending { ctor, handle } =
            <Repr as serde::Deserialize>::deserialize(deserializer)?;
        let (make, name) =
            crate::animation::target::target_ctor_by_name(&ctor).ok_or_else(|| {
                serde::de::Error::custom(
                    crate::persist::PersistError::UnknownTarget(ctor.clone()).to_string(),
                )
            })?;
        Ok(PendingTarget { make, name, handle })
    }
}

/// 编排步骤：**只剩表达式绑定**（旧的 `Wait` / `Play` 已在 5b 删除）。
///
/// 老 payload 里带 `duration` 的表达式步骤仍可解码（占一段，用旧口径）。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum Step {
    Expr {
        /// 旧 payload 的段时长（**只用于解码历史产物**；新产物的糖已展开成 predicate）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration: Option<f64>,
        target: PendingTarget,
        expr: Expr,
        /// 生效条件（Bool 表达式；`None` = 恒真）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        predicate: Option<Expr>,
        /// predicate 失效时的行为。
        #[serde(default)]
        reset: ResetBehavior,
        /// 该绑定的**时钟程序**（空链 = 全局时钟；不进 payload，见 `skip_serializing_if`）。
        #[serde(default, skip_serializing_if = "ClockProgram::is_global")]
        clock: ClockProgram,
        /// **CC-a**：所属作用域（只存声明式路径 + 作用域时钟；序号由编译期重新分配）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<ScopeSpec>,
        /// 函子库身份（namespace + version）—— 随产物走。
        library: LibraryRef,
    },
}

/// 顺序编排器：声明"先…再…"，编译成 `ClockNode + Binding`。
///
/// 目标/曲线以 `Arc`（曲线 `Clone`）持有，`compile` 可反复调用（不同森林/绑定表）。
#[derive(Clone)]
pub struct Timeline {
    steps: Vec<Step>,
}

impl serde::Serialize for Timeline {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(&self.steps, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Timeline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let steps = <Vec<Step> as serde::Deserialize>::deserialize(deserializer)?;
        Ok(Self { steps })
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// 录一段**表达式驱动**的动画（S3；`SceneEncoder` 专用入口）。
    ///
    /// `clock` 决定表达式里的 `time` 读哪条时间轴（空链 = 全局时钟）；
    /// `predicate` / `reset` 决定生效区间与失效行为（`duration` 的糖在
    /// [`crate::scene_script::record_expression`] 已展开，这里只收结果）。
    pub(crate) fn bind_expr_pending(
        &mut self,
        expr: Expr,
        library: LibraryRef,
        clock: ClockProgram,
        predicate: Option<Expr>,
        reset: ResetBehavior,
        make: fn(ObjectId) -> Arc<dyn Target>,
        handle: usize,
    ) -> &mut Self {
        self.bind_expr_pending_scoped(expr, library, clock, predicate, reset, make, handle, None)
    }

    /// **CC-a**：带作用域的表达式绑定（`scope = None` 即根作用域）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bind_expr_pending_scoped(
        &mut self,
        expr: Expr,
        library: LibraryRef,
        clock: ClockProgram,
        predicate: Option<Expr>,
        reset: ResetBehavior,
        make: fn(ObjectId) -> Arc<dyn Target>,
        handle: usize,
        scope: Option<ScopeSpec>,
    ) -> &mut Self {
        let name = make(ObjectId::DANGLING).name();
        self.steps.push(Step::Expr {
            duration: None,
            target: PendingTarget { make, name, handle },
            expr,
            predicate,
            reset,
            clock,
            scope,
            library,
        });
        self
    }

    /// **G 批**：把 Expr 树里 `Node::Read.object`（脚本局部句柄下标）重写为
    /// `ObjectId` 打包值（`index:u32<<32 | generation:u32`），供 `commit` 编译后
    /// 运行期 [`ze_comb::Mono::read_object`] 解包读组件。
    ///
    /// 产物序列化发生在重写**之前**（persist 走未编译的 `Timeline`），
    /// 故 payload 里 `Read.object` 仍是脚本句柄下标（与 `handle_ref` 惯例一致）。
    pub(crate) fn rewrite_read_objects(&mut self, resolve: &impl Fn(u64) -> u64) {
        for step in &mut self.steps {
            let Step::Expr {
                expr, predicate, ..
            } = step;
            *expr = rewrite_reads(expr, resolve);
            if let Some(predicate) = predicate {
                *predicate = rewrite_reads(predicate, resolve);
            }
        }
    }

    /// 编译进时钟树 + 绑定表：每条表达式绑定把自己的时钟链展开成 `ClockNode` 链。
    ///
    /// `ClockNode::insert` 不可能失败（新 id 不在任何已有祖先链上，见 `clock.rs`），
    /// 失败（理论不可达）即跳过该段。
    pub fn compile(&self, forest: &mut ClockForest, bindings: &mut BindingTable) {
        self.compile_with(&|_| ObjectId::DANGLING, forest, bindings);
    }

    /// [`Timeline::compile`] 的"带句柄解析"版本：待解析目标经 `resolve(handle)` 求
    /// 真实 `ObjectId`，再 `make(id)` 生成 target（场景脚本 `commit` 的落点）。
    pub fn compile_with(
        &self,
        resolve: &impl Fn(usize) -> ObjectId,
        forest: &mut ClockForest,
        bindings: &mut BindingTable,
    ) {
        // **CC-a**：每个作用域一条独立时钟根（按首次出现分配序号）。
        // 根作用域沿用既有语义（`parent = None`），因此无作用域脚本逐位不变。
        let mut scope_clocks: Vec<Option<ze_time::ClockId>> = vec![None];
        for step in &self.steps {
            match step {
                Step::Expr {
                    duration,
                    target,
                    expr,
                    predicate,
                    reset,
                    clock,
                    scope,
                    ..
                } => {
                    let scope_id = bindings
                        .declare_scope(scope.as_ref().map(|spec| spec.path.as_str()).unwrap_or(""));
                    if scope_clocks.len() <= scope_id.0 as usize {
                        scope_clocks.resize(scope_id.0 as usize + 1, None);
                    }
                    let scope_clock = if scope_id.is_root() {
                        None
                    } else if let Some(id) = scope_clocks[scope_id.0 as usize] {
                        Some(id)
                    } else {
                        // 作用域自带时钟：先展开作用域链，再让内部绑定接在它下面。
                        let mut parent = None;
                        if let Some(spec) = scope {
                            for link in &spec.clock.links {
                                let mut node = ClockNode::new(parent);
                                node.local_offset = link.shift;
                                node.rate = link.rate;
                                match forest.insert(node) {
                                    Ok(id) => parent = Some(id),
                                    Err(_) => break,
                                }
                            }
                        }
                        // 空链也要一个独立根节点（不共享根作用域的时间轴）。
                        let root = match parent {
                            Some(id) => Some(id),
                            None => forest.insert(ClockNode::new(None)).ok(),
                        };
                        scope_clocks[scope_id.0 as usize] = root;
                        root
                    };

                    // **B 批**：表达式绑定的时间轴由它自己的时钟决定（空链 = 全局）。
                    // 链在编译期展开成 `ClockNode` 链 ⇒ 运行期仍是同一套 `eval_local`
                    // （与 `Curve` 路径同源，G1 由此消失）。
                    let mut parent = scope_clock;
                    for link in &clock.links {
                        let mut node = ClockNode::new(parent);
                        node.local_offset = link.shift;
                        node.rate = link.rate;
                        match forest.insert(node) {
                            Ok(id) => parent = Some(id),
                            Err(_) => break, // insert 天然不成环；失败即放弃本段
                        }
                    }
                    // 新语义：局部时间**不钳**（predicate / reset 决定区间）；
                    // 历史产物还带着 duration ⇒ 用旧口径的段节点（span + Hold）。
                    let clock_id = match (*duration, parent) {
                        (Some(dur), _) => {
                            let mut node = ClockNode::new(parent);
                            node.span = Some((0.0, dur));
                            node.mode = PlayMode::Hold;
                            forest.insert(node).ok()
                        }
                        (None, Some(id)) => Some(id),
                        (None, None) => forest.insert(ClockNode::new(None)).ok(),
                    };
                    let Some(id) = clock_id else { continue };
                    let target: Arc<dyn Target> = (target.make)(resolve(target.handle));
                    // `bind` 时已用同一个 `Expr` 校验/编译过一次；此处失败说明内部不变量被破坏。
                    // 走 `ExprProgram::compile`：装配期顺带单态化（不可单态化则回落 lowered）。
                    let program = match ExprProgram::compile(expr, ze_func::monomorphizer()) {
                        Ok(program) => program,
                        Err(error) => {
                            debug_assert!(false, "表达式绑定在 commit 时编译失败：{error}");
                            continue;
                        }
                    };
                    // **C 批**：predicate → 局部时间区间集。可分析时生效判定与 Hold
                    // 都是纯函数（AH-06 / AH-07）；不可分析才回退到逐帧求值。
                    //
                    // **阶段 2**：含非时间变量（`frame` / 组件字段读）的区间集**不能**
                    // 用来做逐帧生效判定 —— 它约束的是别的连续量。这类 predicate 仍
                    // 回退到逐帧求值（`active = None`），但 `still_time_report` 会
                    // 显式记为 `non_time_dependency`。
                    let active = predicate.as_ref().and_then(|predicate| {
                        crate::animation::predicate::analyze(predicate)
                            .filter(|set| !set.has_non_time_variable())
                    });
                    let predicate = match predicate {
                        Some(expr) => match CompiledExpr::compile(expr) {
                            Ok(program) => Some(program),
                            Err(error) => {
                                debug_assert!(false, "predicate 在 commit 时编译失败：{error}");
                                continue;
                            }
                        },
                        None => None,
                    };
                    bindings.bind_expr_gated_with_states_scoped(
                        id,
                        program,
                        target,
                        predicate,
                        active,
                        *reset,
                        crate::animation::affine::plans_for_expr(expr).unwrap_or_default(),
                        scope_id,
                    );
                }
            }
        }
    }

    /// **BX**：装配期识别所有 `State`（只读 Expr，产出 side table，**不进产物**）。
    ///
    /// `State.step` 不是关于 `prev` 的仿射 ⇒ 显式失败（不降级 `Replay`）。
    /// 返回的分析结果供门禁 / 诊断使用；运行期仍走同一份折叠循环（重放即定义）。
    pub(crate) fn analyze_states(&self) -> Result<Vec<StateAnalysis>, StateUnsupported> {
        let mut out = Vec::new();
        for step in &self.steps {
            let Step::Expr { target, expr, .. } = step;
            let plans = crate::animation::affine::plans_for_expr(expr).map_err(|error| {
                StateUnsupported {
                    target: target.name.to_string(),
                    reason: error.to_string(),
                }
            })?;
            out.extend(plans.into_iter().map(|plan| StateAnalysis {
                target: target.name.to_string(),
                plan,
            }));
        }
        Ok(out)
    }

    /// 产物里出现的**函子库身份**（去重、按 namespace / version 排序）。
    ///
    /// 输入流的配对校验需要它：同一份 ZSB 的 payload hash 已覆盖库版本，
    /// 但把身份**显式**写进输入流，版本漂移时能给出可读的错误而不是"hash 不符"。
    pub fn libraries(&self) -> Vec<LibraryRef> {
        let mut out: Vec<LibraryRef> = self
            .steps
            .iter()
            .map(|step| match step {
                Step::Expr { library, .. } => library.clone(),
            })
            .collect();
        out.sort_by(|a, b| {
            a.namespace
                .cmp(&b.namespace)
                .then_with(|| a.version.cmp(&b.version))
        });
        out.dedup();
        out
    }

    /// **静止时刻**（D 批）：所有只依赖 Time 的门都失效的最晚时刻。
    ///
    /// 它不是"作品时长"——录得长就是尾部静止，录得短就是截断；渲染多久由
    /// `ExportSpec { fps, frames }` 决定。`None` = **不可推导**（含非时间依赖、
    /// 恒生效的 predicate，或负速率时钟），调用方必须显式说明，不能拿 0 冒充。
    ///
    /// # 可分析性（AH-05 / AH-06 / 阶段 2）
    ///
    /// - `Expr` 段：predicate 分析成**多维连续变量区间集**，取 `time` 投影的
    ///   最大有限右端，再按时钟仿射 `local = rate·t + shift` 反解成全局时间；
    /// - **恒生效**（无 predicate）或区间延伸到 `+∞` ⇒ `None`（永远不会静止）；
    /// - **不可分析**的 predicate（事件 / 非字面量边界）**逐条跳过**
    ///   （AH-05：可分析计入、不可分析跳过；全部不可分析才 `None`）；
    /// - **含非时间变量** ⇒ 跳过并在 [`StillTimeReport`] 里显式记账
    ///   （不再把它误当成"时间恒生效"）。
    pub fn still_time(&self) -> Option<f64> {
        self.still_time_report().at
    }

    /// **阶段 2**：`still_time` 的完整报告 —— 区分"不可分析"与"含非时间依赖"。
    ///
    /// 这两个原因都让 `at` 无法给出确定值，但它们的**修法不同**：
    /// 前者要改 predicate 形状，后者要承认表达式依赖别的连续量。
    pub fn still_time_report(&self) -> StillTimeReport {
        let mut end = 0.0f64;
        let mut analyzable = false;
        let mut report = StillTimeReport::default();
        for step in &self.steps {
            match step {
                Step::Expr {
                    predicate: None, ..
                } => {
                    // 恒生效 ⇒ 画面永远在变，没有静止时刻。
                    report.never_static = true;
                    return report;
                }
                Step::Expr {
                    predicate: Some(predicate),
                    clock,
                    ..
                } => match crate::animation::predicate::analyze(predicate) {
                    None => report.unanalyzable = true,
                    Some(active) if active.has_non_time_variable() => {
                        // 依赖非时间连续量：不能拿时间投影冒充结论。
                        report.non_time_dependency = true;
                    }
                    Some(active) => match active.time_finite_max_end() {
                        // 延伸到 +∞ ⇒ 永远生效。
                        None => {
                            report.never_static = true;
                            return report;
                        }
                        Some(local_end) if local_end.is_finite() => {
                            let Some((rate, shift)) = clock_affine(clock) else {
                                report.unanalyzable = true;
                                continue;
                            };
                            end = end.max((local_end - shift) / rate);
                            analyzable = true;
                        }
                        // 从不生效（空集）⇒ 没有贡献。
                        Some(_) => {}
                    },
                },
            }
        }
        if analyzable {
            report.at = Some(end);
        } else if !report.unanalyzable && !report.non_time_dependency {
            report.at = Some(0.0);
        }
        report
    }

    /// 把 `other` 的步骤**整体追加**到本编排之后（合并语义，§3.3）。
    ///
    /// 5b 之后没有"顺序游标"，"接在后面"必须**显式**落到时钟上：
    ///
    /// - **时间偏移**：`other` 每条绑定的时钟前插一层 `shift = time_base`（本编排的
    ///   静止时刻；不可分析时取 0 并保持调用方语义）；
    /// - **句柄偏移**：`other` 的待解析目标里 `handle` 整体 `+ handle_base`。
    pub fn append_shifted(&mut self, other: &Timeline, handle_base: usize, time_base: f64) {
        for step in &other.steps {
            let Step::Expr {
                duration,
                target,
                expr,
                predicate,
                reset,
                clock,
                scope,
                library,
            } = step;
            // 串行合并时作用域时钟也整体后移；`time_base = 0` 即"并行"（作用域保持独立）。
            let scope = scope.as_ref().map(|spec| {
                ScopeSpec::new(spec.path.clone(), shifted_clock(&spec.clock, time_base))
            });
            self.steps.push(Step::Expr {
                duration: *duration,
                target: PendingTarget {
                    make: target.make,
                    name: target.name,
                    handle: target.handle + handle_base,
                },
                expr: expr.clone(),
                predicate: predicate.clone(),
                reset: *reset,
                clock: shifted_clock(clock, time_base),
                scope,
                library: library.clone(),
            });
        }
    }
}

/// 在时钟链**最外层**前插一层 `shift = base`（`local = parent_local - base`）。
///
/// `base == 0` 时原样返回（不产生多余节点）。
fn shifted_clock(clock: &ClockProgram, base: f64) -> ClockProgram {
    if base == 0.0 {
        return clock.clone();
    }
    let mut links = Vec::with_capacity(clock.links.len() + 1);
    links.push(ze_time::ClockLink {
        shift: base,
        rate: 1.0,
    });
    links.extend_from_slice(&clock.links);
    ClockProgram { links }
}

/// 时钟链 → 仿射映射 `local = rate · t + shift`（只接受 `rate > 0`）。
///
/// 逐层复合：`local' = (local − shiftᵢ) × rateᵢ`。负速率 / 零速率没有唯一解
/// （时间反向或冻结）⇒ `None`，不猜。
fn clock_affine(clock: &ClockProgram) -> Option<(f64, f64)> {
    let mut rate = 1.0f64;
    let mut shift = 0.0f64;
    for link in &clock.links {
        shift = (shift - link.shift) * link.rate;
        rate *= link.rate;
    }
    (rate > 0.0).then_some((rate, shift))
}

/// 递归重写 Expr 树里的 `Node::Read.object`（G 批）。
///
/// `resolve` 把**脚本局部句柄下标**映射成 `ObjectId` 打包值；非 `Read` 节点原样
/// 复制（`Ref` 复用不展开，改写仍逐节点进行，`Ref` 指向的子树是另一份 Expr）。
///
/// 子节点重建统一走 [`Expr::map_children`]（不再自己展开 `Call` / `Select` / `NatRec`）。
fn rewrite_reads(expr: &Expr, resolve: &impl Fn(u64) -> u64) -> Expr {
    if let Some((channel, object)) = expr.node.read() {
        return Expr {
            id: expr.id.clone(),
            node: Node::Read {
                channel: channel.clone(),
                object: resolve(object),
            },
        };
    }
    // 方法调用也带脚本句柄下标（`object`），同一重写规则。
    if let Some((method, object, args)) = expr.node.method() {
        return Expr {
            id: expr.id.clone(),
            node: Node::Method {
                method: method.clone(),
                object: resolve(object),
                args: args.iter().map(|arg| rewrite_reads(arg, resolve)).collect(),
            },
        };
    }
    expr.map_children(|child| rewrite_reads(child, resolve))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一条**表达式**绑定：`clock` + `predicate`（时间区间由时钟与谓词共同决定）。
    fn expr_timeline_with(clock: ClockProgram, predicate: Option<Expr>) -> Timeline {
        use crate::animation::target::target_ctor_by_name;
        let (make, _) = target_ctor_by_name("Circle.radius").expect("目标已登记");
        let mut tl = Timeline::new();
        tl.bind_expr_pending(
            Expr::pure(1.0),
            LibraryRef::default(),
            clock,
            predicate,
            ResetBehavior::Skip,
            make,
            0,
        );
        tl
    }

    /// 编译产物：一条表达式绑定 ⇒ 一个时钟节点、一条绑定；时钟链逐层展开。
    #[test]
    fn compile_expands_clock_chain() {
        let clock = ClockProgram::nested(&ClockProgram::new(), 2.5, 1.0);
        let tl = expr_timeline_with(clock, None);
        let mut forest = ClockForest::new();
        let mut bindings = BindingTable::new();
        tl.compile(&mut forest, &mut bindings);

        assert_eq!(bindings.bindings.len(), 1);
        let node = forest.get(bindings.bindings[0].clock).expect("节点在");
        assert_eq!(node.local_offset, 2.5);
        assert_eq!(node.rate, 1.0);
        assert_eq!(node.span, None, "表达式绑定没有固定段");
        assert_eq!(
            forest.eval_local(bindings.bindings[0].clock, 3.5),
            1.0,
            "局部时间 = 全局 − 2.5"
        );
    }

    /// **CC-a 主判据**：两个作用域 ⇒ 两条独立时钟根；作用域内部的绑定接在各自根下。
    #[test]
    fn scopes_get_independent_clock_roots() {
        use crate::animation::scope::ScopeSpec;

        let mut tl = Timeline::new();
        for (path, shift) in [("panel_a", 1.0), ("panel_b", 5.0)] {
            tl.bind_expr_pending_scoped(
                Expr::pure(1.0),
                LibraryRef::default(),
                ClockProgram::new(),
                None,
                ResetBehavior::Skip,
                crate::animation::target::target_ctor_by_name("Circle.radius")
                    .unwrap()
                    .0,
                0,
                Some(ScopeSpec::new(
                    path,
                    ClockProgram::nested(&ClockProgram::new(), shift, 1.0),
                )),
            );
        }
        let mut forest = ClockForest::new();
        let mut bindings = BindingTable::new();
        tl.compile(&mut forest, &mut bindings);

        assert_eq!(
            bindings.scopes(),
            &["".to_string(), "panel_a".to_string(), "panel_b".to_string()]
        );
        assert_eq!(bindings.bindings[0].scope, crate::animation::ScopeId(1));
        assert_eq!(bindings.bindings[1].scope, crate::animation::ScopeId(2));

        let a = bindings.bindings[0].clock;
        let b = bindings.bindings[1].clock;
        // 两条时间轴各自求值（互不影响）。
        assert!((forest.eval_local(a, 5.0) - 4.0).abs() < 1e-9);
        assert!((forest.eval_local(b, 5.0) - 0.0).abs() < 1e-9);
        // 两条时钟节点不同，且各自携带作用域的零点偏移（独立时间轴）。
        assert_ne!(a, b);
        assert!((forest.get(a).unwrap().local_offset - 1.0).abs() < 1e-9);
        assert!((forest.get(b).unwrap().local_offset - 5.0).abs() < 1e-9);
    }

    /// **CC-a 回归**：无作用域脚本仍只有根作用域，时钟语义与改前一致。
    #[test]
    fn root_scope_keeps_global_clock() {
        let tl = expr_timeline_with(ClockProgram::new(), None);
        let mut forest = ClockForest::new();
        let mut bindings = BindingTable::new();
        tl.compile(&mut forest, &mut bindings);
        assert_eq!(bindings.scopes(), &["".to_string()]);
        assert_eq!(bindings.bindings[0].scope, crate::animation::ScopeId::ROOT);
        assert_eq!(forest.eval_local(bindings.bindings[0].clock, 7.5), 7.5);
    }

    /// **合并**：`append_shifted` 把后一段的时钟整体后移（不再有顺序游标）。
    #[test]
    fn append_shifted_moves_clock_by_time_base() {
        let mut a = Timeline::new();
        a.append_shifted(&expr_timeline_with(ClockProgram::new(), None), 0, 2.0);
        let mut forest = ClockForest::new();
        let mut bindings = BindingTable::new();
        a.compile(&mut forest, &mut bindings);
        let node = bindings.bindings[0].clock;
        assert_eq!(forest.eval_local(node, 2.0), 0.0);
        assert_eq!(forest.eval_local(node, 5.0), 3.0);
    }

    // ─── 阶段 2：still_time 的多维语义 ─────────────────────────────────

    fn expr_timeline(predicate: Expr) -> Timeline {
        use crate::animation::target::target_ctor_by_name;
        let (make, _) = target_ctor_by_name("Circle.radius").expect("目标已登记");
        let mut tl = Timeline::new();
        tl.bind_expr_pending(
            Expr::pure(1.0),
            LibraryRef::default(),
            ClockProgram::default(),
            Some(predicate),
            ResetBehavior::Skip,
            make,
            0,
        );
        tl
    }

    fn compare(op: &str, left: Expr, right: Expr) -> Expr {
        Expr::call(ze_comb::OpRef::new("zela.math", op), [left, right])
    }

    /// 只依赖时间的窗口 ⇒ `at = Some(...)`，没有"非时间依赖"标记。
    #[test]
    fn still_time_report_distinguishes_a_time_window() {
        let predicate = compare(
            "lt",
            Expr::channel(ze_comb::ChannelRef::builtin("time")),
            Expr::pure(4.0),
        );
        let report = expr_timeline(predicate).still_time_report();
        assert_eq!(report.at, Some(4.0));
        assert!(!report.unanalyzable);
        assert!(!report.non_time_dependency);
    }

    /// **阶段 2 验收**：依赖 `frame` ⇒ `at = None` 且 `non_time_dependency = true`
    /// （不是"不可分析"，也不是 0）。
    #[test]
    fn still_time_report_names_non_time_dependency() {
        let frame = Expr::channel(ze_comb::ChannelRef::builtin("frame"));
        let predicate = Expr::call(
            ze_comb::OpRef::new("zela.math", "and"),
            [
                compare("lt", frame.clone(), Expr::pure(4.0)),
                compare("gt", frame, Expr::pure(1.0)),
            ],
        );
        let report = expr_timeline(predicate).still_time_report();
        assert_eq!(report.at, None, "非时间依赖 ⇒ 不得用 0 冒充");
        assert!(report.non_time_dependency);
        assert!(!report.unanalyzable, "它可分析，只是不是时间");
        assert!(!report.never_static);
    }

    /// 非字面量边界（`time` 与 `time` 比较）⇒ `unanalyzable`
    /// （与"非时间依赖"分开）。
    #[test]
    fn still_time_report_names_unanalyzable_shapes() {
        let time = Expr::channel(ze_comb::ChannelRef::builtin("time"));
        let predicate = compare("lt", time.clone(), time);
        let report = expr_timeline(predicate).still_time_report();
        assert_eq!(report.at, None);
        assert!(report.unanalyzable);
        assert!(!report.non_time_dependency);
    }
}
