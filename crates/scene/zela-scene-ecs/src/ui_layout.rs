//! **CL-c / CL-R**：控件声明树 → Euler 切片 → 布局策略 → `Frame`。
//!
//! # 为什么驱动放在这一层
//!
//! 关系编译在 `ze-graph-algo`、控件契约与布局策略在 `ze-interact`，两者都不该知道对方；
//! 只有本层（场景 ECS）同时依赖两边，所以「编译 + 布局」的驱动放这里。
//!
//! # 两个策略，同一条链
//!
//! ```text
//! 回溯测量：按 tin 逆序（子先于父）汇总
//! 前推分配：按 tin 正序（父先于子）下发 bounds —— 迭代，零树递归
//! ```
//!
//! 空约束声明 ⇒ 默认策略（上面这条两遍扫描）；声明带约束 ⇒ 交给
//! `ze_interact::solver` 的策略表（`two-pass` 对非空声明显式 `Unsupported`，
//! `kasuari` 解跨容器关系）。**不静默降级**。
//!
//! 容器不持有子对象：层级由 `TreeIndex` 给出，两遍都只在一段 `Vec` 上跑。
//! revision 来自 `solver::plan_revision`（声明 + `StrategyId` + 约束集 + `LayoutTokens`），
//! 是**两条布局链唯一的 revision 来源**；可用空间（`root`）也并进声明摘要。
//!
//! # `Frame` 三列平行
//!
//! `geometry` / `parent` / `semantics` / `text` **同下标**：没有文本的控件在
//! `text` 里放一个空 `TextRun` 占位。这样 `diff_frames` 按 run 下标对齐全等于按控件
//! 下标对齐，增删带文本控件不会再把整体下标前移（CL 缺口 ④）。

use std::collections::{BTreeMap, BTreeSet};

use ze_graph_algo::relation::{self, RelationError, RelationPhase, RelationSource, TreeIndex};
use ze_interact::solver::{
    self, ConstraintDecl, LayoutEngine, SolveError, StrategyId, plan_revision,
};
use ze_interact::widget::{DrawEnv, LayoutTokens, MeasureCtx, WidgetDecl, WidgetId, WidgetRuntime};
use ze_interact::{Constraints, Frame, LayoutId, PlacedRect, Point, Rect, SemanticNode, Size};

/// 布局失败（全部显式；不返回「空布局」当成功）。
#[derive(Debug, Clone)]
pub enum UiLayoutError {
    /// 声明本身不合法（重复 id / 混用策略 / 未注册规格）。
    Declaration(String),
    /// 关系编译失败（环 / 多父 / 未知父）。
    Relation(String),
    /// 策略求解失败（含冲突项路径）。
    Solve(SolveError),
}

impl std::fmt::Display for UiLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Declaration(message) => {
                write!(f, "布局声明不合法：{message}")
            }
            Self::Relation(message) => write!(f, "关系编译失败：{message}"),
            Self::Solve(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for UiLayoutError {}

impl From<SolveError> for UiLayoutError {
    fn from(error: SolveError) -> Self {
        Self::Solve(error)
    }
}

/// 声明树的最小只读视图。
struct DeclTree<'a> {
    nodes: Vec<WidgetId>,
    parents: &'a BTreeMap<WidgetId, WidgetId>,
}

impl RelationSource for DeclTree<'_> {
    type Id = WidgetId;

    fn nodes(&self) -> Vec<WidgetId> {
        self.nodes.clone()
    }

    fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.parents.get(&id).copied()
    }

    fn phase(&self, _id: WidgetId) -> RelationPhase {
        RelationPhase::Assembly
    }
}

/// 布局结果（派生态；`frame` 可进回执，`bounds` 供绘制）。
#[derive(Debug, Clone)]
pub struct LayoutResult {
    pub frame: Frame,
    pub bounds: BTreeMap<WidgetId, Rect>,
    pub revision: u64,
    pub strategy: StrategyId,
}

/// 声明树的布局 revision（声明哈希 + 策略 + 约束集 + 布局 token）。
///
/// 可用空间不进这里：这是「声明摘要」；`layout_widgets` 会把 `root` 并进去再求解。
pub fn layout_revision(decls: &[WidgetDecl], layout: &LayoutTokens) -> Result<u64, UiLayoutError> {
    let (nodes, parents) = index_decls(decls)?;
    let tree = DeclTree {
        nodes,
        parents: &parents,
    };
    let (strategy, constraints) = plan_of(decls)?;
    Ok(plan_revision(
        relation::revision(&tree),
        &strategy,
        &constraints,
        layout,
    ))
}

fn index_decls(
    decls: &[WidgetDecl],
) -> Result<(Vec<WidgetId>, BTreeMap<WidgetId, WidgetId>), UiLayoutError> {
    let mut nodes = Vec::with_capacity(decls.len());
    let mut parents = BTreeMap::new();
    for decl in decls {
        if nodes.contains(&decl.id) {
            return Err(UiLayoutError::Declaration(format!(
                "重复的控件 id：{:?}",
                decl.id
            )));
        }
        nodes.push(decl.id);
        if let Some(parent) = decl.parent {
            parents.insert(decl.id, parent);
        }
    }
    nodes.sort_unstable();
    Ok((nodes, parents))
}

/// 一棵树的策略与约束集：**混用策略是显式错误**（两个求解器不得互相改对方 bounds）。
fn plan_of(decls: &[WidgetDecl]) -> Result<(StrategyId, ConstraintDecl), UiLayoutError> {
    let strategies: BTreeSet<StrategyId> = decls.iter().map(|decl| decl.strategy.clone()).collect();
    let strategy = match strategies.len() {
        0 => StrategyId::default(),
        1 => strategies.into_iter().next().expect("恰好一个"),
        _ => {
            return Err(UiLayoutError::Declaration(format!(
                "同一棵树混用布局策略：{}",
                strategies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    };
    let mut constraints = ConstraintDecl::new();
    for decl in decls {
        constraints.extend(&decl.constraints);
    }
    Ok((strategy, constraints))
}

/// `Rect` 的位模式摘要（可用空间参与求解 ⇒ 必须进 revision）。
fn rect_hash(rect: Rect) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for value in [rect.min.x, rect.min.y, rect.max.x, rect.max.y] {
        for byte in value.to_bits().to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 一帧布局：声明树 → 策略 → `Frame`。
pub fn layout_widgets(
    decls: &[WidgetDecl],
    runtime: &mut WidgetRuntime,
    engine: &mut LayoutEngine,
    root: Rect,
    layout: &LayoutTokens,
    env: &DrawEnv,
) -> Result<LayoutResult, UiLayoutError> {
    let (nodes, parents) = index_decls(decls)?;
    runtime.rebuild(decls).map_err(UiLayoutError::Declaration)?;
    let tree: TreeIndex<WidgetId> = relation::compile_tree(&DeclTree {
        nodes: nodes.clone(),
        parents: &parents,
    })
    .map_err(|error: RelationError<WidgetId>| UiLayoutError::Relation(error.to_string()))?;
    let (strategy, constraints) = plan_of(decls)?;
    let revision = plan_revision(
        relation::revision(&DeclTree {
            nodes: nodes.clone(),
            parents: &parents,
        }) ^ rect_hash(root),
        &strategy,
        &constraints,
        layout,
    );

    let bounds = if constraints.is_empty() && strategy == StrategyId::default() {
        two_pass(&tree, &parents, runtime, root, layout, env)
    } else {
        let available = Constraints::new(root.width(), root.height());
        let placed = engine.solve(revision, &strategy, &constraints, available)?;
        let mut bounds: BTreeMap<WidgetId, Rect> = BTreeMap::new();
        for entry in placed {
            bounds.insert(entry.id, entry.rect);
        }
        for decl in decls {
            if !bounds.contains_key(&decl.id) {
                return Err(UiLayoutError::Solve(SolveError::new(
                    &strategy,
                    solver::SolveErrorKind::Unplaced(decl.id),
                    constraints.iter().map(ToString::to_string),
                )));
            }
        }
        bounds
    };

    // dyn 表拿到最终 bounds（`draw` / `text_run` 都读自己的 bounds）。
    for (&id, &rect) in &bounds {
        if let Some(widget) = runtime.get_mut(id) {
            widget.set_bounds(rect);
        }
    }

    Ok(LayoutResult {
        frame: frame_of(&tree, &parents, &bounds, runtime),
        bounds,
        revision,
        strategy,
    })
}

/// **两遍扫描**：回溯测量（tin 逆序）→ 前推分配（tin 正序，迭代，零递归）。
fn two_pass(
    tree: &TreeIndex<WidgetId>,
    parents: &BTreeMap<WidgetId, WidgetId>,
    runtime: &WidgetRuntime,
    root: Rect,
    layout: &LayoutTokens,
    env: &DrawEnv,
) -> BTreeMap<WidgetId, Rect> {
    let mut children: BTreeMap<WidgetId, Vec<WidgetId>> = BTreeMap::new();
    for tin in 0..tree.len() as u32 {
        if let Some(id) = tree.node_at(tin) {
            children.entry(id).or_default();
        }
    }
    for (&child, &parent) in parents {
        children.entry(parent).or_default().push(child);
    }
    for list in children.values_mut() {
        list.sort_unstable();
    }

    // 回溯：子先于父。
    let mut sizes: BTreeMap<WidgetId, Size> = BTreeMap::new();
    let constraints = Constraints::new(root.width(), root.height());
    for tin in (0..tree.len() as u32).rev() {
        let Some(id) = tree.node_at(tin) else {
            continue;
        };
        let own = runtime
            .get(id)
            .map(|widget| {
                widget.measure(&MeasureCtx {
                    constraints,
                    layout,
                    env,
                })
            })
            .unwrap_or_else(|| Size::new(constraints.max.width, layout.row_height));
        let child_ids = children.get(&id).cloned().unwrap_or_default();
        let size = if child_ids.is_empty() {
            own
        } else {
            let mut width = own.width;
            let mut height = layout.padding * 2.0;
            for (index, child) in child_ids.iter().enumerate() {
                let child_size = sizes.get(child).copied().unwrap_or_default();
                width = width.max(child_size.width);
                height += child_size.height;
                if index > 0 {
                    height += layout.gap;
                }
            }
            Size::new(width, height)
        };
        sizes.insert(id, size);
    }

    // 前推：父先于子（Euler 前序 ⇒ 父的 bounds 一定已就绪）。
    let mut bounds: BTreeMap<WidgetId, Rect> = BTreeMap::new();
    let mut child_cursor: BTreeMap<WidgetId, f64> = BTreeMap::new();
    let mut root_cursor = root.min.y + layout.padding;
    for tin in 0..tree.len() as u32 {
        let Some(id) = tree.node_at(tin) else {
            continue;
        };
        let size = sizes.get(&id).copied().unwrap_or_default();
        let rect = match tree.parent(id) {
            None => {
                let width = size
                    .width
                    .min((root.width() - layout.padding * 2.0).max(0.0));
                let rect = Rect::from_min_size(
                    Point::new(root.min.x + layout.padding, root_cursor),
                    width.max(0.0),
                    size.height.max(0.0),
                );
                root_cursor += size.height.max(0.0) + layout.gap;
                rect
            }
            Some(parent) => {
                let parent_rect = bounds.get(&parent).copied().unwrap_or(root);
                let y = child_cursor
                    .get(&parent)
                    .copied()
                    .unwrap_or(parent_rect.min.y + layout.padding);
                // 与 `BoxLayout` 同口径：宽度取「可用宽 - 两倍内边距」（否则两条链错位）。
                let width = size
                    .width
                    .min((parent_rect.width() - layout.padding * 2.0).max(0.0));
                let rect = Rect::from_min_size(
                    Point::new(parent_rect.min.x + layout.padding, y),
                    width.max(0.0),
                    size.height.max(0.0),
                );
                child_cursor.insert(parent, rect.max.y + layout.gap);
                rect
            }
        };
        child_cursor.insert(id, rect.min.y + layout.padding);
        bounds.insert(id, rect);
    }
    bounds
}

/// `Frame`：三列（几何 / 父 / 语义 / 文本）**同下标**。
fn frame_of(
    tree: &TreeIndex<WidgetId>,
    parents: &BTreeMap<WidgetId, WidgetId>,
    bounds: &BTreeMap<WidgetId, Rect>,
    runtime: &WidgetRuntime,
) -> Frame {
    let mut frame = Frame::new();
    for tin in 0..tree.len() as u32 {
        let Some(id) = tree.node_at(tin) else {
            continue;
        };
        let Some(rect) = bounds.get(&id).copied() else {
            continue;
        };
        frame.geometry.push(PlacedRect { id, rect });
        frame
            .parent
            .push(parents.get(&id).copied().map(|parent| LayoutId(parent.0)));
        match runtime.get(id) {
            Some(widget) => {
                let state = widget.state();
                let mut node = SemanticNode::new(state.role.clone(), state.name.clone(), rect);
                if let Some(value) = state.value {
                    node = node.with_value(value);
                }
                frame.semantics.push(node.focusable(state.focusable));
                // 没有文本也占位：下标与 `geometry` 对齐（CL 缺口 ④）。
                frame.text.push(widget.text_run().unwrap_or_default());
            }
            None => {
                frame.semantics.push(SemanticNode::new("", "", rect));
                frame.text.push(Default::default());
            }
        }
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_interact::Point as IPoint;
    use ze_interact::solver::{Constraint, ConstraintOp, LayoutVar};
    use ze_interact::widget::{BuiltinWidget, GlyphOutline, WidgetSpecId};

    fn runtime() -> WidgetRuntime {
        let mut runtime = WidgetRuntime::new();
        runtime.register("slider", |props| {
            Box::new(BuiltinWidget::new(props.clone()))
        });
        runtime.register("label", |props| Box::new(BuiltinWidget::new(props.clone())));
        runtime
    }

    fn decl(id: u32, parent: Option<u32>, spec: &str) -> WidgetDecl {
        WidgetDecl {
            id: LayoutId(id),
            spec: WidgetSpecId::new(spec),
            parent: parent.map(LayoutId),
            label: format!("w{id}"),
            role: "slider".to_string(),
            value: 0.5,
            ..WidgetDecl::default()
        }
    }

    fn glyphs() -> Vec<GlyphOutline> {
        vec![
            GlyphOutline::from_polyline(
                &[
                    IPoint::new(0.0, 0.0),
                    IPoint::new(4.0, 0.0),
                    IPoint::new(4.0, 8.0),
                ],
                false,
            ),
            GlyphOutline::from_polyline(
                &[
                    IPoint::new(5.0, 0.0),
                    IPoint::new(9.0, 0.0),
                    IPoint::new(9.0, 8.0),
                ],
                false,
            ),
        ]
    }

    fn frame() -> Frame {
        // 带字形的控件：文本层必须填实。
        let decls = vec![decl(0, None, "label"), decl(1, Some(0), "slider")];
        let mut runtime2 = WidgetRuntime::new();
        runtime2.register("label", |props| {
            let mut props = props.clone();
            props.glyphs = glyphs();
            Box::new(BuiltinWidget::new(props))
        });
        runtime2.register("slider", |props| {
            Box::new(BuiltinWidget::new(props.clone()))
        });
        let layout = LayoutTokens {
            row_height: 20.0,
            gap: 4.0,
            padding: 2.0,
            font_size: 12.0,
            knob_radius: 6.0,
        };
        let env = DrawEnv::default();
        let result = layout_widgets(
            &decls,
            &mut runtime2,
            &mut LayoutEngine::new(),
            Rect::from_min_size(IPoint::new(0.0, 0.0), 100.0, 200.0),
            &layout,
            &env,
        )
        .expect("布局成功");
        result.frame
    }

    /// **判据 3**：生产路径填出 `Frame.parent` 与 `Frame.text`，且三列同下标。
    #[test]
    fn production_layout_fills_parent_and_text() {
        let frame = frame();
        assert_eq!(frame.geometry.len(), 2);
        assert_eq!(
            frame.parent,
            vec![None, Some(LayoutId(0))],
            "子控件的父索引"
        );
        assert_eq!(frame.text.len(), frame.geometry.len(), "文本列与几何同长");
        assert_eq!(
            frame.semantics.len(),
            frame.geometry.len(),
            "语义列与几何同长"
        );
        assert_eq!(frame.text[0].chars.len(), 2, "根控件的文本");
        assert!(frame.text[1].chars.is_empty(), "无文本控件占位而非省略");
    }

    /// **判据 4**：等价声明不同插入顺序 ⇒ `Frame` 逐位不变。
    #[test]
    fn layout_is_insertion_order_independent() {
        let layout = LayoutTokens::default();
        let env = DrawEnv::default();
        let root = Rect::from_min_size(IPoint::new(0.0, 0.0), 100.0, 200.0);
        let forward = vec![decl(0, None, "slider"), decl(1, Some(0), "slider")];
        let reversed = vec![decl(1, Some(0), "slider"), decl(0, None, "slider")];
        let mut a = runtime();
        let mut b = runtime();
        let frame_a = layout_widgets(
            &forward,
            &mut a,
            &mut LayoutEngine::new(),
            root,
            &layout,
            &env,
        )
        .unwrap()
        .frame;
        let frame_b = layout_widgets(
            &reversed,
            &mut b,
            &mut LayoutEngine::new(),
            root,
            &layout,
            &env,
        )
        .unwrap()
        .frame;
        assert_eq!(frame_a, frame_b);
    }

    /// **判据 7**：布局 token 变 ⇒ revision 变。
    #[test]
    fn layout_tokens_are_part_of_the_revision() {
        let decls = vec![decl(0, None, "slider")];
        let base = LayoutTokens::default();
        let mut changed = base;
        changed.row_height = 30.0;
        assert_ne!(
            layout_revision(&decls, &base).unwrap(),
            layout_revision(&decls, &changed).unwrap()
        );
        // 换策略也必须变。
        let mut restrategied = decls.clone();
        restrategied[0].strategy = StrategyId::new(StrategyId::KASUARI);
        assert_ne!(
            layout_revision(&decls, &base).unwrap(),
            layout_revision(&restrategied, &base).unwrap()
        );
    }

    /// 跨容器约束：A 在左、B 在右，`A.width == B.width + 2*padding`，B 紧跟 A。
    fn constrained_decls() -> Vec<WidgetDecl> {
        let (a, b) = (LayoutId(0), LayoutId(1));
        let padding = 8.0;
        let mut constraints = ConstraintDecl::new();
        constraints.insert(Constraint::var(LayoutVar::X(a), ConstraintOp::Eq, 0.0));
        constraints.insert(Constraint::var(LayoutVar::Y(a), ConstraintOp::Eq, 0.0));
        constraints.insert(Constraint::var(
            LayoutVar::Height(a),
            ConstraintOp::Eq,
            40.0,
        ));
        constraints.insert(Constraint::var(
            LayoutVar::Width(b),
            ConstraintOp::Eq,
            100.0,
        ));
        constraints.insert(Constraint::var(LayoutVar::Y(b), ConstraintOp::Eq, 0.0));
        constraints.insert(Constraint::var(
            LayoutVar::Height(b),
            ConstraintOp::Eq,
            40.0,
        ));
        constraints.insert(Constraint::new(
            vec![
                ze_interact::solver::LinearTerm::new(LayoutVar::Width(a), 1.0),
                ze_interact::solver::LinearTerm::new(LayoutVar::Width(b), -1.0),
            ],
            ConstraintOp::Eq,
            padding * 2.0,
        ));
        constraints.insert(Constraint::new(
            vec![
                ze_interact::solver::LinearTerm::new(LayoutVar::X(b), 1.0),
                ze_interact::solver::LinearTerm::new(LayoutVar::Width(a), -1.0),
            ],
            ConstraintOp::Eq,
            0.0,
        ));
        let mut first = decl(0, None, "slider");
        first.constraints = constraints;
        vec![first, decl(1, None, "slider")]
    }

    /// **判据 5**：两条布局链合流后，生产驱动对跨容器声明显式给出失败 / 唯一解。
    #[test]
    fn constrained_declaration_is_explicit_about_the_default_strategy() {
        let mut runtime = runtime();
        let layout = LayoutTokens::default();
        let env = DrawEnv::default();
        let error = layout_widgets(
            &constrained_decls(),
            &mut runtime,
            &mut LayoutEngine::new(),
            Rect::from_min_size(IPoint::new(0.0, 0.0), 400.0, 400.0),
            &layout,
            &env,
        )
        .expect_err("默认策略不得假装能解跨容器约束");
        assert!(
            matches!(error, UiLayoutError::Solve(ref solve) if solve.kind == solver::SolveErrorKind::Unsupported),
            "实际：{error}"
        );
    }

    /// **判据 5 / 6**：kasuari 给唯一解，且 60 帧只求解 1 次。
    #[test]
    fn kasuari_layouts_the_cross_container_declaration_once() {
        let mut decls = constrained_decls();
        for decl in &mut decls {
            decl.strategy = StrategyId::new(StrategyId::KASUARI);
        }
        let mut runtime = runtime();
        let layout = LayoutTokens::default();
        let env = DrawEnv::default();
        let mut engine = LayoutEngine::new();
        let root = Rect::from_min_size(IPoint::new(0.0, 0.0), 400.0, 400.0);

        let first = layout_widgets(&decls, &mut runtime, &mut engine, root, &layout, &env)
            .expect("有唯一解");
        assert_eq!(first.bounds[&LayoutId(0)].width(), 116.0);
        assert_eq!(first.bounds[&LayoutId(1)].min.x, 116.0);
        for _ in 0..59 {
            let again = layout_widgets(&decls, &mut runtime, &mut engine, root, &layout, &env)
                .expect("缓存命中");
            assert_eq!(first.frame, again.frame, "同一 revision 的帧必须逐位相同");
        }
        assert_eq!(engine.solves(), 1, "60 帧只有 1 次求解");
    }
}
