// target 的 ECS 侧用例（B3-1b）：target 已下沉 `ze-scene-core`，
// 但用例需要真实的 hecs 场景，故留在本 crate（与实现分离）。
//
// 用例体从 `animation/target.rs` 的 `#[cfg(test)] mod tests` 原样迁来，
// 只把 `use super::*` 换成核心层的显式 import。

use ze_scene_core::animation::target::*;
use ze_scene_core::components::{PhaseAnim, Reveal};
use ze_scene_core::object::{Camera3D, Circle, Framing2D, Transform};
use ze_scene_contract::ObjectId;
use ze_scene_contract::Scene as _;
use ze_render_ir::Opacity;
use ze_time::AnimValue;
use crate::contract::WorldScene;
mod tests {
    use super::*;
    use ze_notion::{Vec2, Vec3};
    use ze_scene_core::Projection;
    // 测试也走契约层（`spawn_dyn` / `with`）—— **本文件不出现 hecs**。
    // （`Scene` trait 已由父模块的 `use ... as _` 经 `use super::*` 带入。）
    use ze_scene_contract::SceneRead;

    use crate::scene::Scene as SceneStruct;

    /// 用契约层 `spawn_dyn` 创建单组件对象（同时验证该组件已登记）。
    fn spawn_one(scene: &mut SceneStruct, c: impl crate::contract::Component) -> ObjectId {
        scene
            .spawn_dyn(vec![Box::new(c)])
            .expect("组件已在登记表中")
    }

    /// 经契约适配器施加一次动画值。
    fn apply_once(t: &dyn Target, scene: &mut SceneStruct, v: AnimValue) -> Result<(), TargetError> {
        t.apply(&mut WorldScene::new(scene.world_mut()), &v)
    }

    /// **#6 硬约束 1**：组件名必须等于组件登记表的名字（不出现第三套命名）。
    #[test]
    fn component_names_come_from_the_registry() {
        use crate::contract::known_type_name;
        assert_eq!(CIRCLE_RADIUS.component, known_type_name::<Circle>().unwrap());
        assert_eq!(PHASE.component, known_type_name::<PhaseAnim>().unwrap());
        assert_eq!(CAMERA_EYE.component, known_type_name::<Camera3D>().unwrap());
        assert_eq!(
            MORPH_T.component,
            known_type_name::<ze_scene_core::components::MorphShape>().unwrap()
        );
        assert_eq!(
            FRAMING2D_SCALE.component,
            known_type_name::<Framing2D>().unwrap()
        );
        assert_eq!(REVEAL_END.component, known_type_name::<Reveal>().unwrap());
        assert_eq!(
            TRANSFORM_POSITION.component,
            known_type_name::<Transform>().unwrap()
        );
        assert_eq!(OPACITY_VALUE.component, known_type_name::<Opacity>().unwrap());

        // `name` 与 `address` 共用**同一个**查表命名空间（见 `target_entry`）：
        // 单字段规格的 `name` 就是 `address`；组合目标没有单一地址，二者也相同。
        // 任一键重复 ⇒ 查表会静默命中错误字段。
        let mut keys = std::collections::BTreeSet::new();
        for entry in TARGETS {
            assert!(
                keys.insert(entry.spec.name),
                "重复的寻址键：{}",
                entry.spec.name
            );
            if entry.spec.address != entry.spec.name {
                assert!(
                    keys.insert(entry.spec.address),
                    "重复的规范地址：{}",
                    entry.spec.address
                );
            }
        }

        // 单字段规格的规范地址必须是 `Component.field`，且查表要命回它自己。
        for entry in TARGETS {
            let spec = entry.spec;
            if spec.address == format!("{}.{}", spec.component, spec.field) {
                let hit = target_entry(spec.address).expect("规范地址可查");
                assert_eq!(hit.spec.name, spec.name);
            }
        }
    }

    /// **#6 硬约束 2**：类型不符是**显式错误**，不是静默跳过。
    #[test]
    fn type_mismatch_is_an_explicit_error() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Circle::new((0.0, 0.0), 1.0));
        let err = apply_once(&CircleRadius { id }, &mut scene, AnimValue::Vec3(Vec3::ONE))
            .expect_err("Vec3 → Circle.radius 必须报错");
        assert_eq!(
            err,
            TargetError::TypeMismatch {
                target: "CircleRadius",
                expected: "scalar",
                actual: "vec3",
            }
        );
    }

    /// **#6 读方向**：read 与 write 是同一个 `FieldSpec` 的两半。
    #[test]
    fn read_direction_comes_from_the_same_spec() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Circle::new((0.0, 0.0), 1.0));
        apply_once(&CircleRadius { id }, &mut scene, AnimValue::Scalar(2.5)).unwrap();
        let value = ze_scene_core::scene_write::read_component::<Circle, _>(
            &WorldScene::new(scene.world_mut()),
            id,
            |c| c.radius,
        )
        .expect("组件在场景里");
        assert_eq!(value, 2.5);
    }

    /// 读回组件（缺失 ⇒ 测试失败）。
    fn read<T: 'static + Send + Sync, R>(
        scene: &SceneStruct,
        id: ObjectId,
        f: impl FnOnce(&T) -> R,
    ) -> R {
        scene.with::<T, _>(id, f).expect("组件在场景里")
    }

    /// `CircleRadius`：`Scalar` 写入半径；类型不符显式报错。
    #[test]
    fn circle_radius_writes_scalar_and_skips_mismatch() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Circle::new((0.0, 0.0), 1.0));
        let t = CircleRadius { id };

        apply_once(&t, &mut scene, AnimValue::Scalar(2.5));
        assert_eq!(read::<Circle, _>(&scene, id, |c| c.radius), 2.5);

        // Vec3 与目标字段不符 ⇒ 跳过，半径保持不变。
        apply_once(&t, &mut scene, AnimValue::Vec3(Vec3::ONE));
        assert_eq!(read::<Circle, _>(&scene, id, |c| c.radius), 2.5);
    }

    /// `Phase`：写 `PhaseAnim.phase`。
    #[test]
    fn phase_writes_phase_anim() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, PhaseAnim::new(0.0, 1.5));
        let t = Phase { id };
        apply_once(&t, &mut scene, AnimValue::Scalar(0.75));
        assert_eq!(read::<PhaseAnim, _>(&scene, id, |c| c.phase), 0.75);
    }

    /// `CameraEye`：写 `Camera3D.eye`。
    #[test]
    fn camera_eye_writes_camera3d() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(
            &mut scene,
            Camera3D::new(
                Vec3::ZERO,
                Vec3::ZERO,
                Vec3::Y,
                Projection::perspective(1.0, 1.0, 0.1, 100.0),
            ),
        );
        let t = CameraEye { id };
        let eye = Vec3::new(1.0, 2.0, 3.0);
        apply_once(&t, &mut scene, AnimValue::Vec3(eye));
        assert_eq!(read::<Camera3D, _>(&scene, id, |c| c.eye), eye);
    }

    /// `Camera2DFraming`：单 target 按 `AnimValue` 变体分派 —— `Scalar` → `scale`、
    /// `Vec2` → `center`（同一条绑定可以只驱动其中一个）。
    #[test]
    fn camera_framing_dispatches_by_variant() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Framing2D::new(Vec2::ZERO, 40.0));
        let t = Camera2DFraming { id };

        apply_once(&t, &mut scene, AnimValue::Scalar(120.0));
        assert_eq!(read::<Framing2D, _>(&scene, id, |f| f.scale), 120.0);
        assert_eq!(read::<Framing2D, _>(&scene, id, |f| f.center), Vec2::ZERO);

        apply_once(&t, &mut scene, AnimValue::Vec2(Vec2::new(1.5, 1.0)));
        assert_eq!(
            read::<Framing2D, _>(&scene, id, |f| f.center),
            Vec2::new(1.5, 1.0)
        );
        assert_eq!(read::<Framing2D, _>(&scene, id, |f| f.scale), 120.0);
    }

    /// 失效 / 不存在的句柄：静默跳过（不 panic）。
    #[test]
    fn dangling_handle_is_noop() {
        let mut scene = SceneStruct::new();
        let t = CircleRadius {
            id: ObjectId::DANGLING,
        };
        apply_once(&t, &mut scene, AnimValue::Scalar(1.0));
    }

    /// `RevealRange`：`Scalar` 只推 `end`（"笔尖往前走"），`Vec2` 写整个区间。
    #[test]
    fn reveal_range_dispatches_by_variant() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(
            &mut scene,
            Reveal::new(ze_notion::Path2D::from_polyline(
                &[ze_notion::Vec2::ZERO, ze_notion::Vec2::new(1.0, 0.0)],
                false,
            )),
        );
        let t = RevealRange { id };

        apply_once(&t, &mut scene, AnimValue::Scalar(0.4));
        let (start, end) = read::<Reveal, _>(&scene, id, |r| (r.start, r.end));
        assert_eq!(end, 0.4);
        assert_eq!(start, 0.0, "Scalar 不得改动 start");

        apply_once(&t, &mut scene, AnimValue::Vec2(Vec2::new(0.3, 0.7)));
        assert_eq!(
            read::<Reveal, _>(&scene, id, |r| (r.start, r.end)),
            (0.3, 0.7)
        );

        // 类型不符 ⇒ 跳过（保持上一帧的值）。
        apply_once(&t, &mut scene, AnimValue::Vec3(Vec3::ONE));
        assert_eq!(
            read::<Reveal, _>(&scene, id, |r| (r.start, r.end)),
            (0.3, 0.7)
        );
    }

    /// `name()` / `target_id()` 与实现的对应关系（UI 显示 + 按对象筛选）。
    ///
    /// ⚠️ 这不是"集中清单"——名字**由各 target 自带**；本用例只是把**既有**八个的
    /// 字面量钉住（防手滑改错），新增 target **不需要**在这里加一行。
    #[test]
    fn names_and_target_ids_are_reported() {
        let id = ObjectId::new(7, 3);
        assert_eq!(CircleRadius { id }.name(), "CircleRadius");
        assert_eq!(Phase { id }.name(), "Phase");
        assert_eq!(CameraEye { id }.name(), "CameraEye");
        assert_eq!(MorphT { id }.name(), "MorphT");
        assert_eq!(Camera2DFraming { id }.name(), "Camera2DFraming");
        assert_eq!(RevealRange { id }.name(), "RevealRange");
        assert_eq!(OpacityValue { id }.name(), "OpacityValue");
        assert_eq!(
            TransformChannelValue {
                id,
                channel: TransformChannel::Rotation
            }
            .name(),
            "TransformRotation"
        );
        // 全部 target 都如实报告自己的对象句柄（筛选的唯一依据）。
        assert_eq!(CircleRadius { id }.target_id(), id);
        assert_eq!(
            TransformChannelValue {
                id,
                channel: TransformChannel::Position
            }
            .target_id(),
            id
        );
        assert_eq!(OpacityValue { id }.target_id(), id);
    }

    /// **B7**：`Target.TransformValue` 已删除 ⇒ 按名字反查构造器必须返回 `None`
    /// （脚本 / 旧 ZSB 引用它时在装配期显式失败，不是静默跳过）。
    #[test]
    fn transform_value_is_no_longer_a_target() {
        assert!(
            target_ctor_by_name("TransformValue").is_none(),
            "TransformValue 已删除，不得还能按名字构造"
        );
        // 三条显式通道仍然可用。
        for name in ["TransformPosition", "TransformRotation", "TransformScale"] {
            assert!(target_ctor_by_name(name).is_some(), "{name} 应仍然可用");
        }
    }

    /// `TransformPosition`：`Vec2` → `position.xy`（z 不动）、`Vec3` → 整个 `position`。
    #[test]
    fn transform_position_channel_writes_vec2_and_vec3() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Transform::default());
        let t = TransformChannelValue {
            id,
            channel: TransformChannel::Position,
        };
        let read_tr =
            |scene: &SceneStruct| read::<Transform, _>(scene, id, |tr| (tr.position, tr.scale));

        // 整体写位置。
        apply_once(&t, &mut scene, AnimValue::Vec3(Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(read_tr(&scene).0, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(read_tr(&scene).1, Vec3::ONE, "Position 通道不得改缩放");

        // 再只用 Vec2：xy 变、**z 保持**（混排作品里 z 可能被 3D 内容占用）。
        apply_once(&t, &mut scene, AnimValue::Vec2(Vec2::new(2.0, -3.0)));
        assert_eq!(read_tr(&scene).0, Vec3::new(2.0, -3.0, 3.0), "Vec2 不得清零 z");
        assert_eq!(read_tr(&scene).1, Vec3::ONE, "Vec2 不得改缩放");
    }

    /// **S3**：`Rotation` 通道把 `Scalar` 写成绕 Z 的 `Quat`，且不改位置 / 缩放。
    #[test]
    fn transform_rotation_channel_rotates_about_z() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Transform::default());
        let t = TransformChannelValue {
            id,
            channel: TransformChannel::Rotation,
        };

        apply_once(
            &t,
            &mut scene,
            AnimValue::Scalar(std::f32::consts::FRAC_PI_2),
        );
        let (position, scale, point) = read::<Transform, _>(&scene, id, |tr| {
            (
                tr.position,
                tr.scale,
                tr.to_affine2d().apply_point(Vec2::new(1.0, 0.0)),
            )
        });
        assert_eq!(position, Vec3::ZERO, "Rotation 不得改位置");
        assert_eq!(scale, Vec3::ONE, "Rotation 不得改缩放");
        assert!((point.x - 0.0).abs() < 1e-5, "x = {}", point.x);
        assert!((point.y - 1.0).abs() < 1e-5, "y = {}", point.y);
    }

    /// **S3**：`Position` / `Rotation` / `Scale` 三段绑定各写各的通道，互不覆盖。
    #[test]
    fn transform_channels_are_isolated() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Transform::default());

        apply_once(
            &TransformChannelValue {
                id,
                channel: TransformChannel::Position,
            },
            &mut scene,
            AnimValue::Vec2(Vec2::new(2.0, 3.0)),
        );
        apply_once(
            &TransformChannelValue {
                id,
                channel: TransformChannel::Rotation,
            },
            &mut scene,
            AnimValue::Scalar(std::f32::consts::FRAC_PI_2),
        );
        apply_once(
            &TransformChannelValue {
                id,
                channel: TransformChannel::Scale,
            },
            &mut scene,
            AnimValue::Scalar(2.0),
        );

        let (position, scale, point) = read::<Transform, _>(&scene, id, |tr| {
            (
                tr.position,
                tr.scale,
                tr.to_affine2d().apply_point(Vec2::new(1.0, 0.0)),
            )
        });
        assert_eq!(position, Vec3::new(2.0, 3.0, 0.0));
        assert_eq!(scale, Vec3::splat(2.0));
        // 先缩放 (2,0) → 再旋转 90° → (0,2) → 再平移 (2,3) = (2,5)。
        assert!((point.x - 2.0).abs() < 1e-5, "x = {}", point.x);
        assert!((point.y - 5.0).abs() < 1e-5, "y = {}", point.y);
    }

    /// `OpacityValue`：`Scalar` 写入 `Opacity.0`；其它变体跳过（不 panic）。
    #[test]
    fn opacity_value_writes_scalar_and_skips_mismatch() {
        let mut scene = SceneStruct::new();
        let id = spawn_one(&mut scene, Opacity::new(1.0));
        let t = OpacityValue { id };

        apply_once(&t, &mut scene, AnimValue::Scalar(0.25));
        assert_eq!(read::<Opacity, _>(&scene, id, |o| o.0), 0.25);

        apply_once(&t, &mut scene, AnimValue::Vec2(Vec2::new(0.1, 0.2)));
        assert_eq!(
            read::<Opacity, _>(&scene, id, |o| o.0),
            0.25,
            "非 Scalar 变体不得写入（静默跳过）"
        );
    }
}
