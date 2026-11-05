//! **CL-c**：控件声明树 → Euler 切片 → 两遍扫描 → `Frame`。
//!
//! # 为什么驱动放在这一层
//!
//! 关系编译在 `ze-graph-algo`、控件契约在 `ze-interact`，两者都不该知道对方；
//! 只有本层（场景 ECS）同时依赖两边，所以「编译 + 布局」的驱动放这里。
//!
//! # 两遍扫描，零树递归
//!
//! ```text
//! 回溯测量：按 tin 逆序（子先于父）沿连续切片汇总
//! 前推分配：按 tin 正序（父先于子）把 bounds 下发
//! ```
//!
//! 容器不持有子对象：层级由 `TreeIndex` 给出，两遍都只在一段 `Vec` 上跑。
//! revision = 声明哈希 + `LayoutTokens`（改 paint token 不重排）。

use std::collections::BTreeMap;

use ze_graph_algo::relation::{self, RelationPhase, RelationSource, TreeIndex};
use ze_interact::widget::{
    DrawEnv, LayoutTokens, MeasureCtx, WidgetDecl, WidgetId, WidgetRuntime,
};
use ze_interact::{Constraints, Frame, LayoutId, PlacedRect, Point, Rect, SemanticNode, Size};

use ze_graph_algo::relation::RelationError;

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
pub struct LayoutResult {
    pub frame: Frame,
    pub bounds: BTreeMap<WidgetId, Rect>,
    pub revision: u64,
}

/// 声明树的布局 revision（声明哈希 + 布局 token）。
pub fn layout_revision(decls: &[WidgetDecl], layout: &LayoutTokens) -> Result<u64, String> {
    let (nodes, parents) = index_decls(decls)?;
    let tree = DeclTree {
        nodes,
        parents: &parents,
    };
    Ok(relation::revision(&tree) ^ layout.revision())
}

fn index_decls(
    decls: &[WidgetDecl],
) -> Result<(Vec<WidgetId>, BTreeMap<WidgetId, WidgetId>), String> {
    let mut nodes = Vec::with_capacity(decls.len());
    let mut parents = BTreeMap::new();
    for decl in decls {
        if nodes.contains(&decl.id) {
            return Err(format!("重复的控件 id：{:?}", decl.id));
        }
        nodes.push(decl.id);
        if let Some(parent) = decl.parent {
            parents.insert(decl.id, parent);
        }
    }
    nodes.sort_unstable();
    Ok((nodes, parents))
}

/// **两遍扫描**：测量（回溯）→ 分配（前推）→ `Frame`。
pub fn layout_widgets(
    decls: &[WidgetDecl],
    runtime: &mut WidgetRuntime,
    root: Rect,
    layout: &LayoutTokens,
    env: &DrawEnv,
) -> Result<LayoutResult, String> {
    let (nodes, parents) = index_decls(decls)?;
    // 运行期 dyn 表由声明重建（按 `WidgetId` 升序；与插入顺序无关）。
    runtime.rebuild(decls)?;
    let tree: TreeIndex<WidgetId> = relation::compile_tree(&DeclTree {
        nodes: nodes.clone(),
        parents: &parents,
    })
    .map_err(|error: RelationError<WidgetId>| error.to_string())?;

    // 子表（按 id 升序；稳定序）。
    let mut children: BTreeMap<WidgetId, Vec<WidgetId>> = BTreeMap::new();
    for &id in &nodes {
        children.entry(id).or_default();
    }
    for (&child, &parent) in &parents {
        children.entry(parent).or_default().push(child);
    }
    for list in children.values_mut() {
        list.sort_unstable();
    }

    // 回溯：按 tin 逆序测量（子先于父）。
    let mut sizes: BTreeMap<WidgetId, Size> = BTreeMap::new();
    let constraints = Constraints::new(root.width(), root.height());
    for tin in (0..tree.len() as u32).rev() {
        let Some(id) = tree.node_at(tin) else {
            continue;
        };
        let own = runtime
            .get(id)
            .map(|widget| widget.measure(&MeasureCtx {
                constraints,
                layout,
                env,
            }))
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

    // 前推：根按声明序纵向排列，再逐层下发。
    let mut bounds: BTreeMap<WidgetId, Rect> = BTreeMap::new();
    let mut y = root.min.y;
    for tin in 0..tree.len() as u32 {
        let Some(id) = tree.node_at(tin) else {
            continue;
        };
        if tree.parent(id).is_some() {
            continue;
        }
        let size = sizes.get(&id).copied().unwrap_or_default();
        let rect = Rect::from_min_size(
            Point::new(root.min.x + layout.padding, y + layout.padding),
            size.width.max(0.0),
            size.height.max(0.0),
        );
        assign(id, rect, &children, &sizes, runtime, &mut bounds, layout);
        y += size.height + layout.gap;
    }

    // `Frame`：几何 / 父索引 / 语义 / 文本。
    let mut frame = Frame::new();
    for (&id, &rect) in &bounds {
        frame.geometry.push(PlacedRect { id, rect });
        frame.parent.push(
            parents
                .get(&id)
                .copied()
                .map(|parent| LayoutId(parent.0)),
        );
        if let Some(widget) = runtime.get(id) {
            let state = widget.state();
            let mut node = SemanticNode::new(state.role.clone(), state.name.clone(), rect);
            if let Some(value) = state.value {
                node = node.with_value(value);
            }
            frame.semantics.push(node.focusable(state.focusable));
            if let Some(run) = widget.text_run() {
                frame.text.push(run);
            }
        }
    }

    Ok(LayoutResult {
        frame,
        bounds,
        revision: relation::revision(&DeclTree {
            nodes,
            parents: &parents,
        }) ^ layout.revision(),
    })
}

fn assign(
    id: WidgetId,
    rect: Rect,
    children: &BTreeMap<WidgetId, Vec<WidgetId>>,
    sizes: &BTreeMap<WidgetId, Size>,
    runtime: &mut WidgetRuntime,
    bounds: &mut BTreeMap<WidgetId, Rect>,
    layout: &LayoutTokens,
) {
    bounds.insert(id, rect);
    if let Some(widget) = runtime.get_mut(id) {
        widget.set_bounds(rect);
    }
    let Some(child_ids) = children.get(&id) else {
        return;
    };
    if child_ids.is_empty() {
        return;
    }
    let mut y = rect.min.y + layout.padding;
    for child in child_ids {
        let size = sizes.get(child).copied().unwrap_or_default();
        let child_rect = Rect::from_min_size(
            Point::new(rect.min.x + layout.padding, y),
            size.width.max(0.0),
            size.height.max(0.0),
        );
        assign(*child, child_rect, children, sizes, runtime, bounds, layout);
        y += size.height + layout.gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_interact::widget::{BuiltinWidget, GlyphOutline, WidgetSpecId};
    use ze_interact::Point as IPoint;

    fn runtime() -> WidgetRuntime {
        let mut runtime = WidgetRuntime::new();
        runtime.register("slider", |props| Box::new(BuiltinWidget::new(props.clone())));
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
        }
    }

    fn glyphs() -> Vec<GlyphOutline> {
        vec![
            GlyphOutline {
                points: vec![IPoint::new(0.0, 0.0), IPoint::new(4.0, 0.0), IPoint::new(4.0, 8.0)],
                closed: false,
            },
            GlyphOutline {
                points: vec![IPoint::new(5.0, 0.0), IPoint::new(9.0, 0.0), IPoint::new(9.0, 8.0)],
                closed: false,
            },
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
        runtime2.register("slider", |props| Box::new(BuiltinWidget::new(props.clone())));
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
            Rect::from_min_size(IPoint::new(0.0, 0.0), 100.0, 200.0),
            &layout,
            &env,
        )
        .expect("布局成功");
        result.frame
    }

    /// **判据 10**：生产路径填出 `Frame.parent` 与 `Frame.text`。
    #[test]
    fn production_layout_fills_parent_and_text() {
        let frame = frame();
        assert_eq!(frame.geometry.len(), 2);
        assert_eq!(frame.parent, vec![None, Some(LayoutId(0))], "子控件的父索引");
        assert!(!frame.text.is_empty(), "文本层必须有数据");
        assert_eq!(frame.text[0].chars.len(), 2);
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
        let frame_a = layout_widgets(&forward, &mut a, root, &layout, &env)
            .unwrap()
            .frame;
        let frame_b = layout_widgets(&reversed, &mut b, root, &layout, &env)
            .unwrap()
            .frame;
        assert_eq!(frame_a, frame_b);
    }

    /// **判据 7**：布局 token 变 ⇒ revision 变；paint 不进 revision。
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
    }
}
