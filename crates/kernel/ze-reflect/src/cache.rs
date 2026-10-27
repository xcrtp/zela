//! **单态化缓存**：按需生成、按 op 缓存、带上界。
//!
//! 计划是纯函数（`ze-type::gen`），缓存是运行时状态（这里）。
//! 缓存不改变语义：同一份签名永远得到同一组实例。

use std::collections::HashMap;

use ze_type::r#gen::{monomorphize, MAX_INSTANCES};
use ze_type::{Blame, MonoInstance, OpSig, ReflectError};

/// 按算子名缓存单态化实例（上限 [`MAX_INSTANCES`]）。
#[derive(Debug)]
pub struct MonoCache {
    limit: usize,
    entries: HashMap<&'static str, Vec<MonoInstance>>,
}

impl Default for MonoCache {
    fn default() -> Self {
        Self::with_limit(MAX_INSTANCES)
    }
}

impl MonoCache {
    /// 上界默认取 [`MAX_INSTANCES`]；测试可收紧。
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            entries: HashMap::new(),
        }
    }

    /// 取（必要时生成）某算子的实例表。
    ///
    /// 越界 ⇒ 显式错误（上界进产物），不是静默截断。
    pub fn instances(
        &mut self,
        op: &'static str,
        sig: &OpSig,
    ) -> Result<&[MonoInstance], ReflectError> {
        if !self.entries.contains_key(op) {
            let instances = monomorphize(sig)?;
            if instances.len() > self.limit {
                return Err(ReflectError::TypeMismatch {
                    expected: format!("`{op}` 的实例数 ≤ {}", self.limit),
                    actual: format!("{} 个", instances.len()),
                    origin: Blame::Assembly,
                });
            }
            self.entries.insert(op, instances);
        }
        Ok(&self.entries[op])
    }

    /// 已缓存的算子数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_type::{Kind, ScalarKind, TypeSpec};

    const NUM: &TypeSpec = &TypeSpec::Var("T");
    const COS: OpSig = OpSig {
        params: &[NUM],
        ret: NUM,
        vars: &[("T", Kind::Num)],
        variadic: false,
    };

    #[test]
    fn instances_are_computed_once_per_op() {
        let mut cache = MonoCache::default();
        let first: *const MonoInstance = cache.instances("cos", &COS).unwrap().as_ptr();
        let second: *const MonoInstance = cache.instances("cos", &COS).unwrap().as_ptr();
        assert_eq!(first, second, "第二次必须命中缓存而不是重算");
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.instances("cos", &COS).unwrap().len(), 2);
        assert_eq!(
            cache.instances("cos", &COS).unwrap()[0].ret,
            TypeSpec::Scalar(ScalarKind::F32)
        );
    }

    #[test]
    fn the_upper_bound_is_enforced_at_runtime_too() {
        let mut cache = MonoCache::with_limit(1);
        assert!(cache.instances("cos", &COS).is_err());
        assert!(cache.is_empty(), "越界不写入缓存");
    }
}
