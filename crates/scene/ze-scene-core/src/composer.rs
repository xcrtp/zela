// composer.rs — **合成器对象化**（S3）+ **CK-f**：声明式图层挂载。
//
// # 它是什么，不是什么
//
// `Composer` 是**场景状态**（layer / blend / opacity），只能经 `Scene::set` 或
// 命令路径修改。它是**声明**，不是每帧遍历的节点。
//
// 本批删掉了旧的 `ComposerQueue` —— 那套「用生命周期事件增量维护自己的派生态」
// 的结构（历史上自述为「待相机无关批处理落地后就是 DrawQueue 的增量来源」），
// 渲染层与 app 层对它**零 `get`**。按元规则 B，这是二义性而不是能力：
// 增量结果必须与全量重算逐位对拍 ⇒ 增量白写，还引入插入顺序依赖。
//
// # 现在：树编译出的 `LayerMount` 描述
//
// ```text
// 声明（Composer 组件 + LifetimeTracked 对象）
//   → layer_mounts(scene)      纯函数、每次重算
//   → Vec<LayerMount>          固定声明顺序（layer, ObjectId）
// ```
//
// 合成求值顺序**不由 spawn 顺序决定**：`layer_mounts` 先按 `LayerId`、再按
// `ObjectId`（声明期 id）排序；内容项也按 `ObjectId` 升序。同一份场景两次调用
// 逐位相同。`LayerMount` 是编译产物（纯值、不进 payload、不持句柄）。

use std::collections::BTreeSet;

use ze_render_ir::{LayerBlend, LayerId};
use ze_scene_contract::{LifetimeTracked, ObjectId, SceneRead};

/// 合成器配置（**场景状态**；只能经 `Scene::set` / 命令路径修改）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Composer {
    /// 目标图层。
    pub layer: LayerId,
    /// 与下层的混合方式。
    pub blend: LayerBlend,
    /// 图层不透明度（[0,1]，消费前钳制）。
    pub opacity: f32,
}

impl Default for Composer {
    fn default() -> Self {
        Self {
            layer: LayerId(0),
            blend: LayerBlend::Alpha,
            opacity: 1.0,
        }
    }
}

impl Composer {
    pub fn new(layer: LayerId, blend: LayerBlend, opacity: f32) -> Self {
        Self {
            layer,
            blend,
            opacity,
        }
    }

    /// 钳制后的不透明度（合成唯一取值口径）。
    pub fn clamped_opacity(&self) -> f32 {
        self.opacity.clamp(0.0, 1.0)
    }
}

/// 合成器的**引用**（声明期 id；不持句柄、不进 payload）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComposerRef(pub ObjectId);

/// 一个图层的挂载描述（合成求值的输入；编译产物，不是运行期关系对象）。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerMount {
    /// 由哪个合成器声明（声明期 id）。
    pub composer: ObjectId,
    /// 目标图层。
    pub layer: LayerId,
    /// 与下层的混合方式。
    pub blend: LayerBlend,
    /// 钳制后的不透明度。
    pub opacity: f32,
    /// 本图层的内容项（按声明期 id 升序）。
    pub items: Vec<ObjectId>,
}

/// **编译**图层挂载描述（纯函数；顺序固定，不随 spawn / 枚举顺序漂移）。
///
/// - 合成器按 `(LayerId, ObjectId)` 升序；
/// - 内容项 = 全部 `LifetimeTracked` 对象（按 `ObjectId` 升序），并排除合成器自身。
pub fn layer_mounts(scene: &impl SceneRead) -> Vec<LayerMount> {
    let mut composers: Vec<(ObjectId, Composer)> = Vec::new();
    scene.for_each::<Composer>(|id, composer| composers.push((id, *composer)));
    composers.sort_by_key(|(id, composer)| (composer.layer.0, *id));

    let mut items: BTreeSet<ObjectId> = BTreeSet::new();
    scene.for_each::<LifetimeTracked>(|id, _| {
        items.insert(id);
    });

    let composer_ids: BTreeSet<ObjectId> = composers.iter().map(|(id, _)| *id).collect();
    let content: Vec<ObjectId> = items
        .into_iter()
        .filter(|id| !composer_ids.contains(id))
        .collect();

    composers
        .into_iter()
        .map(|(composer, config)| LayerMount {
            composer,
            layer: config.layer,
            blend: config.blend,
            opacity: config.clamped_opacity(),
            items: content.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_are_ordered_by_layer_then_declaration() {
        // 直接使用 hecs 之外的轻量实现不可得；这里用一个最小的 `SceneRead` 桩。
        struct Stub {
            composers: Vec<(ObjectId, Composer)>,
            tracked: Vec<ObjectId>,
        }
        impl SceneRead for Stub {
            fn contains(&self, _id: ObjectId) -> bool {
                true
            }
            fn len(&self) -> usize {
                self.composers.len() + self.tracked.len()
            }
            fn for_each<T: 'static + Send + Sync>(&self, mut f: impl FnMut(ObjectId, &T)) {
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<Composer>() {
                    for (id, composer) in &self.composers {
                        let boxed: &dyn std::any::Any = composer;
                        f(*id, boxed.downcast_ref::<T>().expect("类型一致"));
                    }
                }
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<LifetimeTracked>() {
                    let marker = LifetimeTracked;
                    let boxed: &dyn std::any::Any = &marker;
                    for id in &self.tracked {
                        f(*id, boxed.downcast_ref::<T>().expect("类型一致"));
                    }
                }
            }
            fn for_each_pair<A: 'static + Send + Sync, B: 'static + Send + Sync>(
                &self,
                _f: impl FnMut(ObjectId, &A, &B),
            ) {
            }
            fn for_each_pair_opt<
                A: 'static + Send + Sync,
                B: 'static + Send + Sync,
                C: 'static + Send + Sync,
            >(
                &self,
                _f: impl FnMut(ObjectId, &A, &B, Option<&C>),
            ) {
            }
            fn with<T: 'static + Send + Sync, R>(
                &self,
                _id: ObjectId,
                _f: impl FnOnce(&T) -> R,
            ) -> Option<R> {
                None
            }
        }

        let a = ObjectId::new(1, 1);
        let b = ObjectId::new(2, 1);
        let c = ObjectId::new(3, 1);
        let stub = Stub {
            composers: vec![
                (b, Composer::new(LayerId(1), LayerBlend::Alpha, 2.0)),
                (a, Composer::new(LayerId(0), LayerBlend::Alpha, 1.0)),
            ],
            tracked: vec![c, a],
        };
        let mounts = layer_mounts(&stub);
        assert_eq!(mounts.len(), 2);
        // 按图层排序，而不是按插入顺序（插入是 b, a）。
        assert_eq!(mounts[0].composer, a);
        assert_eq!(mounts[1].composer, b);
        assert_eq!(mounts[1].opacity, 1.0, "不透明度在编译时钳制");
        assert_eq!(mounts[0].items, vec![c], "合成器自身不作为内容项");
    }
}
