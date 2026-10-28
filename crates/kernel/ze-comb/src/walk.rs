// walk.rs — **词法树遍历器**（`Expr` 是树；共享 / 环由 [`crate::graph_view`] 处理）。
//
// # 为什么需要它
//
// 本 crate 从第一版就有 `Node::children()`，但每个 pass 仍各自写递归 + 各自
// `match &expr.node`。多 pass 架构缺的那一半正是「遍历」：子节点枚举有了，
// **进入 / 离开的骨架**没有 ⇒ 11 处手写遍历各写一遍，漂移只是时间问题。
//
// 这里给出唯一一份骨架：前序 `enter`（可剪枝）+ 后序 `leave`。
// 树遍历与图遍历分工明确：
// - [`crate::ExprGraphView`] + `ze-graph-core::traverse`：`Def` 共享 / `Ref` 边（DAG 语义）；
// - 本模块：**词法**嵌套（`NatRec.step` 的作用域这类"看起来在哪"的问题）。
//
// # 硬约束
//
// 不引入 `HashMap`；遍历顺序由 `children()` 的声明顺序决定（`Call.args` 顺序即语义顺序）。

use crate::{Expr, Node};

/// 词法树遍历者。
///
/// `Error` 让校验类 pass 能中途失败（不想失败的 pass 用 [`std::convert::Infallible`]）。
pub trait ExprVisitor<'a> {
    /// 失败类型。
    type Error;

    /// 前序进入；`parent` 是词法父节点（根为 `None`）。
    ///
    /// 返回 `Ok(false)` ⇒ **剪枝**：不进入该节点子树，也不会为它调用 [`Self::leave`]。
    fn enter(
        &mut self,
        _expr: &'a Expr,
        _parent: Option<&'a Node>,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// 后序离开（只对 `enter` 返回 `true` 的节点调用）。
    fn leave(&mut self, _expr: &'a Expr) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Expr {
    /// 词法树遍历：前序进入（可剪枝）→ 子节点 → 后序离开。
    ///
    /// 递归深度 = 表达式词法深度（表达式由脚本生成，深度受 `compile` 的既有约束）。
    pub fn walk<'a, V>(&'a self, visitor: &mut V) -> Result<(), V::Error>
    where
        V: ExprVisitor<'a>,
    {
        fn step<'a, V>(expr: &'a Expr, parent: Option<&'a Node>, visitor: &mut V) -> Result<(), V::Error>
        where
            V: ExprVisitor<'a>,
        {
            if !visitor.enter(expr, parent)? {
                return Ok(());
            }
            for child in expr.node.children() {
                step(child, Some(&expr.node), visitor)?;
            }
            visitor.leave(expr)
        }
        step(self, None, visitor)
    }

    /// 重建节点：每个**直接子表达式**过一遍 `f`，节点自身的 `id` 与叶子原样保留。
    ///
    /// 这是重写类 pass 的唯一子节点重建入口（不复制 `Call` / `Select` / `NatRec`
    /// 的字段展开）。
    pub fn map_children(&self, mut f: impl FnMut(&Expr) -> Expr) -> Expr {
        let node = match &self.node {
            Node::Call { op, args } => Node::Call {
                op: op.clone(),
                args: args.iter().map(|arg| f(arg)).collect(),
            },
            Node::Select { cond, then, r#else } => Node::Select {
                cond: Box::new(f(cond)),
                then: Box::new(f(then)),
                r#else: Box::new(f(r#else)),
            },
            Node::NatRec { count, init, step } => Node::NatRec {
                count: Box::new(f(count)),
                init: Box::new(f(init)),
                step: Box::new(f(step)),
            },
            Node::Method {
                method,
                object,
                args,
            } => Node::Method {
                method: method.clone(),
                object: *object,
                args: args.iter().map(|arg| f(arg)).collect(),
            },
            other => other.clone(),
        };
        Expr {
            id: self.id.clone(),
            node,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChannelRef, Expr, OpRef};

    fn op(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    /// 前序 / 后序顺序 + 剪枝。
    #[test]
    fn walk_is_preorder_with_pruning_and_leave() {
        struct Order(Vec<String>);
        impl<'a> ExprVisitor<'a> for Order {
            type Error = std::convert::Infallible;
            fn enter(
                &mut self,
                expr: &'a Expr,
                _parent: Option<&'a Node>,
            ) -> Result<bool, Self::Error> {
                if let Some(id) = expr.id.as_deref() {
                    self.0.push(id.to_string());
                    // 带 id 的节点剪枝：其子树不再进入。
                    return Ok(false);
                }
                Ok(true)
            }
            fn leave(&mut self, expr: &'a Expr) -> Result<(), Self::Error> {
                if let Node::Call { op, .. } = &expr.node {
                    self.0.push(op.name.clone());
                }
                Ok(())
            }
        }

        let tree = Expr::call(
            op("sin"),
            vec![Expr::call(op("cos"), vec![Expr::pure(1.0)]).with_id("h1")],
        );
        let mut order = Order(Vec::new());
        tree.walk(&mut order).unwrap_or(());
        // 根 `sin` 后序离开；`h1` 剪枝 ⇒ 不进入 `cos`，也不 leave 它。
        assert_eq!(order.0, vec!["h1".to_string(), "sin".to_string()]);
    }

    /// `map_children` 保留 id 与叶子，只替换子表达式。
    #[test]
    fn map_children_rewrites_children_and_keeps_id() {
        let tree = Expr::call(
            op("sin"),
            vec![Expr::channel(ChannelRef::builtin("time"))],
        )
        .with_id("h2");
        let rewritten = tree.map_children(|_| Expr::pure(0.0));
        assert_eq!(rewritten.id.as_deref(), Some("h2"));
        let Node::Call { args, .. } = &rewritten.node else {
            panic!("节点种类必须保持 Call");
        };
        assert_eq!(args, &vec![Expr::pure(0.0)]);
    }
}
