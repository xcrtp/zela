// path_follow.rs — PathFollowApplySystem：把 `PathFollow { path, t, orient }` 落成摆放。
//
// 阶段：SceneSync（必须在 BindingApply 之后 —— 后者每帧写 `PathFollow.t`，
// 且要早于 Prepare 的 `TransformHierarchySystem`，父子累乘读的是本帧位置）。
// 纯组件读写（`ctx.world`），不读写任何 Resource。
//
// 与 `MorphApplySystem` 的分工：那里 `t` 驱动**几何**（产出 `Path`），
// 这里 `t` 驱动**摆放**（写同实体的 `Transform`）—— 位置是派生量，
// 与「Transform 只存归档值」的口径一致。

use ze_notion::Quat;
use ze_scene_core::components::PathFollow;
use ze_scene_core::Transform;
use zela_schedule::context::ExecutorContext;
use zela_schedule::system::System;

/// 路径跟随应用：`t` → `Transform.position`（`orient` 时同时写 `rotation`）。
///
/// 位置 / 朝向都由 [`PathFollow::position_at`] / [`PathFollow::angle_at`] 给出 ——
/// 它们与 `Reveal::current_pos` 共用 `Path2D::point_at` / `tangent_at`，
/// 因此"跟随笔尖"与"跟随整条路径"不可能出现两套采样。
///
/// `position.z` **保留原值**：2D 场景里 z 常被 3D 内容占用（与 `Transform.position`
/// 通道的 `Vec2 → (x, y, 保留 z)` 同一口径）。
pub struct PathFollowApplySystem;

impl System for PathFollowApplySystem {
    type Reads = ();
    type Writes = ();

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        for (follow, transform) in ctx.world.query::<(&PathFollow, &mut Transform)>().iter() {
            let t = follow.t;
            let position = follow.position_at(t);
            transform.position.x = position.x;
            transform.position.y = position.y;
            if follow.orient {
                transform.rotation = Quat::from_rotation_z(follow.angle_at(t));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_notion::{Path2D, Vec2, Vec3};
    use zela_schedule::resource::Resources;
    use zela_schedule::scheduler::{Phase, Scheduler};

    fn corner_path() -> Path2D {
        Path2D::from_polyline(
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 2.0),
            ],
            false,
        )
    }

    fn run_one_frame(follow: PathFollow) -> (PathFollow, Transform) {
        let mut world = hecs::World::new();
        let entity = world.spawn((
            follow,
            Transform {
                position: Vec3::new(9.0, 9.0, 0.5),
                ..Default::default()
            },
        ));
        let mut scheduler = Scheduler::new(world, Resources::new());
        scheduler.add_system(Phase::SceneSync, PathFollowApplySystem);
        scheduler.run_frame(0.0);
        let follow = {
            let guard = scheduler.world().get::<&PathFollow>(entity).unwrap();
            (*guard).clone()
        };
        let transform = {
            let guard = scheduler.world().get::<&Transform>(entity).unwrap();
            (*guard).clone()
        };
        (follow, transform)
    }

    /// 判据 2：`t` 走完整条路径，位置与**手写逐帧 Transform** 逐位相同。
    #[test]
    fn follow_matches_handwritten_transforms_bitwise() {
        for t in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let mut follow = PathFollow::new(corner_path());
            follow.t = t;
            follow.orient = false;
            let (follow, transform) = run_one_frame(follow);

            // 手写参照：同一份弧长实现（`point_at`），逐字段比较（不是容差比较）。
            let expected = follow.position_at(t);
            assert_eq!(transform.position.x, expected.x, "t = {t} 的 x 不逐位相同");
            assert_eq!(transform.position.y, expected.y, "t = {t} 的 y 不逐位相同");
            assert_eq!(transform.position.z, 0.5, "z 必须保留原值");
        }
    }

    /// 判据 3：`orient = false` 只写位置 ⇒ 旋转**一位都不动**。
    #[test]
    fn orient_false_leaves_rotation_alone() {
        let mut follow = PathFollow::new(corner_path());
        follow.t = 0.5;
        follow.orient = false;
        let (_, transform) = run_one_frame(follow);
        assert_eq!(transform.rotation, Quat::IDENTITY);
    }

    /// `orient = true`：绕 Z 的角度 = `atan2(tangent.y, tangent.x)`（第二段 ⇒ 90°）。
    #[test]
    fn orient_true_turns_along_the_tangent() {
        let mut follow = PathFollow::new(corner_path());
        // 总长 3；t = 0.5 ⇒ 弧长 1.5 ⇒ 第二段（0,1）→（1,2）方向 (0, 1) ⇒ 90°。
        follow.t = 0.5;
        follow.orient = true;
        let (follow, transform) = run_one_frame(follow);
        assert_eq!(
            transform.rotation,
            Quat::from_rotation_z(follow.angle_at(0.5))
        );
        // 与 `Transform.rotation` 读通道同一口径：绕 Z 的角度 = 2·atan2(z, w)。
        let angle = 2.0 * transform.rotation.z.atan2(transform.rotation.w);
        assert!(
            (angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6,
            "角度应约等于 90°，实际 {angle}"
        );
    }
}
