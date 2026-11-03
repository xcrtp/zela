//! Pod trait：不透明状态机 + 函数集合 + 协商 + 快慢双路径。

use std::any::Any;
use std::fmt;

use crate::capability::{Capability, Resolved};
use crate::error::PodError;
use crate::handle::PodId;
use crate::qualified::QualifiedName;
use crate::typespec::TypeSpec;

/// 一个可被脚本选择的**通道声明**（提案 #6）。
///
/// Pod 自述"我有哪些字段地址"，语言绑定据此在组件构造器上挂常量
/// （`Circle.channel.radius`）。与 [`Pod::functions`] 一样是**自述面**，不参与协商。
///
/// **命名**：脚本侧的通道**值**类型叫 `ChannelSpec`（`ze-pod-func`）；
/// 这里是**声明**，故叫 `ChannelDecl`（计划 §0.2：同名 `pub struct` 只允许一处）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelDecl {
    /// 命名词汇表的 namespace（如 `zela.scene`）。
    pub namespace: &'static str,
    /// 所属组件（`Circle`）。
    pub component: &'static str,
    /// 字段名（`radius`）。
    pub name: &'static str,
    /// 规范地址（`Circle.radius`）。
    pub address: &'static str,
    /// 字段类型（写方向 / 文档用）。
    pub ty: &'static TypeSpec,
    /// 常量**值**的 PodType 名（`{ namespace, name }` 结构体；声明文件用它标注）。
    pub value_type: &'static str,
}

/// 一个可被脚本调用的**方法声明**（B2 方法目录的自述面）。
///
/// 与 [`ChannelDecl`] 同构：语言绑定据此把方法挂到组件构造器上
/// （`Reveal.current_pos(curve)` 等价于 `expr.method { name = "Reveal.current_pos", object = curve }`）。
///
/// 与通道的区别是**类型级的**：方法不在写表里 ⇒ 永远选不中作绑定目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodDecl {
    /// 命名词汇表的 namespace（如 `zela.scene`）。
    pub namespace: &'static str,
    /// 所属组件（`Reveal`）。
    pub component: &'static str,
    /// 成员名（`current_pos`）。
    pub name: &'static str,
    /// 规范方法名（`Reveal.current_pos`；`expr.method.name` 用它）。
    pub qualified: &'static str,
    /// **显式**参数个数（接收者不计）。
    pub arity: usize,
    /// 返回类型（声明文件 / 编辑期提示用）。
    pub ret: &'static TypeSpec,
}

/// 函数句柄：**per-Pod 局部槽位 + generation**（计划 §0.2 的第六个句柄）。
///
/// 旧形态是裸 `u64`（只有编号、没有 owner / generation），查找靠
/// `functions().iter().position(..)` —— 线性扫描，且槽位复用后会读到别人的函数。
/// 现在是 generation 感知句柄：机制与 [`crate::Handle`] 同源
/// （`ze_reflect::handle::HandleCore`），Pod 在装配期建立符号表，之后是整数槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionId {
    owner: PodId,
    slot: u64,
    generation: u32,
}

impl FunctionId {
    /// 直接构造（自定义 Pod 的慢路径兼容入口：owner = `PodId(0)`、generation = 0）。
    pub const fn new(raw: u64) -> Self {
        Self {
            owner: PodId::new(0),
            slot: raw,
            generation: 0,
        }
    }

    /// 带 owner + generation 的构造（装配期符号表用）。
    pub const fn local(owner: PodId, slot: u64, generation: u32) -> Self {
        Self {
            owner,
            slot,
            generation,
        }
    }

    /// 所属 Pod。
    pub const fn owner(&self) -> PodId {
        self.owner
    }

    /// 槽位号（= 旧形态的"函数编号"）。
    pub const fn slot(&self) -> u64 {
        self.slot
    }

    /// 槽位代数（防 ABA）。
    pub const fn generation(&self) -> u32 {
        self.generation
    }

    /// 原始槽位号（兼容旧的 `get()`）。
    pub const fn get(self) -> u64 {
        self.slot
    }

    /// 校验句柄属于 `owner`；不属于 ⇒ [`PodError::CrossPodHandle`]。
    ///
    /// `owner = PodId(0)` 视为"未绑定 owner 的兼容句柄"，不参与校验。
    pub fn ensure_owner(&self, owner: PodId) -> Result<(), PodError> {
        if self.owner == owner || self.owner == PodId::new(0) {
            Ok(())
        } else {
            Err(PodError::CrossPodHandle {
                expected: owner.get(),
                actual: self.owner.get(),
            })
        }
    }
}

impl ze_reflect::handle::HandleCore for FunctionId {
    type Owner = PodId;

    fn owner(&self) -> PodId {
        self.owner
    }

    fn slot(&self) -> u64 {
        self.slot
    }

    fn generation(&self) -> u32 {
        self.generation
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.slot)
    }
}

/// 一个 Pod：**不透明状态机** + 它的函数集合。
///
/// # 借用方案（§0.2.1 第 3 条）
///
/// 快慢两路径都用 `&self`。有状态 Pod 用内部可变性（`Cell` / `RefCell` / 锁）；
/// 状态归 Pod 自己管，外部看不到"它在变"。
///
/// # 快路径 vs 慢路径
///
/// - **快路径**：[`Pod::negotiate`] 一次性解析出 typed 句柄，之后每次调用 =
///   一次 fn 指针间接寻址；无字符串比较、无 downcast；
/// - **慢路径**：[`Pod::call`] 按 [`FunctionId`] + 类型擦除实参保底；
///   纯解释型 Pod 无法提供 typed 可调用时走它。
///
/// 两者必须**结果一致**（用例锁定）。
pub trait Pod: 'static {
    /// per-Pod 身份（用于不透明句柄的跨 Pod 检测）。
    fn id(&self) -> PodId;

    /// 具体状态机的向下转型入口（有状态 Pod 在函数里读自己的状态）。
    fn as_any(&self) -> &dyn Any;

    /// 函数集合的完整函数名清单。
    ///
    /// 这是"Pod 自述"的最小形态（§3 待定项 4：直接返回 `&[QualifiedName]` 最简单）。
    fn functions(&self) -> &[QualifiedName];

    /// 按完整函数名解析出 typed 可调用对象。
    ///
    /// 返回 `None` ⇒ 该 Pod 没有（或只有 erased 实现、无法提供快路径）。
    fn resolve(&self, name: &QualifiedName) -> Option<&(dyn Any + Send + Sync)>;

    /// 慢路径（保底）：按函数编号 + 类型擦除实参调用。
    fn call(&self, id: FunctionId, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError>;

    /// 本 Pod 可被脚本选择的**通道常量**（默认空）。
    ///
    /// 语言绑定用它把 `Circle.channel.radius` 挂到组件构造器上；端点（写方向）仍由
    /// 各语言的地址表/宿主校验决定。
    fn channels(&self) -> &'static [ChannelDecl] {
        &[]
    }

    /// 本 Pod 可被脚本调用的**方法**（默认空）。
    ///
    /// 语言绑定用它把 `Reveal.current_pos(curve)` 挂到组件构造器上 ——
    /// 方法不在写表里，作者面只有这一条调用路径（`expr.method { … }` 是底层形态）。
    fn methods(&self) -> &'static [MethodDecl] {
        &[]
    }

    /// 按完整函数名取函数句柄（默认按 [`Pod::functions`] 的顺序，线性扫描）。
    ///
    /// 有符号表的 Pod（如 `RegistryPod`）应覆盖它：装配期解析一次，之后 O(1)。
    fn function_id(&self, name: &QualifiedName) -> Option<FunctionId> {
        self.functions()
            .iter()
            .position(|n| n == name)
            .map(|i| FunctionId::local(self.id(), i as u64, 0))
    }

    /// 以 capability struct 为泛型实参协商；**全有或全无**。
    ///
    /// `C` 是 **struct**（如 `MathPod<'a>`），不是 trait —— 见 [`Capability`]。
    ///
    /// 对 `dyn Pod`（如注入后的 `Arc<dyn Pod>`）请用自由函数
    /// [`negotiate_dyn`]：泛型方法不进入 vtable，trait object 上调不到。
    fn negotiate<'a, C>(&'a self) -> Option<C>
    where
        C: Capability<'a>,
        Self: Sized + 'a,
    {
        negotiate_dyn::<C>(self)
    }
}

/// **线程安全徽章**：`Pod + Send + Sync` 的自动认证。
///
/// # 为什么是 supertrait 而不是反向 impl
///
/// `SyncPod: Pod` ⇒ 每个 `SyncPod` 天然是 `Pod`（"自动实现 Pod"由 supertrait 成立）；
/// blanket impl 只提供"自动认证"，不写 `impl<T: SyncPod> Pod for T`（那会循环且冲突）。
///
/// # 为什么不能强制 `trait Pod: Send + Sync`
///
/// 强制会给所有 Pod 的 `RefCell` 换成锁、`Rc` 换成 `Arc`，连无竞争的热路径也要加锁。
/// 本项目的并发模型是"主线程唯一、并发只在极小边界" —— 全局强制正好反着来。
/// 因此线程安全是 **opt-in 徽章**：需要跨线程（pyo3 闭包 / 未来 async）的 Pod 才声明。
///
/// # 为什么有 `as_pod`
///
/// 让 `Arc<dyn SyncPod>` 在不依赖 trait upcasting 的工具链上也能拿到 `&dyn Pod`
/// （能力协商只认 `Pod`）。blanket impl 里由具体类型完成这次 unsized 转换。
pub trait SyncPod: Pod + Send + Sync {
    /// 以基础 `Pod` trait object 使用（能力协商 / `Pod::call` 的入口）。
    fn as_pod(&self) -> &dyn Pod;
}

impl<T: Pod + Send + Sync> SyncPod for T {
    fn as_pod(&self) -> &dyn Pod {
        self
    }
}

/// 对 **trait object**（`&dyn Pod`）可用的协商入口。
///
/// # 为什么是自由函数而不是 trait 方法
///
/// `Pod::negotiate` 是泛型方法 + `Self: Sized` ⇒ 只存在于具体类型，trait object
/// 上没有这个符号。泛型方法**无法** object-safe（即便去掉 `Self: Sized`，
/// vtable 也放不下单态化后的所有 `C`）。
///
/// 宿主把能力以 `Arc<dyn Pod>` 注入后，Pod 侧拿到的正是 trait object ——
/// 这条路径必须用自由函数。
///
/// 行为与 [`Pod::negotiate`] **完全一致**：按完整函数名逐个解析，全有或全无；
/// 调用期仍是一次 fn 指针间接寻址（协商只发生一次）。
pub fn negotiate_dyn<'a, C>(pod: &'a dyn Pod) -> Option<C>
where
    C: Capability<'a>,
{
    let sigs = C::signatures();
    let mut resolved: Vec<Resolved<'a>> = Vec::with_capacity(sigs.len());
    for sig in sigs {
        let name = sig.qualified();
        // 任一签名缺失 ⇒ 整个协商失败（不返回"有的能调、有的崩"的句柄）。
        let callable = pod.resolve(&name)?;
        resolved.push(Resolved {
            signature: *sig,
            callable,
        });
    }
    C::from_negotiated(pod, &resolved)
}
