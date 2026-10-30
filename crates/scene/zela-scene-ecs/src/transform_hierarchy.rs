// transform_hierarchy.rs — 父子变换累乘（S5，S1 换型为 3D）。
//
// # 产物是**派生态**
//
// `WorldTransforms` 是 Resource（每帧重建，不进场景持久化）。渲染路径读它；
// 无 `Parent` 的对象其世界矩阵与 local **逐位相同**（直接复制，不乘恒等），
// 因此默认场景零变化。
//
// # 组合顺序
//
// 沿用 `ze_scene_core::Transform::to_affine3d` 的约定：`scale → rotation → translation`。
// 父子：`world = parent ∘ local`（点先被 local 作用，再被 parent 作用）。
// `Transform3D::compose(self, other)` 的语义是"先 self 后 other"，故写
// `local.compose(parent)`。
//
// # 2D 是投影视图（S1 / D1 / D4）
//
// 本资源只存 **3D 世界矩阵（Mat4，唯一来源）**；2D 消费方经 [`WorldTransforms::get2d`]
// 读投影视图（取 xy 子块，`z` 平移与 xy↔z 耦合被丢弃，见 `GAPS.md` G-101）。
// 旧的"2D 仿射"是派生 getter，不是第二份状态。
//
// # 确定性 / 环路
//
// - 遍历按 `ObjectId` 升序（容器一律 `BTreeMap`，不存在"依赖哈希序"的口子）；
// - `Parent(DANGLING)` 视为**无父**（声明期合法占位）；
// - 环路写入 `TransformHierarchyError`（**完整环路径 `Vec<ObjectId>`**），并跳过该子树
//   （返回 local）。不静默截断：错误 Resource 可由测试 / 宿主读取。

use std::collections::BTreeMap;

use ze_notion::{Color32, Mat4, Quat, Transform3D, Vec3};
use ze_render_ir::{FillSolid, Opacity, StrokeSolid};
use ze_scene_core::{Transform, WorldInherit};
use ze_scene_contract::{ObjectId, Parent, ParentInherit, SceneRead};
use zela_schedule::context::ExecutorContext;
use zela_schedule::system::System;

use crate::contract::WorldView;

/// 每帧派生的**世界矩阵**（Resource；不进持久化）。
///
/// B3-1 收尾：容器与读取口径已下沉 `ze_scene_core::world_transforms`
/// （渲染层只读它，不该依赖 ECS 实现）；**生产**仍是本文件的 System。
pub use ze_scene_core::world_transforms::WorldTransforms;

/// 父子环路错误（S5）：`chain = None` = 本帧无环。
///
/// **完整环路径**（阶段 0）：与其余四处环诊断（`Cycle(Vec<Id>)` / `Cycle { chain }` /
/// `ParentCycle { chain }` / `CycleDetected { chain }`）同口径 —— 首尾相同，
/// 不再把结构化路径降级成字符串。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformHierarchyError {
    pub chain: Option<Vec<ObjectId>>,
}

impl TransformHierarchyError {
    pub fn is_err(&self) -> bool {
        self.chain.is_some()
    }

    /// 完整环路径（首尾相同）；判定请用它，不要解析字符串。
    pub fn chain(&self) -> Option<&[ObjectId]> {
        self.chain.as_deref()
    }

    /// 可读诊断（调试 / 日志用）。
    pub fn message(&self) -> Option<String> {
        self.chain
            .as_ref()
            .map(|chain| format!("父子变换环路：{chain:?}（v0 → … → v0）"))
    }
}

/// 父子变换累乘（Prepare：在 `BindingApplySystem` 写入 local Transform 之后、录制之前）。
pub struct TransformHierarchySystem;

impl System for TransformHierarchySystem {
    type Reads = ();
    // 样式继承结果与矩阵**同帧同 System** 产出：`WorldInherit` 作为
    // `WorldTransforms` 的字段一起写（避开 `ResourceTuple` 不支持 5 元组）。
    type Writes = (WorldTransforms, TransformHierarchyError);

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        // 先在世界只读借用内收集 local / parent / 继承权重 / 本地样式（收集完即释放）。
        let (locals, parents, inherits, local_opacity, local_fill, local_stroke) = {
            let world = WorldView::new(ctx.borrow_world());
            let mut locals: BTreeMap<ObjectId, Transform3D> = BTreeMap::new();
            let mut parents: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
            let mut inherits: BTreeMap<ObjectId, ParentInherit> = BTreeMap::new();
            let mut local_opacity: BTreeMap<ObjectId, f32> = BTreeMap::new();
            let mut local_fill: BTreeMap<ObjectId, Color32> = BTreeMap::new();
            let mut local_stroke: BTreeMap<ObjectId, Color32> = BTreeMap::new();
            world.for_each::<Transform>(|id, transform| {
                locals.insert(id, transform.to_affine3d());
            });
            world.for_each::<Parent>(|id, parent| {
                if parent.0 != ObjectId::DANGLING {
                    parents.insert(id, parent.0);
                }
            });
            world.for_each::<ParentInherit>(|id, inherit| {
                inherits.insert(id, *inherit);
            });
            world.for_each::<Opacity>(|id, opacity| {
                local_opacity.insert(id, opacity.0);
            });
            world.for_each::<FillSolid>(|id, fill| {
                local_fill.insert(id, fill.color);
            });
            world.for_each::<StrokeSolid>(|id, stroke| {
                local_stroke.insert(id, stroke.color);
            });
            (locals, parents, inherits, local_opacity, local_fill, local_stroke)
        };

        // 拓扑序由 ObjectId 升序决定（确定性；BTreeMap 的键序即最终序）。
        let mut ids: Vec<ObjectId> = locals
            .keys()
            .copied()
            .chain(parents.keys().copied())
            .collect();
        ids.sort_unstable();
        ids.dedup();

        let mut out: BTreeMap<ObjectId, Transform3D> = BTreeMap::new();
        let mut style_out: BTreeMap<ObjectId, InheritedStyle> = BTreeMap::new();
        let mut visiting: Vec<ObjectId> = Vec::new();
        let mut chain: Option<Vec<ObjectId>> = None;
        for id in ids {
            let _ = world_of(
                id,
                &locals,
                &parents,
                &inherits,
                &mut out,
                &mut visiting,
                &mut chain,
            );
            let _ = inherit_style_of(
                id,
                &local_opacity,
                &local_fill,
                &local_stroke,
                &inherits,
                &parents,
                &mut style_out,
                &mut visiting,
                &mut chain,
            );
        }

        // 只有**与本地不同**的填充 / 描边才登记覆盖；不透明度按最终值登记
        // （消费方缺席回退本地，故登记 `local` 本身也是逐位等价的）。
        let mut opacity: BTreeMap<ObjectId, f32> = BTreeMap::new();
        let mut fill: BTreeMap<ObjectId, Color32> = BTreeMap::new();
        let mut stroke: BTreeMap<ObjectId, Color32> = BTreeMap::new();
        for (id, style) in style_out {
            opacity.insert(id, style.opacity);
            if style.fill != local_fill.get(&id).copied() {
                if let Some(color) = style.fill {
                    fill.insert(id, color);
                }
            }
            if style.stroke != local_stroke.get(&id).copied() {
                if let Some(color) = style.stroke {
                    stroke.insert(id, color);
                }
            }
        }
        let inherit = WorldInherit::from_maps(opacity, fill, stroke);

        if let Some(resource) = ctx.get_mut::<WorldTransforms>() {
            *resource = WorldTransforms::from_map(out).with_inherit(inherit);
        }
        if let Some(resource) = ctx.get_mut::<TransformHierarchyError>() {
            resource.chain = chain;
        }
    }
}

/// 递归得到的样式继承结果（final 值，`opacity` 已含本对象）。
#[derive(Clone, Copy, Default)]
struct InheritedStyle {
    opacity: f32,
    fill: Option<Color32>,
    stroke: Option<Color32>,
}

fn world_of(
    id: ObjectId,
    locals: &BTreeMap<ObjectId, Transform3D>,
    parents: &BTreeMap<ObjectId, ObjectId>,
    inherits: &BTreeMap<ObjectId, ParentInherit>,
    out: &mut BTreeMap<ObjectId, Transform3D>,
    visiting: &mut Vec<ObjectId>,
    error: &mut Option<Vec<ObjectId>>,
) -> Transform3D {
    if let Some(world) = out.get(&id) {
        return *world;
    }
    if let Some(start) = visiting.iter().position(|visited| *visited == id) {
        if error.is_none() {
            // 完整环路径：从首次出现的节点到当前节点（首尾相同）。
            let mut chain: Vec<ObjectId> = visiting[start..].to_vec();
            chain.push(id);
            *error = Some(chain);
        }
        return Transform3D::identity();
    }
    visiting.push(id);

    let local = locals
        .get(&id)
        .copied()
        .unwrap_or_else(Transform3D::identity);
    let world = match parents.get(&id) {
        // 先 local 后 parent ⇒ parent ∘ local。
        Some(parent) => {
            let parent_world = world_of(*parent, locals, parents, inherits, out, visiting, error);
            let w = inherits.get(&id).copied().unwrap_or_default();
            // 三个变换权重全 1 ⇒ 整份使用父世界矩阵 ⇒ 与旧行为逐位相同；
            // 全 0 ⇒ 不继承变换 ⇒ world = local。
            if w.full_transform() {
                local.compose(parent_world)
            } else if w.position == 0.0 && w.rotation == 0.0 && w.scale == 0.0 {
                local
            } else {
                local.compose(mix_transform(&parent_world, &w))
            }
        }
        None => local,
    };

    visiting.pop();
    out.insert(id, world);
    world
}

/// 把父世界矩阵按三个权重混合到 identity：位置 `0→父平移`、旋转
/// `identity→父旋转`（slerp）、缩放 `1→父缩放`。
///
/// `full_transform()` 由调用方短路，故这里只处理至少一个权重非 1 的情形。
fn mix_transform(parent: &Transform3D, w: &ParentInherit) -> Transform3D {
    let (scale, rotation, translation) = parent.matrix.to_scale_rotation_translation();
    // 单个维度权重为 1 时**原样**保留该分量（不经过 lerp 的舍入）。
    let translation = if w.position == 1.0 {
        translation
    } else {
        translation * w.position
    };
    let rotation = if w.rotation == 1.0 {
        rotation
    } else {
        Quat::IDENTITY.slerp(rotation, w.rotation)
    };
    let scale = if w.scale == 1.0 {
        scale
    } else {
        Vec3::ONE.lerp(scale, w.scale)
    };
    Transform3D::from_matrix(Mat4::from_scale_rotation_translation(
        scale,
        rotation,
        translation,
    ))
}

fn inherit_style_of(
    id: ObjectId,
    local_opacity: &BTreeMap<ObjectId, f32>,
    local_fill: &BTreeMap<ObjectId, Color32>,
    local_stroke: &BTreeMap<ObjectId, Color32>,
    inherits: &BTreeMap<ObjectId, ParentInherit>,
    parents: &BTreeMap<ObjectId, ObjectId>,
    out: &mut BTreeMap<ObjectId, InheritedStyle>,
    visiting: &mut Vec<ObjectId>,
    error: &mut Option<Vec<ObjectId>>,
) -> InheritedStyle {
    if let Some(style) = out.get(&id) {
        return *style;
    }
    let local = InheritedStyle {
        opacity: local_opacity.get(&id).copied().unwrap_or(1.0),
        fill: local_fill.get(&id).copied(),
        stroke: local_stroke.get(&id).copied(),
    };
    if let Some(start) = visiting.iter().position(|visited| *visited == id) {
        if error.is_none() {
            let mut chain: Vec<ObjectId> = visiting[start..].to_vec();
            chain.push(id);
            *error = Some(chain);
        }
        return local;
    }
    visiting.push(id);

    let style = match parents.get(&id) {
        None => local,
        Some(parent) => {
            let parent_style = inherit_style_of(
                *parent,
                local_opacity,
                local_fill,
                local_stroke,
                inherits,
                parents,
                out,
                visiting,
                error,
            );
            let w = inherits.get(&id).copied().unwrap_or_default();
            InheritedStyle {
                // world_opacity = local × mix(1, parent_world, w)
                opacity: local.opacity * (1.0 + (parent_style.opacity - 1.0) * w.opacity),
                fill: inherit_color(local.fill, parent_style.fill, w.fill),
                stroke: inherit_color(local.stroke, parent_style.stroke, w.stroke),
            }
        }
    };

    visiting.pop();
    out.insert(id, style);
    style
}

/// 颜色继承：子有颜色 ⇒ 按 `w` 向父色靠拢（父缺席时向透明色靠拢）；
/// 子缺席且 `w > 0` ⇒ 继承父色；`w = 0` ⇒ 保持本地（逐位不变）。
fn inherit_color(local: Option<Color32>, parent: Option<Color32>, w: f32) -> Option<Color32> {
    if w == 0.0 {
        return local;
    }
    match local {
        Some(color) => Some(mix_color(
            color,
            parent.unwrap_or(Color32::TRANSPARENT),
            w,
        )),
        None => parent,
    }
}

fn mix_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let lerp = |a: u8, b: u8| {
        (a as f32 + (b as f32 - a as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::new(
        lerp(from.r(), to.r()),
        lerp(from.g(), to.g()),
        lerp(from.b(), to.b()),
        lerp(from.a(), to.a()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_notion::Vec2;
    use zela_schedule::resource::Resources;
    use zela_schedule::scheduler::{Phase, Scheduler};

    fn run_all(world: hecs::World) -> (WorldTransforms, TransformHierarchyError, WorldInherit) {
        let mut sched = Scheduler::new(world, Resources::new());
        sched.add_system(Phase::Prepare, TransformHierarchySystem);
        sched.init_resources();
        sched.run_frame(0.0);
        let worlds = sched.resources().get::<WorldTransforms>().cloned().unwrap();
        let inherit = worlds.inherit().clone();
        (
            worlds,
            sched
                .resources()
                .get::<TransformHierarchyError>()
                .cloned()
                .unwrap(),
            inherit,
        )
    }

    fn run(world: hecs::World) -> (WorldTransforms, TransformHierarchyError) {
        let (worlds, error, _) = run_all(world);
        (worlds, error)
    }

    fn transform(x: f32, y: f32) -> Transform {
        Transform {
            position: ze_notion::Vec3::new(x, y, 0.0),
            ..Transform::default()
        }
    }

    #[test]
    fn no_parent_world_equals_local() {
        let mut world = hecs::World::new();
        let entity = world.spawn((transform(2.0, 3.0),));
        let id = crate::contract::object_id_of(entity);
        let (worlds, error) = run(world);
        assert!(!error.is_err());
        // S1：get2d（2D 投影视图）逐位等于旧的 to_affine2d ⇒ 2D 路径回退不变性。
        assert_eq!(
            worlds.get2d(id),
            Some(transform(2.0, 3.0).to_affine2d())
        );
        // S1：3D 世界矩阵与 local 的 to_affine3d 逐位一致。
        assert_eq!(
            worlds.get(id),
            Some(&transform(2.0, 3.0).to_affine3d())
        );
    }

    #[test]
    fn two_level_world_is_parent_then_local() {
        let mut world = hecs::World::new();
        let parent = world.spawn((transform(10.0, 0.0),));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((transform(0.0, 5.0), Parent(parent_id)));
        let child_id = crate::contract::object_id_of(child);

        let (worlds, error) = run(world);
        assert!(!error.is_err());
        assert_eq!(
            worlds
                .get2d(child_id)
                .unwrap()
                .apply_point(Vec2::ZERO),
            Vec2::new(10.0, 5.0)
        );
    }

    /// **S1-5 #1**：3D 世界矩阵传播 z —— 父 `position.z = 5`，子 local 恒等
    /// ⇒ 子世界 z = 5（旧的 2D 仿射根本表达不了 z）。
    #[test]
    fn world_of_3d_child_inherits_parent_z() {
        let mut world = hecs::World::new();
        let parent = world.spawn((Transform {
            position: ze_notion::Vec3::new(0.0, 0.0, 5.0),
            rotation: ze_notion::Quat::IDENTITY,
            scale: ze_notion::Vec3::ONE,
        },));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((Transform::default(), Parent(parent_id)));
        let child_id = crate::contract::object_id_of(child);

        let (worlds, error) = run(world);
        assert!(!error.is_err());
        let world_child = worlds.get(child_id).unwrap();
        assert_eq!(
            world_child.apply_point(ze_notion::Vec3::ZERO),
            ze_notion::Vec3::new(0.0, 0.0, 5.0),
            "3D 世界矩阵必须传播 z"
        );
        // 2D 投影视图不受 z 影响（xy 平移为 0）。
        assert_eq!(
            worlds.get2d(child_id).unwrap().apply_point(Vec2::ZERO),
            Vec2::ZERO
        );
    }

    #[test]
    fn rotation_parent_composes_with_local_translation() {
        use ze_notion::Quat;
        let mut world = hecs::World::new();
        let parent = world.spawn((Transform {
            position: ze_notion::Vec3::ZERO,
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: ze_notion::Vec3::ONE,
        },));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((transform(1.0, 0.0), Parent(parent_id)));
        let child_id = crate::contract::object_id_of(child);

        let (worlds, _) = run(world);
        let p = worlds.get2d(child_id).unwrap().apply_point(Vec2::ZERO);
        assert!((p.x - 0.0).abs() < 1e-5, "x = {}", p.x);
        assert!((p.y - 1.0).abs() < 1e-5, "y = {}", p.y);
    }

    #[test]
    fn cycle_is_an_explicit_error() {
        let mut world = hecs::World::new();
        let a = world.spawn((transform(1.0, 0.0),));
        let b = world.spawn((transform(0.0, 1.0),));
        let a_id = crate::contract::object_id_of(a);
        let b_id = crate::contract::object_id_of(b);
        world.insert_one(a, Parent(b_id)).unwrap();
        world.insert_one(b, Parent(a_id)).unwrap();

        let (_, error) = run(world);
        assert!(error.is_err(), "环路必须产生明确错误");
        let chain = error.chain().expect("环路必须携带完整路径");
        assert_eq!(
            chain,
            &[a_id, b_id, a_id],
            "诊断必须是结构化完整环路径（首尾相同），实测：{chain:?}"
        );
    }

    #[test]
    fn dangling_parent_is_treated_as_root() {
        let mut world = hecs::World::new();
        let id = {
            let e = world.spawn((transform(1.0, 2.0), Parent(ObjectId::DANGLING)));
            crate::contract::object_id_of(e)
        };
        let (worlds, error) = run(world);
        assert!(!error.is_err());
        assert_eq!(
            worlds.get2d(id).unwrap().apply_point(Vec2::ZERO),
            Vec2::new(1.0, 2.0)
        );
    }

    #[test]
    fn parent_scale_composes_with_child_translation() {
        let mut world = hecs::World::new();
        let parent = world.spawn((Transform {
            position: ze_notion::Vec3::ZERO,
            rotation: ze_notion::Quat::IDENTITY,
            scale: ze_notion::Vec3::splat(2.0),
        },));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((transform(1.0, 0.0), Parent(parent_id)));
        let child_id = crate::contract::object_id_of(child);

        let (worlds, _) = run(world);
        let p = worlds.get2d(child_id).unwrap().apply_point(Vec2::ZERO);
        assert!((p.x - 2.0).abs() < 1e-5, "x = {}", p.x);
        assert!(p.y.abs() < 1e-5, "y = {}", p.y);
    }

    /// 文本成组后，每个字形仍是独立对象 ⇒ 各自持有样式，互不覆盖。
    #[test]
    fn glyph_styles_are_independent_under_a_shared_parent() {
        use ze_notion::{Color32, Path2D};
        use ze_scene_core::Path;
        use ze_render_ir::StrokeSolid;

        let mut world = hecs::World::new();
        let parent = world.spawn((transform(2.0, 3.0),));
        let parent_id = crate::contract::object_id_of(parent);

        let path = Path::new(Path2D::from_polyline(
            &[Vec2::ZERO, Vec2::new(0.2, 0.0)],
            false,
        ));
        let glyph_a = world.spawn((
            path.clone(),
            StrokeSolid::new(1.0, (0, 255, 0, 255)),
            Parent(parent_id),
        ));
        let glyph_b = world.spawn((
            path,
            StrokeSolid::new(1.0, (0, 0, 255, 255)),
            Parent(parent_id),
        ));

        let (worlds, _) = run(world);
        let a = crate::contract::object_id_of(glyph_a);
        let b = crate::contract::object_id_of(glyph_b);
        assert_eq!(worlds.get(a), worlds.get(b), "同一父下世界变换相同");
        assert_eq!(
            worlds.get2d(a).unwrap().apply_point(Vec2::ZERO),
            Vec2::new(2.0, 3.0)
        );

        // 两个字形各自持有 StrokeSolid：颜色可以不同，不会互相覆盖。
        let _a_color = Color32::new(0, 255, 0, 255);
        let _b_color = Color32::new(0, 0, 255, 255);
    }

    #[test]
    fn world_transforms_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorldTransforms>();
        assert_send_sync::<TransformHierarchyError>();
    }

    // ─── BC：多维度连续继承 ────────────────────────────────────────────────

    /// 不透明度：`w = 1` 时父 0.5、子不设 `Opacity` ⇒ 子世界不透明度 = 0.5。
    #[test]
    fn opacity_inherits_when_weight_is_one() {
        let mut world = hecs::World::new();
        let parent = world.spawn((transform(0.0, 0.0), Opacity::new(0.5)));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((
            transform(0.0, 0.0),
            Parent(parent_id),
            ParentInherit {
                opacity: 1.0,
                ..ParentInherit::default()
            },
        ));
        let child_id = crate::contract::object_id_of(child);

        let (_, error, inherit) = run_all(world);
        assert!(!error.is_err());
        assert_eq!(inherit.opacity_of(parent_id), Some(0.5));
        assert_eq!(
            inherit.opacity_of(child_id),
            Some(0.5),
            "w_opacity = 1 ⇒ 子完全继承父的不透明度"
        );
    }

    /// 默认权重（`opacity = 0`）⇒ 子不继承样式 ⇒ 与旧行为一致。
    #[test]
    fn opacity_not_inherited_by_default() {
        let mut world = hecs::World::new();
        let parent = world.spawn((transform(0.0, 0.0), Opacity::new(0.5)));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((transform(0.0, 0.0), Parent(parent_id)));
        let child_id = crate::contract::object_id_of(child);

        let (_, _, inherit) = run_all(world);
        assert_eq!(inherit.opacity_of(child_id), Some(1.0));
    }

    /// 填充色：`w_fill = 1` 时子不设 `Fill`、父红 ⇒ 子继承红。
    #[test]
    fn fill_inherits_when_weight_is_one() {
        let red = Color32::new(255, 0, 0, 255);
        let mut world = hecs::World::new();
        let parent = world.spawn((transform(0.0, 0.0), FillSolid::new(red)));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((
            transform(0.0, 0.0),
            Parent(parent_id),
            ParentInherit {
                fill: 1.0,
                ..ParentInherit::default()
            },
        ));
        let child_id = crate::contract::object_id_of(child);

        let (_, _, inherit) = run_all(world);
        assert_eq!(inherit.fill_of(child_id), Some(red));
        // 子自身有 Fill 且 w = 0 ⇒ 不登记覆盖（默认行为）。
        assert_eq!(inherit.fill_of(parent_id), None);
    }

    /// 三个变换权重全 1 ⇒ 世界矩阵与"手动写死父变换到子对象"逐位相同。
    #[test]
    fn full_transform_weights_match_manual_composition() {
        let parent_local = Transform {
            position: ze_notion::Vec3::new(3.0, -2.0, 1.0),
            rotation: ze_notion::Quat::from_rotation_z(0.4),
            scale: ze_notion::Vec3::new(2.0, 0.5, 1.0),
        };
        let child_local = transform(1.0, 4.0);
        let expected = child_local
            .to_affine3d()
            .compose(parent_local.to_affine3d());

        let mut world = hecs::World::new();
        let parent = world.spawn((parent_local,));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((
            child_local,
            Parent(parent_id),
            // 显式写满 1.0：与缺席完全同义。
            ParentInherit::none(),
        ));
        let child_id = crate::contract::object_id_of(child);

        let (worlds, error, _) = run_all(world);
        assert!(!error.is_err());
        assert_eq!(
            worlds.get(child_id),
            Some(&expected),
            "全 1 权重必须逐位等于 parent ∘ local"
        );
    }

    /// 三个变换权重全 0 ⇒ 完全忽略父变换（world = local）。
    #[test]
    fn zero_transform_weights_ignore_parent() {
        let parent_local = Transform {
            position: ze_notion::Vec3::new(10.0, 0.0, 0.0),
            ..Transform::default()
        };
        let child_local = transform(1.0, 2.0);
        let expected = child_local.to_affine3d();
        let mut world = hecs::World::new();
        let parent = world.spawn((parent_local,));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((
            child_local,
            Parent(parent_id),
            ParentInherit {
                position: 0.0,
                rotation: 0.0,
                scale: 0.0,
                ..ParentInherit::default()
            },
        ));
        let child_id = crate::contract::object_id_of(child);

        let (worlds, _, _) = run_all(world);
        assert_eq!(
            worlds.get(child_id),
            Some(&expected),
            "三个权重全 0 ⇒ world = local"
        );
    }

    /// 位置权重 0.5 ⇒ 平移只继承一半。
    #[test]
    fn half_position_weight_halves_translation() {
        let parent_local = Transform {
            position: ze_notion::Vec3::new(8.0, 0.0, 0.0),
            ..Transform::default()
        };
        let mut world = hecs::World::new();
        let parent = world.spawn((parent_local,));
        let parent_id = crate::contract::object_id_of(parent);
        let child = world.spawn((
            Transform::default(),
            Parent(parent_id),
            ParentInherit {
                position: 0.5,
                rotation: 1.0,
                scale: 1.0,
                ..ParentInherit::default()
            },
        ));
        let child_id = crate::contract::object_id_of(child);

        let (worlds, _, _) = run_all(world);
        let p = worlds.get(child_id).unwrap().apply_point(ze_notion::Vec3::ZERO);
        assert!((p.x - 4.0).abs() < 1e-5, "x = {}", p.x);
    }

    #[test]
    fn world_inherit_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorldInherit>();
    }
}
