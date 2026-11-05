//! **CK-a**：关系契约 —— 树 / DAG 的六问，与编译期纯值索引。
//!
//! # 为什么只有一份词汇
//!
//! 层级语义、坐标转换、命中测试、合成语义、深度模型这五个主题，历史上被
//! `Parent` 链 / `LayerId` / `ZOrder` / `scope_path` / `SurfaceScope` / `View.bounds`
//! / `LayoutId` / `HitId` 约十套词汇各写了一遍。它们不是五块独立代码，是同一条接缝：
//! **向下/前推（变换、布局、主题、求值、录制）与向上/回溯（冒泡、找祖先、dirty
//! 传播、失效归因）是一个有向关系的两个方向**。
//!
//! 树 = 归属（入度 ≤ 1）；DAG = 依赖（可多入度，拓扑序）。树是 DAG 加了约束的特例，
//! 共用同一份遍历契约。几何偏序（遮挡）是 per-pixel、随相机视口变化的另一类语义，
//! **不进树、不进 DAG**（见 GAPS CK 段与 `docs/principles.md`）。
//!
//! # 六问（每条关系声明都要回答）
//!
//! ```text
//! 方向   push / pull / 双向（由编译结果的两个方向表达，不另设字段）
//! 顺序   稳定序（BTreeMap / Id 升序 / 索引序）
//! 环     一律拒绝，给完整路径
//! 归属   声明期数据可进产物；遍历结果是派生态，不进产物
//! 可观测 影响必须能差分出来，不靠传播 tag
//! 相位   在哪一帧、哪个阶段生效，不隐式触发
//! ```
//!
//! # 不进产物（本批最硬的口径）
//!
//! [`TreeIndex`] / [`DagIndex`] 是**编译产物**，与 `WorldTransforms` 同类：派生态、
//! 不进 payload、回放端从「已有声明 + 事件流前缀」重算。它们不持句柄，只有
//! `Vec<u32>` 与声明期的 `Id` 列表。
//!
//! # 三条反面约束
//!
//! 1. **不做增量索引维护**：插一个节点 `in/out` 全体偏移，插一条边 topo 可能整体变；
//!    增量结果必须与全量重编译逐位对拍 ⇒ 增量白写。这里的答案是「重算」。
//! 2. **不建通用 `Relation` 运行期类型**：各域存储保持原样，编译产物是纯 `Vec<u32>`。
//! 3. **不沿关系传播 tag**：归因靠快照差分（见 `zela-app/src/debug.rs`）。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

/// 关系生效相位。
///
/// 一次影响只在指定相位生效；同帧子→父触发的父→子重算排到下一帧
/// （§3.3：相位与双向不互激）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationPhase {
    /// 装配期：结构声明、索引编译。
    Assembly,
    /// 输入相位：窗口 / RPC 事件进入场景之前。
    Input,
    /// 逻辑相位：状态机、动画求值。
    LogicUpdate,
    /// 帧末：回写与诊断采集。
    FrameEnd,
}

impl Default for RelationPhase {
    fn default() -> Self {
        Self::Assembly
    }
}

impl RelationPhase {
    /// 单调序（调度器按它排序；只用于比较，不是产物字段）。
    pub fn rank(self) -> u8 {
        match self {
            Self::Assembly => 0,
            Self::Input => 1,
            Self::LogicUpdate => 2,
            Self::FrameEnd => 3,
        }
    }
}

/// 关系声明的**只读适配器**。
///
/// 不要求任何存储改形状：各域自己的 `BTreeMap` / `Vec<Id>` / `PlacedRect` 原样，
/// 实现是编译期代码。四个问题各自独立，缺省为空关系。
pub trait RelationSource {
    /// 声明期 id（`Ord` 保证稳定序；`Copy` 保证不进句柄）。
    type Id: Copy + Ord + Debug;

    /// 全部节点。**顺序不参与语义**（实现内部一律再排序）。
    fn nodes(&self) -> Vec<Self::Id>;

    /// 归属（树）：最多一个父。缺省 = 没有父（根）。
    fn parent(&self, _id: Self::Id) -> Option<Self::Id> {
        None
    }

    /// 归属声明的**原始**入边，用于「入度 > 1」门禁。
    ///
    /// 缺省由 [`RelationSource::parent`] 推出；存储本身允许多父的适配器必须覆盖，
    /// 否则多父会被静默压成一个（那就是二义性）。
    fn parents(&self, id: Self::Id) -> Vec<Self::Id> {
        self.parent(id).into_iter().collect()
    }

    /// 依赖（DAG）：可多入度。缺省 = 没有依赖。
    fn deps(&self, _id: Self::Id) -> Vec<Self::Id> {
        Vec::new()
    }

    /// 生效相位。缺省 = 装配期（关系声明本身就是装配期数据）。
    fn phase(&self, _id: Self::Id) -> RelationPhase {
        RelationPhase::Assembly
    }
}

/// 关系声明被拒绝的显式原因（完整路径，不静默兜底）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationError<Id> {
    /// 自引用（`id` 是自己的父或依赖）。
    SelfReference { id: Id },
    /// 父不在节点集合里。
    UnknownParent { id: Id, parent: Id },
    /// 依赖不在节点集合里。
    UnknownDependency { id: Id, dep: Id },
    /// 归属入度 > 1（树语义下非法）。
    MultipleParents { id: Id, parents: Vec<Id> },
    /// 环：首尾相同的完整路径（例如 `[a, b, c, a]`）。
    Cycle { path: Vec<Id> },
}

impl<Id: Debug> std::fmt::Display for RelationError<Id> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfReference { id } => write!(f, "关系声明自引用：{id:?}"),
            Self::UnknownParent { id, parent } => {
                write!(f, "关系声明的父不在节点集合：{id:?} → {parent:?}")
            }
            Self::UnknownDependency { id, dep } => {
                write!(f, "关系声明的依赖不在节点集合：{id:?} → {dep:?}")
            }
            Self::MultipleParents { id, parents } => {
                write!(f, "关系声明多父（树要求入度 ≤ 1）：{id:?} ← {parents:?}")
            }
            Self::Cycle { path } => write!(f, "关系声明成环：{path:?}"),
        }
    }
}

impl<Id: Debug> std::error::Error for RelationError<Id> {}

/// **门禁 1 + 2**：归属唯一、环一律拒绝；依赖也不许成环。
///
/// 这是装配期检查（回放端也能自检），零运行期分支。顺序解耦不是检查项 ——
/// 编译函数本身只按 `Id` 升序工作，由用例证明。
pub fn check<S>(src: &S) -> Result<(), RelationError<S::Id>>
where
    S: RelationSource + ?Sized,
{
    check_tree(src)?;
    check_dag(src)
}

/// 只检查归属（树）那半边。
pub fn check_tree<S>(src: &S) -> Result<(), RelationError<S::Id>>
where
    S: RelationSource + ?Sized,
{
    let nodes = stable_nodes(src);
    let index = index_of(&nodes);
    for &id in &nodes {
        let parents = unique_sorted(src.parents(id));
        if parents.iter().any(|parent| *parent == id) {
            return Err(RelationError::SelfReference { id });
        }
        for parent in &parents {
            if !index.contains_key(parent) {
                return Err(RelationError::UnknownParent {
                    id,
                    parent: *parent,
                });
            }
        }
        if parents.len() > 1 {
            return Err(RelationError::MultipleParents { id, parents });
        }
    }
    // 沿父链上溯：命中自己走过的路径 ⇒ 环；路径首尾相同。
    let mut done = vec![false; nodes.len()];
    for start in &nodes {
        let mut path = Vec::new();
        let mut seen = BTreeMap::new();
        let mut cursor = Some(*start);
        while let Some(id) = cursor {
            let slot = index[&id] as usize;
            if done[slot] {
                break;
            }
            if let Some(&position) = seen.get(&id) {
                let mut cycle: Vec<S::Id> = path[position..].to_vec();
                cycle.push(id);
                return Err(RelationError::Cycle { path: cycle });
            }
            seen.insert(id, path.len());
            path.push(id);
            cursor = src.parent(id);
        }
        for id in path {
            done[index[&id] as usize] = true;
        }
    }
    Ok(())
}

/// 只检查依赖（DAG）那半边。
pub fn check_dag<S>(src: &S) -> Result<(), RelationError<S::Id>>
where
    S: RelationSource + ?Sized,
{
    let nodes = stable_nodes(src);
    let index = index_of(&nodes);
    let deps = dependency_lists(src, &nodes, &index)?;

    // 迭代 DFS：0 = 未访问，1 = 在栈上，2 = 完成。
    let mut color = vec![0u8; nodes.len()];
    let mut stack: Vec<(u32, usize)> = Vec::new();
    let mut path: Vec<u32> = Vec::new();
    for start in 0..nodes.len() as u32 {
        if color[start as usize] != 0 {
            continue;
        }
        color[start as usize] = 1;
        stack.push((start, 0));
        path.push(start);
        while let Some(top) = stack.len().checked_sub(1) {
            let (node, cursor) = stack[top];
            let node_deps = &deps[node as usize];
            if cursor < node_deps.len() {
                let dep = node_deps[cursor];
                stack[top].1 += 1;
                match color[dep as usize] {
                    0 => {
                        color[dep as usize] = 1;
                        stack.push((dep, 0));
                        path.push(dep);
                    }
                    1 => {
                        let position = path
                            .iter()
                            .position(|&item| item == dep)
                            .expect("在栈上的节点必在路径里");
                        let mut cycle: Vec<S::Id> =
                            path[position..].iter().map(|&i| nodes[i as usize]).collect();
                        cycle.push(nodes[dep as usize]);
                        return Err(RelationError::Cycle { path: cycle });
                    }
                    _ => {}
                }
            } else {
                color[node as usize] = 2;
                stack.pop();
                path.pop();
            }
        }
    }
    Ok(())
}

/// Euler 区间编译结果（树的编译产物）。
///
/// `nodes` 按 `Id` 升序（声明序，供二分查找）；`tin / out / dfs` 是**纯 `u32`**。
/// `u` 是 `v` 的祖先 ⇔ `tin[u] ≤ tin[v] < out[u]`；`u` 的子树是 `tin` 序上
/// `[tin[u], out[u])` 一段**连续切片**。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeIndex<Id> {
    nodes: Vec<Id>,
    parent: Vec<Option<u32>>,
    tin: Vec<u32>,
    out: Vec<u32>,
    dfs: Vec<u32>,
}

impl<Id: Copy + Ord + Debug> TreeIndex<Id> {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 声明序（`Id` 升序）。
    pub fn nodes(&self) -> &[Id] {
        &self.nodes
    }

    /// `Id` → 声明序下标。
    pub fn slot(&self, id: Id) -> Option<u32> {
        self.nodes.binary_search(&id).ok().map(|i| i as u32)
    }

    pub fn contains(&self, id: Id) -> bool {
        self.slot(id).is_some()
    }

    /// 父（声明序下标）。
    pub fn parent(&self, id: Id) -> Option<Id> {
        let slot = self.slot(id)?;
        self.parent[slot as usize].map(|i| self.nodes[i as usize])
    }

    /// `u` 是否为 `v` 的祖先（含自身）。
    pub fn is_ancestor(&self, u: Id, v: Id) -> bool {
        let (Some(u), Some(v)) = (self.slot(u), self.slot(v)) else {
            return false;
        };
        self.tin[u as usize] <= self.tin[v as usize] && self.tin[v as usize] < self.out[u as usize]
    }

    /// 子树（`tin` 序连续切片），含自身。
    pub fn subtree(&self, id: Id) -> Option<impl Iterator<Item = Id> + '_> {
        let slot = self.slot(id)?;
        let (start, end) = (
            self.tin[slot as usize] as usize,
            self.out[slot as usize] as usize,
        );
        Some(
            self.dfs[start..end]
                .iter()
                .map(move |&index| self.nodes[index as usize]),
        )
    }

    pub fn subtree_len(&self, id: Id) -> usize {
        self.slot(id)
            .map(|slot| (self.out[slot as usize] - self.tin[slot as usize]) as usize)
            .unwrap_or(0)
    }

    /// 子树在 `tin` 序上的半开区间。
    pub fn subtree_range(&self, id: Id) -> Option<(u32, u32)> {
        let slot = self.slot(id)?;
        Some((self.tin[slot as usize], self.out[slot as usize]))
    }

    /// `tin` 序（纯下标；供「遍历一段切片」用）。
    pub fn dfs_order(&self) -> &[u32] {
        &self.dfs
    }

    /// 按 `tin` 序取节点。
    pub fn node_at(&self, tin: u32) -> Option<Id> {
        self.dfs.get(tin as usize).map(|&i| self.nodes[i as usize])
    }
}

/// **编译**树（Euler 区间）。环 / 多父 / 未知父一律拒绝。
pub fn compile_tree<S>(src: &S) -> Result<TreeIndex<S::Id>, RelationError<S::Id>>
where
    S: RelationSource + ?Sized,
{
    check_tree(src)?;
    let nodes = stable_nodes(src);
    let index = index_of(&nodes);
    let n = nodes.len();
    let mut parent = vec![None; n];
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (slot, &id) in nodes.iter().enumerate() {
        if let Some(parent_id) = src.parent(id) {
            let parent_slot = index[&parent_id];
            parent[slot] = Some(parent_slot);
            children[parent_slot as usize].push(slot as u32);
        }
    }

    let mut tin = vec![0u32; n];
    let mut out = vec![0u32; n];
    let mut dfs = Vec::with_capacity(n);
    let mut timer = 0u32;
    let mut stack: Vec<(u32, usize)> = Vec::new();
    for root in 0..n as u32 {
        if parent[root as usize].is_some() {
            continue;
        }
        tin[root as usize] = timer;
        timer += 1;
        dfs.push(root);
        stack.push((root, 0));
        while let Some(top) = stack.len().checked_sub(1) {
            let (node, cursor) = stack[top];
            let node_children = &children[node as usize];
            if cursor < node_children.len() {
                let child = node_children[cursor];
                stack[top].1 += 1;
                tin[child as usize] = timer;
                timer += 1;
                dfs.push(child);
                stack.push((child, 0));
            } else {
                out[node as usize] = timer;
                stack.pop();
            }
        }
    }
    debug_assert_eq!(dfs.len(), n, "无环时全部节点都会被访问");
    Ok(TreeIndex {
        nodes,
        parent,
        tin,
        out,
        dfs,
    })
}

/// DAG 编译结果：双向 CSR + 拓扑序（纯 `u32`）。
///
/// 正向段（`offsets/targets`）= 下游（求值序）；反向段
/// （`r_offsets/r_targets`）= 上游（dirty / 归因回溯序）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DagIndex<Id> {
    nodes: Vec<Id>,
    topo: Vec<Id>,
    position: Vec<u32>,
    offsets: Vec<u32>,
    targets: Vec<u32>,
    r_offsets: Vec<u32>,
    r_targets: Vec<u32>,
}

impl<Id: Copy + Ord + Debug> DagIndex<Id> {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 声明序（`Id` 升序）。
    pub fn nodes(&self) -> &[Id] {
        &self.nodes
    }

    /// 拓扑序（上游在前，同层按 id 升序）。
    pub fn topo(&self) -> &[Id] {
        &self.topo
    }

    /// 拓扑位置（越大越靠下游）。
    pub fn topo_position(&self, id: Id) -> Option<u32> {
        let slot = self.nodes.binary_search(&id).ok()?;
        Some(self.position[slot])
    }

    /// 下游（按拓扑序）。正向段顺序 = 求值序。
    pub fn successors(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        self.forward_range(id)
            .map(|i| self.topo[self.targets[i] as usize])
    }

    /// 上游（按拓扑序）。反向段顺序 = dirty / 归因回溯序。
    pub fn predecessors(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        self.reverse_range(id)
            .map(|i| self.topo[self.r_targets[i] as usize])
    }

    pub fn out_degree(&self, id: Id) -> usize {
        self.forward_range(id).len()
    }

    pub fn in_degree(&self, id: Id) -> usize {
        self.reverse_range(id).len()
    }

    /// 正向 CSR 段（`targets` 下标；按拓扑位置取）。
    pub fn forward_range(&self, id: Id) -> std::ops::Range<usize> {
        match self.nodes.binary_search(&id) {
            Ok(slot) => {
                let place = self.position[slot] as usize;
                self.offsets[place] as usize..self.offsets[place + 1] as usize
            }
            Err(_) => 0..0,
        }
    }

    /// 反向 CSR 段。
    pub fn reverse_range(&self, id: Id) -> std::ops::Range<usize> {
        match self.nodes.binary_search(&id) {
            Ok(slot) => {
                let place = self.position[slot] as usize;
                self.r_offsets[place] as usize..self.r_offsets[place + 1] as usize
            }
            Err(_) => 0..0,
        }
    }

    pub fn offsets(&self) -> &[u32] {
        &self.offsets
    }

    pub fn targets(&self) -> &[u32] {
        &self.targets
    }

    pub fn r_offsets(&self) -> &[u32] {
        &self.r_offsets
    }

    pub fn r_targets(&self) -> &[u32] {
        &self.r_targets
    }
}

/// **编译** DAG（双向 CSR + 拓扑序）。环 / 未知依赖一律拒绝。
pub fn compile_dag<S>(src: &S) -> Result<DagIndex<S::Id>, RelationError<S::Id>>
where
    S: RelationSource + ?Sized,
{
    check_dag(src)?;
    let nodes = stable_nodes(src);
    let index = index_of(&nodes);
    let deps = dependency_lists(src, &nodes, &index)?;
    let n = nodes.len();

    // Kahn：就绪集合取最小 id ⇒ 输出只由拓扑与 id 决定。
    let mut in_degree = vec![0usize; n];
    let mut downstream: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (slot, node_deps) in deps.iter().enumerate() {
        for &dep in node_deps {
            downstream[dep as usize].push(slot as u32);
            in_degree[slot] += 1;
        }
    }
    let mut ready: BTreeSet<u32> = (0..n as u32)
        .filter(|&slot| in_degree[slot as usize] == 0)
        .collect();
    let mut topo_slots = Vec::with_capacity(n);
    while let Some(slot) = ready.pop_first() {
        topo_slots.push(slot);
        for &target in &downstream[slot as usize] {
            in_degree[target as usize] -= 1;
            if in_degree[target as usize] == 0 {
                ready.insert(target);
            }
        }
    }
    debug_assert_eq!(topo_slots.len(), n, "无环时 Kahn 会排完所有节点");

    let mut position = vec![0u32; n];
    for (place, &slot) in topo_slots.iter().enumerate() {
        position[slot as usize] = place as u32;
    }
    let topo: Vec<S::Id> = topo_slots.iter().map(|&i| nodes[i as usize]).collect();

    // 正向 CSR：按拓扑序排好的下游。
    let mut offsets = Vec::with_capacity(n + 1);
    let mut targets = Vec::new();
    offsets.push(0u32);
    for &slot in &topo_slots {
        let mut out = downstream[slot as usize].clone();
        out.sort_unstable_by_key(|&i| position[i as usize]);
        targets.extend(out.iter().map(|&i| position[i as usize]));
        offsets.push(targets.len() as u32);
    }

    // 反向 CSR：上游（deps）也按拓扑序。
    let mut r_offsets = Vec::with_capacity(n + 1);
    let mut r_targets = Vec::new();
    r_offsets.push(0u32);
    for &slot in &topo_slots {
        let mut up = deps[slot as usize].clone();
        up.sort_unstable_by_key(|&i| position[i as usize]);
        r_targets.extend(up.iter().map(|&i| position[i as usize]));
        r_offsets.push(r_targets.len() as u32);
    }

    Ok(DagIndex {
        nodes,
        topo,
        position,
        offsets,
        targets,
        r_offsets,
        r_targets,
    })
}

/// **声明哈希**：`revision = hash(声明，BTree 排序后)`。
///
/// 不取运行期计数器、不由宿主递增 —— 插入顺序 / 跨机器重放都不漂移。
/// 关系没变 ⇒ 同一帧复用编译结果（`60 帧只编译 1 次`）。
pub fn revision<S>(src: &S) -> u64
where
    S: RelationSource + ?Sized,
{
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for id in stable_nodes(src) {
        for byte in format!("{id:?}").bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
        for parent in unique_sorted(src.parents(id)) {
            for byte in format!(">{parent:?}").bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(PRIME);
            }
        }
        for dep in unique_sorted(src.deps(id)) {
            for byte in format!("|{dep:?}").bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(PRIME);
            }
        }
        hash ^= u64::from(src.phase(id).rank());
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn stable_nodes<S>(src: &S) -> Vec<S::Id>
where
    S: RelationSource + ?Sized,
{
    let mut nodes: Vec<S::Id> = src.nodes();
    nodes.sort_unstable();
    nodes.dedup();
    nodes
}

fn index_of<Id: Copy + Ord>(nodes: &[Id]) -> BTreeMap<Id, u32> {
    nodes
        .iter()
        .enumerate()
        .map(|(slot, &id)| (id, slot as u32))
        .collect()
}

fn unique_sorted<Id: Copy + Ord>(mut ids: Vec<Id>) -> Vec<Id> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// 依赖 → 声明序下标（排序去重；未知依赖显式报错）。
fn dependency_lists<S>(
    src: &S,
    nodes: &[S::Id],
    index: &BTreeMap<S::Id, u32>,
) -> Result<Vec<Vec<u32>>, RelationError<S::Id>>
where
    S: RelationSource + ?Sized,
{
    let mut out = Vec::with_capacity(nodes.len());
    for &id in nodes {
        let mut slots = Vec::new();
        for dep in unique_sorted(src.deps(id)) {
            if dep == id {
                return Err(RelationError::SelfReference { id });
            }
            let Some(&slot) = index.get(&dep) else {
                return Err(RelationError::UnknownDependency { id, dep });
            };
            slots.push(slot);
        }
        slots.sort_unstable();
        slots.dedup();
        out.push(slots);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用适配器：节点 + 父 + 依赖，插入顺序可被打乱。
    #[derive(Debug, Clone, Default)]
    struct MapSource {
        nodes: Vec<u32>,
        parent: BTreeMap<u32, u32>,
        extra_parents: BTreeMap<u32, Vec<u32>>,
        deps: BTreeMap<u32, Vec<u32>>,
    }

    impl MapSource {
        fn tree(edges: &[(u32, Option<u32>)], nodes: &[u32]) -> Self {
            Self {
                nodes: nodes.to_vec(),
                parent: edges
                    .iter()
                    .filter_map(|(id, parent)| parent.map(|parent| (*id, parent)))
                    .collect(),
                extra_parents: BTreeMap::new(),
                deps: BTreeMap::new(),
            }
        }
    }

    impl RelationSource for MapSource {
        type Id = u32;

        fn nodes(&self) -> Vec<u32> {
            self.nodes.clone()
        }

        fn parent(&self, id: u32) -> Option<u32> {
            self.parent.get(&id).copied()
        }

        fn parents(&self, id: u32) -> Vec<u32> {
            self.extra_parents
                .get(&id)
                .cloned()
                .unwrap_or_else(|| self.parent(id).into_iter().collect())
        }

        fn deps(&self, id: u32) -> Vec<u32> {
            self.deps.get(&id).cloned().unwrap_or_default()
        }
    }

    // 结构：0 根；1、2 是 0 的子；3 是 1 的子；4 是 2 的子。
    fn sample_tree() -> MapSource {
        MapSource::tree(
            &[
                (0, None),
                (1, Some(0)),
                (2, Some(0)),
                (3, Some(1)),
                (4, Some(2)),
            ],
            &[0, 1, 2, 3, 4],
        )
    }

    #[test]
    fn compile_tree_gives_euler_intervals() {
        let index = compile_tree(&sample_tree()).expect("合法树");
        assert_eq!(index.len(), 5);
        assert!(index.is_ancestor(0, 3));
        assert!(index.is_ancestor(1, 3));
        assert!(index.is_ancestor(3, 3), "自己是自己的祖先");
        assert!(!index.is_ancestor(1, 4));
        assert!(!index.is_ancestor(3, 1));

        let subtree: Vec<u32> = index.subtree(1).unwrap().collect();
        assert_eq!(subtree, vec![1, 3], "子树是连续切片");
        assert_eq!(index.subtree_len(0), 5);
        assert_eq!(index.subtree_len(4), 1);
    }

    /// **判据 1**：`nodes()` 逆序 / 不同插入顺序 ⇒ 编译结果逐位不变。
    #[test]
    fn compilation_is_independent_of_node_enumeration_order() {
        let forward = sample_tree();
        let mut reversed = sample_tree();
        reversed.nodes.reverse();
        assert_eq!(
            compile_tree(&forward).unwrap(),
            compile_tree(&reversed).unwrap()
        );
        assert_eq!(revision(&forward), revision(&reversed));

        // DAG 同理。
        let mut dag = MapSource {
            nodes: vec![2, 0, 1],
            ..Default::default()
        };
        dag.deps.insert(1, vec![0]);
        dag.deps.insert(2, vec![1]);
        let mut dag_reversed = dag.clone();
        dag_reversed.nodes.reverse();
        assert_eq!(compile_dag(&dag).unwrap(), compile_dag(&dag_reversed).unwrap());
        assert_eq!(revision(&dag), revision(&dag_reversed));
    }

    /// **判据 2**：人造环被拦，错误信息给完整路径。
    #[test]
    fn cycle_is_rejected_with_full_path() {
        let source = MapSource::tree(&[(0, Some(2)), (1, Some(0)), (2, Some(1))], &[0, 1, 2]);
        let error = compile_tree(&source).unwrap_err();
        match error {
            RelationError::Cycle { mut path } => {
                assert_eq!(path.first(), path.last(), "首尾相同：{path:?}");
                path.sort_unstable();
                path.dedup();
                assert_eq!(path, vec![0, 1, 2], "路径覆盖整个环");
            }
            other => panic!("期望环错误，得到 {other:?}"),
        }
        assert!(check(&source).is_err());
    }

    #[test]
    fn dag_cycle_is_rejected() {
        let mut dag = MapSource {
            nodes: vec![0, 1],
            ..Default::default()
        };
        dag.deps.insert(0, vec![1]);
        dag.deps.insert(1, vec![0]);
        let error = compile_dag(&dag).unwrap_err();
        assert!(matches!(error, RelationError::Cycle { .. }));
    }

    /// **门禁 1**：多父直接报错，并列出全部父。
    #[test]
    fn multiple_parents_are_rejected() {
        let mut source = sample_tree();
        source.extra_parents.insert(3, vec![1, 2]);
        let error = check_tree(&source).unwrap_err();
        assert_eq!(
            error,
            RelationError::MultipleParents {
                id: 3,
                parents: vec![1, 2]
            }
        );
    }

    #[test]
    fn unknown_parent_and_dependency_are_explicit() {
        let source = MapSource::tree(&[(1, Some(9))], &[1]);
        assert_eq!(
            compile_tree(&source).unwrap_err(),
            RelationError::UnknownParent {
                id: 1,
                parent: 9
            }
        );

        let mut dag = MapSource {
            nodes: vec![0],
            ..Default::default()
        };
        dag.deps.insert(0, vec![7]);
        assert_eq!(
            compile_dag(&dag).unwrap_err(),
            RelationError::UnknownDependency { id: 0, dep: 7 }
        );
    }

    #[test]
    fn self_reference_is_rejected() {
        let source = MapSource::tree(&[(0, Some(0))], &[0]);
        assert_eq!(
            check_tree(&source).unwrap_err(),
            RelationError::SelfReference { id: 0 }
        );
    }

    #[test]
    fn dag_csr_is_bidirectional_and_topological() {
        let mut dag = MapSource {
            nodes: vec![3, 1, 0, 2],
            ..Default::default()
        };
        dag.deps.insert(1, vec![0]);
        dag.deps.insert(2, vec![0]);
        dag.deps.insert(3, vec![1, 2]);
        let index = compile_dag(&dag).expect("DAG");

        let topo = index.topo().to_vec();
        let position = |id: u32| topo.iter().position(|&x| x == id).unwrap();
        assert!(position(0) < position(1));
        assert!(position(0) < position(2));
        assert!(position(1) < position(3));
        assert!(position(2) < position(3));

        assert_eq!(index.successors(0).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(index.predecessors(3).collect::<Vec<_>>(), vec![1, 2]);
        assert!(index.predecessors(0).next().is_none());
        assert!(index.successors(3).next().is_none());
        assert_eq!(index.out_degree(0), 2);
        assert_eq!(index.in_degree(3), 2);
    }

    #[test]
    fn revision_changes_when_declaration_changes() {
        let tree = sample_tree();
        let before = revision(&tree);
        let mut moved = tree.clone();
        moved.parent.insert(3, 2);
        assert_ne!(before, revision(&moved), "父变化必须改变 revision");

        let mut other = tree.clone();
        other.nodes.push(5);
        assert_ne!(before, revision(&other), "节点增删必须改变 revision");
    }

    #[test]
    fn empty_source_compiles_to_empty_indexes() {
        let empty = MapSource::default();
        assert!(check(&empty).is_ok());
        assert!(compile_tree(&empty).unwrap().is_empty());
        assert!(compile_dag(&empty).unwrap().is_empty());
    }
}
