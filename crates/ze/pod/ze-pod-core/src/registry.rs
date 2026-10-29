//! 第一 Pod：闭包注册表（内置小 Pod）。
//!
//! 注册 `(完整函数名, 可调用)`，同时暴露：
//! - **typed**：裸 fn 指针 ⇒ 协商出快路径句柄（每次调用一次间接寻址）；
//! - **erased**：`Fn(&dyn Pod, &[&dyn Any])` ⇒ 只有慢路径（纯解释型 Pod 的形态）。
//!
//! typed 注册会**自动**同时生成 erased 版本 —— 两条路径调用的是同一个函数，
//! 故结果必然一致（这是"快慢一致"最有力的实现方式）。

use std::any::Any;

use ze_reflect::Blame;
use ze_reflect::sym::SymbolTable;

use crate::error::PodError;
use crate::handle::PodId;
use crate::qualified::QualifiedName;
use crate::type_name::{IntoPodTypeName, PodTypeName};
use crate::pod::{FunctionId, Pod};

/// 类型擦除的可调用对象（慢路径）。
///
/// 对裸 fn 指针有 blanket 实现（arity 0..=3）。自定义 Pod 可直接持有
/// `Box<dyn ErasedCall>` 来复用慢路径。
pub trait ErasedCall: Send + Sync {
    /// 以类型擦除实参调用。
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError>;
}

fn expect_arity(args: &[&dyn Any], expected: usize) -> Result<(), PodError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(PodError::ArityMismatch {
            expected,
            got: args.len(),
        })
    }
}

fn arg<'b, T: 'static>(args: &[&'b dyn Any], index: usize) -> Result<&'b T, PodError> {
    let value: &'b dyn Any = *args.get(index).ok_or(PodError::ArityMismatch {
        expected: index + 1,
        got: args.len(),
    })?;
    value.downcast_ref::<T>().ok_or(PodError::ArgTypeMismatch {
        index,
        expected: std::any::type_name::<T>(),
    })
}

impl<R: 'static> ErasedCall for fn(&dyn Pod) -> R {
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        expect_arity(args, 0)?;
        Ok(Box::new((*self)(pod)))
    }
}

impl<A1: Clone + 'static, R: 'static> ErasedCall for fn(&dyn Pod, A1) -> R {
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        expect_arity(args, 1)?;
        let a1 = arg::<A1>(args, 0)?.clone();
        Ok(Box::new((*self)(pod, a1)))
    }
}

impl<A1: Clone + 'static, A2: Clone + 'static, R: 'static> ErasedCall for fn(&dyn Pod, A1, A2) -> R {
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        expect_arity(args, 2)?;
        let a1 = arg::<A1>(args, 0)?.clone();
        let a2 = arg::<A2>(args, 1)?.clone();
        Ok(Box::new((*self)(pod, a1, a2)))
    }
}

impl<A1: Clone + 'static, A2: Clone + 'static, A3: Clone + 'static, R: 'static> ErasedCall
    for fn(&dyn Pod, A1, A2, A3) -> R
{
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        expect_arity(args, 3)?;
        let a1 = arg::<A1>(args, 0)?.clone();
        let a2 = arg::<A2>(args, 1)?.clone();
        let a3 = arg::<A3>(args, 2)?.clone();
        Ok(Box::new((*self)(pod, a1, a2, a3)))
    }
}

/// arity 4（第 BB 批：糖层的 `rgba(r, g, b, a)` 需要四个独立标量）。
impl<
        A1: Clone + 'static,
        A2: Clone + 'static,
        A3: Clone + 'static,
        A4: Clone + 'static,
        R: 'static,
    > ErasedCall for fn(&dyn Pod, A1, A2, A3, A4) -> R
{
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        expect_arity(args, 4)?;
        let a1 = arg::<A1>(args, 0)?.clone();
        let a2 = arg::<A2>(args, 1)?.clone();
        let a3 = arg::<A3>(args, 2)?.clone();
        let a4 = arg::<A4>(args, 3)?.clone();
        Ok(Box::new((*self)(pod, a1, a2, a3, a4)))
    }
}

/// arity 5（第 BB 批：糖层的 `ramp2(ax, ay, bx, by, dur)` 需要五个独立标量）。
impl<
        A1: Clone + 'static,
        A2: Clone + 'static,
        A3: Clone + 'static,
        A4: Clone + 'static,
        A5: Clone + 'static,
        R: 'static,
    > ErasedCall for fn(&dyn Pod, A1, A2, A3, A4, A5) -> R
{
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        expect_arity(args, 5)?;
        let a1 = arg::<A1>(args, 0)?.clone();
        let a2 = arg::<A2>(args, 1)?.clone();
        let a3 = arg::<A3>(args, 2)?.clone();
        let a4 = arg::<A4>(args, 3)?.clone();
        let a5 = arg::<A5>(args, 4)?.clone();
        Ok(Box::new((*self)(pod, a1, a2, a3, a4, a5)))
    }
}

/// 适配器：把 `Fn(&dyn Pod, &[&dyn Any]) -> Result<..>` 包成 [`ErasedCall`]。
///
/// （不能对闭包做 blanket impl —— 那会与上面的 fn 指针 impl 冲突。）
struct ErasedFn<F>(F);

impl<F> ErasedCall for ErasedFn<F>
where
    F: Fn(&dyn Pod, &[&dyn Any]) -> Result<Box<dyn Any>, PodError> + Send + Sync + 'static,
{
    fn invoke(&self, pod: &dyn Pod, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        (self.0)(pod, args)
    }
}

/// 闭包注册表 Pod。
///
/// `id` 在构造时分配；注册发生在装配期（`&mut self`），调用期只读（`&self`）。
pub struct RegistryPod {
    id: PodId,
    names: Vec<QualifiedName>,
    /// 名字 → 槽位（装配期解析一次；调用期 O(1)，不再 `iter().position`）。
    index: SymbolTable<QualifiedName, u32>,
    typed: Vec<Option<Box<dyn Any + Send + Sync>>>,
    erased: Vec<Option<Box<dyn ErasedCall>>>,
}

impl Default for RegistryPod {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryPod {
    /// 新建空 Pod。
    pub fn new() -> Self {
        Self {
            id: PodId::next(),
            names: Vec::new(),
            index: SymbolTable::new(),
            typed: Vec::new(),
            erased: Vec::new(),
        }
    }

    fn put(
        &mut self,
        name: QualifiedName,
        typed: Option<Box<dyn Any + Send + Sync>>,
        erased: Box<dyn ErasedCall>,
    ) {
        if let Some((_, slot)) = self.index.resolve(&name) {
            let i = *slot as usize;
            self.typed[i] = typed;
            self.erased[i] = Some(erased);
        } else {
            let i = self.names.len() as u32;
            self.index
                .intern(name.clone(), i, Blame::pod("registry"))
                .expect("调用方已确认名字不存在");
            self.names.push(name);
            self.typed.push(typed);
            self.erased.push(Some(erased));
        }
    }

    /// 注册 arity 0 的 typed 函数（类型名由 [`IntoPodTypeName`] 推断）。
    pub fn register_fn0<R>(&mut self, path: &str, f: fn(&dyn Pod) -> R)
    where
        R: IntoPodTypeName + 'static,
    {
        self.register_fn0_named::<R>(path, R::NAME, f);
    }

    /// 注册 arity 0 的 typed 函数（显式类型名）。
    pub fn register_fn0_named<R>(&mut self, path: &str, ret: PodTypeName, f: fn(&dyn Pod) -> R)
    where
        R: 'static,
    {
        let name = QualifiedName::from_path(path, Vec::new(), ret);
        self.put(name, Some(Box::new(f)), Box::new(f));
    }

    /// 注册 arity 1 的 typed 函数（类型名由 [`IntoPodTypeName`] 推断）。
    pub fn register_fn1<A1, R>(&mut self, path: &str, f: fn(&dyn Pod, A1) -> R)
    where
        A1: IntoPodTypeName + Clone + 'static,
        R: IntoPodTypeName + 'static,
    {
        self.register_fn1_named::<A1, R>(path, A1::NAME, R::NAME, f);
    }

    /// 注册 arity 1 的 typed 函数（显式类型名）。
    pub fn register_fn1_named<A1, R>(
        &mut self,
        path: &str,
        param0: PodTypeName,
        ret: PodTypeName,
        f: fn(&dyn Pod, A1) -> R,
    ) where
        A1: Clone + 'static,
        R: 'static,
    {
        let name = QualifiedName::from_path(path, vec![param0.to_string()], ret);
        self.put(name, Some(Box::new(f)), Box::new(f));
    }

    /// 注册 arity 2 的 typed 函数（类型名由 [`IntoPodTypeName`] 推断）。
    pub fn register_fn2<A1, A2, R>(&mut self, path: &str, f: fn(&dyn Pod, A1, A2) -> R)
    where
        A1: IntoPodTypeName + Clone + 'static,
        A2: IntoPodTypeName + Clone + 'static,
        R: IntoPodTypeName + 'static,
    {
        self.register_fn2_named::<A1, A2, R>(path, A1::NAME, A2::NAME, R::NAME, f);
    }

    /// 注册 arity 2 的 typed 函数（显式类型名）。
    pub fn register_fn2_named<A1, A2, R>(
        &mut self,
        path: &str,
        param0: PodTypeName,
        param1: PodTypeName,
        ret: PodTypeName,
        f: fn(&dyn Pod, A1, A2) -> R,
    ) where
        A1: Clone + 'static,
        A2: Clone + 'static,
        R: 'static,
    {
        let name =
            QualifiedName::from_path(path, vec![param0.to_string(), param1.to_string()], ret);
        self.put(name, Some(Box::new(f)), Box::new(f));
    }

    /// 注册 arity 3 的 typed 函数（显式类型名）。
    pub fn register_fn3_named<A1, A2, A3, R>(
        &mut self,
        path: &str,
        param0: PodTypeName,
        param1: PodTypeName,
        param2: PodTypeName,
        ret: PodTypeName,
        f: fn(&dyn Pod, A1, A2, A3) -> R,
    ) where
        A1: Clone + 'static,
        A2: Clone + 'static,
        A3: Clone + 'static,
        R: 'static,
    {
        let name = QualifiedName::from_path(
            path,
            vec![param0.to_string(), param1.to_string(), param2.to_string()],
            ret,
        );
        self.put(name, Some(Box::new(f)), Box::new(f));
    }

    /// 注册 arity 4 的 typed 函数（显式类型名）。
    ///
    /// 第 BB 批：糖层的 `rgba(r, g, b, a)` 用四个独立标量（比一个 4 字段结构体
    /// 更接近作者的写法）。零额外机制：与 arity 1–3 走同一条 `put` 路径。
    #[allow(clippy::too_many_arguments)]
    pub fn register_fn4_named<A1, A2, A3, A4, R>(
        &mut self,
        path: &str,
        param0: PodTypeName,
        param1: PodTypeName,
        param2: PodTypeName,
        param3: PodTypeName,
        ret: PodTypeName,
        f: fn(&dyn Pod, A1, A2, A3, A4) -> R,
    ) where
        A1: Clone + 'static,
        A2: Clone + 'static,
        A3: Clone + 'static,
        A4: Clone + 'static,
        R: 'static,
    {
        let name = QualifiedName::from_path(
            path,
            vec![
                param0.to_string(),
                param1.to_string(),
                param2.to_string(),
                param3.to_string(),
            ],
            ret,
        );
        self.put(name, Some(Box::new(f)), Box::new(f));
    }

    /// 注册 arity 5 的 typed 函数（显式类型名）。
    ///
    /// 第 BB 批：糖层的 `ramp2(ax, ay, bx, by, dur)` 用五个独立标量。
    #[allow(clippy::too_many_arguments)]
    pub fn register_fn5_named<A1, A2, A3, A4, A5, R>(
        &mut self,
        path: &str,
        param0: PodTypeName,
        param1: PodTypeName,
        param2: PodTypeName,
        param3: PodTypeName,
        param4: PodTypeName,
        ret: PodTypeName,
        f: fn(&dyn Pod, A1, A2, A3, A4, A5) -> R,
    ) where
        A1: Clone + 'static,
        A2: Clone + 'static,
        A3: Clone + 'static,
        A4: Clone + 'static,
        A5: Clone + 'static,
        R: 'static,
    {
        let name = QualifiedName::from_path(
            path,
            vec![
                param0.to_string(),
                param1.to_string(),
                param2.to_string(),
                param3.to_string(),
                param4.to_string(),
            ],
            ret,
        );
        self.put(name, Some(Box::new(f)), Box::new(f));
    }

    /// 注册只有慢路径（erased）的函数。
    ///
    /// 它的签名仍进入 [`Pod::functions`]，但 [`Pod::resolve`] 返回 `None`
    /// ⇒ 需要该函数的 capability **协商失败**（全有或全无），只能走 [`Pod::call`]。
    pub fn register_erased<F>(&mut self, path: &str, params: &[PodTypeName], ret: PodTypeName, f: F)
    where
        F: Fn(&dyn Pod, &[&dyn Any]) -> Result<Box<dyn Any>, PodError> + Send + Sync + 'static,
    {
        let name =
            QualifiedName::from_path(path, params.iter().map(|s| (*s).to_string()).collect(), ret);
        self.put(name, None, Box::new(ErasedFn(f)));
    }

    /// 注册的完整函数名数量。
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

impl Pod for RegistryPod {
    fn id(&self) -> PodId {
        self.id
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn functions(&self) -> &[QualifiedName] {
        &self.names
    }

    fn resolve(&self, name: &QualifiedName) -> Option<&(dyn Any + Send + Sync)> {
        let (_, slot) = self.index.resolve(name)?;
        self.typed[*slot as usize].as_deref()
    }

    /// 快路径句柄：装配期建立的符号槽位，调用期 O(1)。
    fn function_id(&self, name: &QualifiedName) -> Option<FunctionId> {
        let (symbol, _) = self.index.resolve(name)?;
        Some(FunctionId::local(
            self.id,
            u64::from(symbol.slot()),
            symbol.generation(),
        ))
    }

    fn call(&self, id: FunctionId, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        id.ensure_owner(self.id)?;
        let slot = u32::try_from(id.slot()).map_err(|_| PodError::UnknownFunction(format!(
            "函数编号 {id}"
        )))?;
        // generation 不匹配 ⇒ 槽位已被复用：显式错误，不是读到别人的函数。
        self.index
            .get(ze_reflect::SymbolId::new(slot, id.generation()))
            .map_err(|_| PodError::StaleFunction {
                slot: id.slot(),
                generation: id.generation(),
            })?;
        let entry = self
            .erased
            .get(id.slot() as usize)
            .and_then(|e| e.as_deref())
            .ok_or_else(|| PodError::UnknownFunction(format!("函数编号 {id}")))?;
        entry.invoke(self, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 函数句柄 = owner + slot + generation：装配期解析一次，调用期 O(1)。
    #[test]
    fn function_ids_are_generation_aware() {
        let mut pod = RegistryPod::new();
        pod.register_fn0::<u64>("test::zero", |_| 7u64);
        let name = QualifiedName::from_path("test::zero", Vec::new(), "u64");

        let id = pod.function_id(&name).expect("已注册");
        assert_eq!(id.owner(), pod.id());
        assert_eq!(
            *pod.call(id, &[]).unwrap().downcast::<u64>().unwrap(),
            7u64
        );

        // generation 不匹配 ⇒ 显式错误，不是读到别人的函数。
        let stale = FunctionId::local(pod.id(), id.slot(), id.generation() + 1);
        assert!(matches!(
            pod.call(stale, &[]),
            Err(PodError::StaleFunction { .. })
        ));

        // owner 不匹配 ⇒ 跨 Pod 句柄。
        let foreign = FunctionId::local(PodId::next(), id.slot(), id.generation());
        assert!(matches!(
            pod.call(foreign, &[]),
            Err(PodError::CrossPodHandle { .. })
        ));

        // 兼容入口（owner = 0 / generation = 0）仍然可用。
        assert!(pod.call(FunctionId::new(id.slot()), &[]).is_ok());
    }

    /// 名字解析走符号表（重名不再静默覆盖）。
    #[test]
    fn re_registration_replaces_the_same_slot() {
        let mut pod = RegistryPod::new();
        pod.register_fn0::<u64>("test::value", |_| 1u64);
        pod.register_fn0::<u64>("test::value", |_| 2u64);
        let name = QualifiedName::from_path("test::value", Vec::new(), "u64");
        let id = pod.function_id(&name).unwrap();
        assert_eq!(
            *pod.call(id, &[]).unwrap().downcast::<u64>().unwrap(),
            2u64
        );
        assert_eq!(pod.len(), 1, "同名重注册复用同一槽位");
    }
}
