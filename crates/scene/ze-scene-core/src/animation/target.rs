// target.rs — 绑定目标：**数据化寻址**（提案 #6）。
//
// # 为什么不再是一个集中枚举
//
// 旧形态 `TargetKind` 把「组件 + 字段 + 类型 + 写入口」四件事钉在 8 个手写变体上：
// 新增字段要改枚举、改 `name()`、改 `ctor()`、改集中表 —— 也就是计划里说的
// 「加一个改三处」。本批把它换成**数据**：
//
// ```text
// address = (组件名, 字段路径) + TypeSpec + 读写方向
// ```
//
// 组件名复用组件登记表（`known_type_name`），字段名就是 Rust 字段名；
// 一个 [`FieldSpec`] 同时持有**写**与**读**两半 —— read 自动获得，不需要第二张表。
//
// # 三条硬约束（§4.2 / §8）
//
// 1. **类型不符显式报错**：`apply` 返回 `Result`；不再静默跳过本帧；
// 2. **coercion 独立成层**：`Scalar → Quat` / `Scalar → Vec3` 是值转换，
//    放在 `coerce` 模块，不混进地址；
// 3. **命名唯一真源**：`component` 必须等于组件登记表的名字（有测试钉住），
//    持久化名沿用旧名（`CircleRadius`），保证既有 payload 不变。
//
// # 兼容
//
// `Circle.radius` 与旧名 `CircleRadius` 指向**同一个** [`FieldSpec`]；后者是
// `persist_name`，前者只是别名。这样老脚本 / 老产物一个字都不用改，
// 新脚本可以用规范地址。

use std::sync::Arc;

use ze_pod_core::{ScalarKind, TypeSpec};
use ze_scene_contract::ObjectId;
use ze_time::AnimValue;

use crate::components::{MorphShape, PhaseAnim, Reveal};
use crate::object::{Camera3D, Circle, Framing2D, Transform};
use crate::scene_write::{read_component, with_component, SceneRead, SceneWrite};
use ze_render_ir::Opacity;

/// 目标写入失败（**显式**，不静默跳过）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetError {
    /// 驱动值类型与字段类型不符。
    TypeMismatch {
        target: &'static str,
        expected: &'static str,
        actual: &'static str,
    },
}

impl TargetError {
    fn mismatch(target: &'static str, expected: &'static str, value: &AnimValue) -> Self {
        Self::TypeMismatch {
            target,
            expected,
            actual: value_kind(value),
        }
    }
}

fn value_kind(value: &AnimValue) -> &'static str {
    match value {
        AnimValue::Scalar(_) => "scalar",
        AnimValue::Vec2(_) => "vec2",
        AnimValue::Vec3(_) => "vec3",
        AnimValue::Color(_) => "color",
        AnimValue::Path(_) => "path",
    }
}

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypeMismatch {
                target,
                expected,
                actual,
            } => write!(f, "目标 `{target}` 期望 {expected}，实际 {actual}"),
        }
    }
}

impl std::error::Error for TargetError {}

/// 驱动目标：把曲线 / 表达式产出的 [`AnimValue`] 写进**某对象**的**某个字段**。
///
/// 对象用 [`ObjectId`] 而非 `hecs::Entity`：target 不依赖 hecs，且能接上
/// `ObjectId::is_stale` 的失效检测。
pub trait Target: Send + Sync + std::fmt::Debug + 'static {
    /// 把本帧的值写入世界；类型不符 ⇒ [`TargetError::TypeMismatch`]。
    fn apply(&self, scene: &mut dyn SceneWrite, v: &AnimValue) -> Result<(), TargetError>;

    /// 目标**持久化名**（旧名优先，见文件头「兼容」）。
    fn name(&self) -> &'static str;

    /// 目标作用的**对象句柄**。
    fn target_id(&self) -> ObjectId;

    /// 读取当前值（`reset = Reset` 的**初值**快照；不支持的实现返回 `None`）。
    ///
    /// 默认实现返回 `None`：只有字段目标（[`FieldTarget`]）知道如何读回自己。
    fn read(&self, _scene: &dyn SceneRead) -> Option<AnimValue> {
        None
    }
}

/// 声明期可引用的目标构造器（`commit` 时按真实 id 生成 trait object）。
pub trait TargetCtor: Send + Sync + 'static {
    fn make(id: ObjectId) -> Arc<dyn Target>;
}

/// **组件字段地址 + 读写入口**（唯一真源）。
#[derive(Clone, Copy, Debug)]
pub struct FieldSpec {
    /// 持久化名（旧 payload 用的名字；新目标可直接用规范地址）。
    pub name: &'static str,
    /// 规范地址 `Component.field`。
    pub address: &'static str,
    /// 组件登记表里的稳定名字（有测试钉住）。
    pub component: &'static str,
    /// Rust 字段名。
    pub field: &'static str,
    /// 字段类型（声明文件 / 通道常量用）。
    pub ty: &'static TypeSpec,
    /// 写入；**类型不符必须报错**。
    pub write: fn(&mut dyn SceneWrite, ObjectId, &AnimValue) -> Result<(), TargetError>,
    /// 读取（read 与 write 同源）。
    pub read: fn(&dyn SceneRead, ObjectId) -> Option<AnimValue>,
}

/// 一个具体的目标实例：`(对象, 字段规格)`。
#[derive(Debug, Clone, Copy)]
pub struct FieldTarget {
    pub id: ObjectId,
    pub spec: &'static FieldSpec,
}

impl Target for FieldTarget {
    fn apply(&self, scene: &mut dyn SceneWrite, v: &AnimValue) -> Result<(), TargetError> {
        (self.spec.write)(scene, self.id, v)
    }

    fn name(&self) -> &'static str {
        self.spec.name
    }

    fn target_id(&self) -> ObjectId {
        self.id
    }

    fn read(&self, scene: &dyn SceneRead) -> Option<AnimValue> {
        (self.spec.read)(scene, self.id)
    }
}

// ─── 值转换（coercion）：**独立成层**，不混进寻址 ──────────────────────

mod coerce {
    use ze_notion::{Quat, Vec3};
    use ze_time::AnimValue;

    /// 标量 → 绕 Z 旋转。
    pub fn rotation_z(value: &AnimValue) -> Option<Quat> {
        value.try_scalar().map(Quat::from_rotation_z)
    }

    /// 标量 → 统一缩放。
    pub fn splat3(value: &AnimValue) -> Option<Vec3> {
        value.try_scalar().map(Vec3::splat)
    }

    /// `Vec2` → `(x, y, 保留原 z)` / `Vec3` 原样。
    ///
    /// 保留 z 不是细节：2D 场景里 `Transform.position.z` 常被 3D 内容占用，
    /// 位置通道只写 xy 时不得把 z 清零（否则混排作品会整体下沉）。
    pub fn position3(value: &AnimValue, z: f32) -> Option<Vec3> {
        if let Some(v) = value.try_vec3() {
            return Some(v);
        }
        value.try_vec2().map(|v| Vec3::new(v.x, v.y, z))
    }
}

// ─── 字段规格（新增字段只写这里一行）─────────────────────────────────

macro_rules! scalar_spec {
    ($ident:ident, $name:literal, $address:literal, $component:ty, $field:ident) => {
        pub static $ident: FieldSpec = FieldSpec {
            name: $name,
            address: $address,
            component: stringify!($component),
            field: stringify!($field),
            ty: &TypeSpec::Scalar(ScalarKind::F32),
            write: |scene, id, value| {
                let value = value
                    .try_scalar()
                    .ok_or_else(|| TargetError::mismatch($name, "scalar", value))?;
                with_component::<$component, _>(scene, id, |c| c.$field = value);
                Ok(())
            },
            read: |scene, id| {
                read_component::<$component, _>(scene, id, |c| AnimValue::Scalar(c.$field))
            },
        };
    };
}

macro_rules! vec2_spec {
    ($ident:ident, $name:literal, $address:literal, $component:ty, $field:ident) => {
        pub static $ident: FieldSpec = FieldSpec {
            name: $name,
            address: $address,
            component: stringify!($component),
            field: stringify!($field),
            ty: &TypeSpec::Struct {
                name: "zela.Vec2",
                fields: &[],
            },
            write: |scene, id, value| {
                let value = value
                    .try_vec2()
                    .ok_or_else(|| TargetError::mismatch($name, "vec2", value))?;
                with_component::<$component, _>(scene, id, |c| c.$field = value);
                Ok(())
            },
            read: |scene, id| {
                read_component::<$component, _>(scene, id, |c| AnimValue::Vec2(c.$field))
            },
        };
    };
}

macro_rules! vec3_spec {
    ($ident:ident, $name:literal, $address:literal, $component:ty, $field:ident) => {
        pub static $ident: FieldSpec = FieldSpec {
            name: $name,
            address: $address,
            component: stringify!($component),
            field: stringify!($field),
            ty: &TypeSpec::Struct {
                name: "zela.Vec3",
                fields: &[],
            },
            write: |scene, id, value| {
                let value = value
                    .try_vec3()
                    .ok_or_else(|| TargetError::mismatch($name, "vec3", value))?;
                with_component::<$component, _>(scene, id, |c| c.$field = value);
                Ok(())
            },
            read: |scene, id| {
                read_component::<$component, _>(scene, id, |c| AnimValue::Vec3(c.$field))
            },
        };
    };
}

scalar_spec!(CIRCLE_RADIUS, "CircleRadius", "Circle.radius", Circle, radius);
scalar_spec!(PHASE, "Phase", "PhaseAnim.phase", PhaseAnim, phase);
scalar_spec!(MORPH_T, "MorphT", "MorphShape.t", MorphShape, t);
vec3_spec!(CAMERA_EYE, "CameraEye", "Camera3D.eye", Camera3D, eye);
vec2_spec!(
    FRAMING2D_CENTER,
    "Framing2D.center",
    "Framing2D.center",
    Framing2D,
    center
);
scalar_spec!(
    FRAMING2D_SCALE,
    "Framing2D.scale",
    "Framing2D.scale",
    Framing2D,
    scale
);
scalar_spec!(REVEAL_END, "Reveal.end", "Reveal.end", Reveal, end);

/// 旧名 `Camera2DFraming`：`Scalar → scale`、`Vec2 → center`（一个 target 两种值型）。
///
/// 组合目标**没有单一字段地址**，故 `address` 与持久化名相同 ——
/// `Framing2D.scale` / `Framing2D.center` 分别属于下面两张单字段规格。
pub static CAMERA2D_FRAMING: FieldSpec = FieldSpec {
    name: "Camera2DFraming",
    address: "Camera2DFraming",
    component: "Framing2D",
    field: "scale",
    ty: &TypeSpec::Scalar(ScalarKind::F32),
    write: |scene, id, value| {
        if let Some(scale) = value.try_scalar() {
            with_component::<Framing2D, _>(scene, id, |f| f.scale = scale);
            Ok(())
        } else if let Some(center) = value.try_vec2() {
            with_component::<Framing2D, _>(scene, id, |f| f.center = center);
            Ok(())
        } else {
            Err(TargetError::mismatch("Camera2DFraming", "scalar | vec2", value))
        }
    },
    read: |scene, id| {
        read_component::<Framing2D, _>(scene, id, |f| AnimValue::Scalar(f.scale))
    },
};

/// 旧名 `RevealRange`：`Scalar → end`（笔尖）、`Vec2 → (start, end)`（区间）。
///
/// 组合目标没有单一字段地址；`Reveal.end` / `Reveal.range` 见下方两张单字段规格。
pub static REVEAL_RANGE: FieldSpec = FieldSpec {
    name: "RevealRange",
    address: "RevealRange",
    component: "Reveal",
    field: "end",
    ty: &TypeSpec::Scalar(ScalarKind::F32),
    write: |scene, id, value| {
        if let Some(end) = value.try_scalar() {
            with_component::<Reveal, _>(scene, id, |r| r.end = end);
            Ok(())
        } else if let Some(range) = value.try_vec2() {
            with_component::<Reveal, _>(scene, id, |r| {
                r.start = range.x;
                r.end = range.y;
            });
            Ok(())
        } else {
            Err(TargetError::mismatch("RevealRange", "scalar | vec2", value))
        }
    },
    read: |scene, id| read_component::<Reveal, _>(scene, id, |r| {
        AnimValue::Vec2(ze_notion::Vec2::new(r.start, r.end))
    }),
};

/// 规范地址 `Reveal.range`（只接受 `Vec2`）。
pub static REVEAL_RANGE_ADDR: FieldSpec = FieldSpec {
    name: "Reveal.range",
    address: "Reveal.range",
    component: "Reveal",
    field: "range",
    ty: &TypeSpec::Struct {
        name: "zela.Vec2",
        fields: &[],
    },
    write: |scene, id, value| {
        let range = value
            .try_vec2()
            .ok_or_else(|| TargetError::mismatch("Reveal.range", "vec2", value))?;
        with_component::<Reveal, _>(scene, id, |r| {
            r.start = range.x;
            r.end = range.y;
        });
        Ok(())
    },
    read: |scene, id| read_component::<Reveal, _>(scene, id, |r| {
        AnimValue::Vec2(ze_notion::Vec2::new(r.start, r.end))
    }),
};

/// 写 `Transform.position`（`Vec2 → xy`，`Vec3` 整体）。
pub static TRANSFORM_POSITION: FieldSpec = FieldSpec {
    name: "TransformPosition",
    address: "Transform.position",
    component: "Transform",
    field: "position",
    ty: &TypeSpec::Struct {
        name: "zela.Vec3",
        fields: &[],
    },
    write: |scene, id, value| {
        if value.try_vec2().is_none() && value.try_vec3().is_none() {
            return Err(TargetError::mismatch(
                "TransformPosition",
                "vec2 | vec3",
                value,
            ));
        }
        with_component::<Transform, _>(scene, id, |t| {
            if let Some(position) = coerce::position3(value, t.position.z) {
                t.position = position;
            }
        });
        Ok(())
    },
    read: |scene, id| {
        read_component::<Transform, _>(scene, id, |t| AnimValue::Vec3(t.position))
    },
};

/// 写 `Transform.rotation`（`Scalar` 弧度 → 绕 Z 四元数）。
pub static TRANSFORM_ROTATION: FieldSpec = FieldSpec {
    name: "TransformRotation",
    address: "Transform.rotation",
    component: "Transform",
    field: "rotation",
    ty: &TypeSpec::Scalar(ScalarKind::F32),
    write: |scene, id, value| {
        let rotation = coerce::rotation_z(value)
            .ok_or_else(|| TargetError::mismatch("TransformRotation", "scalar", value))?;
        with_component::<Transform, _>(scene, id, |t| t.rotation = rotation);
        Ok(())
    },
    read: |scene, id| {
        // 旋转存的是四元数；read 方向返回**绕 Z 的角度**（与写入口互逆）。
        read_component::<Transform, _>(scene, id, |t| {
            AnimValue::Scalar(2.0 * t.rotation.z.atan2(t.rotation.w))
        })
    },
};

/// 写 `Transform.scale`（`Scalar` → 统一缩放）。
pub static TRANSFORM_SCALE: FieldSpec = FieldSpec {
    name: "TransformScale",
    address: "Transform.scale",
    component: "Transform",
    field: "scale",
    ty: &TypeSpec::Scalar(ScalarKind::F32),
    write: |scene, id, value| {
        let scale = coerce::splat3(value)
            .ok_or_else(|| TargetError::mismatch("TransformScale", "scalar", value))?;
        with_component::<Transform, _>(scene, id, |t| t.scale = scale);
        Ok(())
    },
    read: |scene, id| {
        read_component::<Transform, _>(scene, id, |t| AnimValue::Vec3(t.scale))
    },
};

/// 写 `Opacity.value`（`Opacity` 是元组结构体 ⇒ 字段名是 `0`）。
pub static OPACITY_VALUE: FieldSpec = FieldSpec {
    name: "OpacityValue",
    address: "Opacity.value",
    component: "Opacity",
    field: "value",
    ty: &TypeSpec::Scalar(ScalarKind::F32),
    write: |scene, id, value| {
        let value = value
            .try_scalar()
            .ok_or_else(|| TargetError::mismatch("OpacityValue", "scalar", value))?;
        with_component::<Opacity, _>(scene, id, |o| o.0 = value);
        Ok(())
    },
    read: |scene, id| read_component::<Opacity, _>(scene, id, |o| AnimValue::Scalar(o.0)),
};

// ─── 构造器表：`name/address → fn(ObjectId) -> Arc<dyn Target>` ─────────

macro_rules! make_target {
    ($make:ident, $spec:ident) => {
        pub fn $make(id: ObjectId) -> Arc<dyn Target> {
            Arc::new(FieldTarget { id, spec: &$spec })
        }
    };
}

make_target!(make_circle_radius, CIRCLE_RADIUS);
make_target!(make_phase, PHASE);
make_target!(make_camera_eye, CAMERA_EYE);
make_target!(make_morph_t, MORPH_T);
make_target!(make_camera2d_framing, CAMERA2D_FRAMING);
make_target!(make_camera2d_center, FRAMING2D_CENTER);
make_target!(make_framing2d_scale, FRAMING2D_SCALE);
make_target!(make_reveal_range, REVEAL_RANGE);
make_target!(make_reveal_range_addr, REVEAL_RANGE_ADDR);
make_target!(make_reveal_end, REVEAL_END);
make_target!(make_transform_position, TRANSFORM_POSITION);
make_target!(make_transform_rotation, TRANSFORM_ROTATION);
make_target!(make_transform_scale, TRANSFORM_SCALE);
make_target!(make_opacity_value, OPACITY_VALUE);

/// 表项：一个字段规格 + 它的构造器。
#[derive(Clone, Copy)]
pub struct TargetEntry {
    pub spec: &'static FieldSpec,
    pub make: fn(ObjectId) -> Arc<dyn Target>,
}

/// **唯一的地址表**（`Target::name()` / 规范地址都能查）。
pub const TARGETS: &[TargetEntry] = &[
    TargetEntry {
        spec: &CIRCLE_RADIUS,
        make: make_circle_radius,
    },
    TargetEntry {
        spec: &PHASE,
        make: make_phase,
    },
    TargetEntry {
        spec: &CAMERA_EYE,
        make: make_camera_eye,
    },
    TargetEntry {
        spec: &MORPH_T,
        make: make_morph_t,
    },
    TargetEntry {
        spec: &CAMERA2D_FRAMING,
        make: make_camera2d_framing,
    },
    TargetEntry {
        spec: &FRAMING2D_CENTER,
        make: make_camera2d_center,
    },
    TargetEntry {
        spec: &FRAMING2D_SCALE,
        make: make_framing2d_scale,
    },
    TargetEntry {
        spec: &REVEAL_RANGE,
        make: make_reveal_range,
    },
    TargetEntry {
        spec: &REVEAL_RANGE_ADDR,
        make: make_reveal_range_addr,
    },
    TargetEntry {
        spec: &REVEAL_END,
        make: make_reveal_end,
    },
    TargetEntry {
        spec: &TRANSFORM_POSITION,
        make: make_transform_position,
    },
    TargetEntry {
        spec: &TRANSFORM_ROTATION,
        make: make_transform_rotation,
    },
    TargetEntry {
        spec: &TRANSFORM_SCALE,
        make: make_transform_scale,
    },
    TargetEntry {
        spec: &OPACITY_VALUE,
        make: make_opacity_value,
    },
];

/// 按**持久化名或规范地址**查表。
pub fn target_entry(name: &str) -> Option<&'static TargetEntry> {
    TARGETS
        .iter()
        .find(|entry| entry.spec.name == name || entry.spec.address == name)
}

/// 反序列化 / 脚本共用：名字 → `(构造器, 持久化名)`。
pub fn target_ctor_by_name(name: &str) -> Option<(fn(ObjectId) -> Arc<dyn Target>, &'static str)> {
    target_entry(name).map(|entry| (entry.make, entry.spec.name))
}

/// `Transform` 的写入通道（B7：只有三条显式通道）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformChannel {
    Position,
    Rotation,
    Scale,
}

impl TransformChannel {
    /// 通道 → 构造器（与地址表同一批函数）。
    pub fn make(self) -> fn(ObjectId) -> Arc<dyn Target> {
        match self {
            Self::Position => make_transform_position,
            Self::Rotation => make_transform_rotation,
            Self::Scale => make_transform_scale,
        }
    }
}

/// 旧的**具体目标类型**：保留公开构造形态，实现全部委托给 [`FieldSpec`]。
///
/// 这样既有手写调用（`CircleRadius { id }` / `SceneEncoder::animate::<OpacityValue>`）
/// 与测试一行不用改，而"写什么字段、类型怎么检查"只有 `FieldSpec` 一份。
macro_rules! delegating_target {
    ($ty:ident, $spec:ident) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $ty {
            pub id: ObjectId,
        }
        impl Target for $ty {
            fn apply(&self, scene: &mut dyn SceneWrite, v: &AnimValue) -> Result<(), TargetError> {
                ($spec.write)(scene, self.id, v)
            }
            fn name(&self) -> &'static str {
                $spec.name
            }
            fn target_id(&self) -> ObjectId {
                self.id
            }
        }
        impl TargetCtor for $ty {
            fn make(id: ObjectId) -> Arc<dyn Target> {
                Arc::new(Self { id })
            }
        }
    };
}

delegating_target!(CircleRadius, CIRCLE_RADIUS);
delegating_target!(Phase, PHASE);
delegating_target!(CameraEye, CAMERA_EYE);
delegating_target!(MorphT, MORPH_T);
delegating_target!(Camera2DFraming, CAMERA2D_FRAMING);
delegating_target!(RevealRange, REVEAL_RANGE);
delegating_target!(OpacityValue, OPACITY_VALUE);

/// `Transform` 的通道值（旧构造形态保留；写入委托给对应 `FieldSpec`）。
#[derive(Debug, Clone, Copy)]
pub struct TransformChannelValue {
    pub id: ObjectId,
    pub channel: TransformChannel,
}

impl Target for TransformChannelValue {
    fn apply(&self, scene: &mut dyn SceneWrite, v: &AnimValue) -> Result<(), TargetError> {
        let spec = match self.channel {
            TransformChannel::Position => &TRANSFORM_POSITION,
            TransformChannel::Rotation => &TRANSFORM_ROTATION,
            TransformChannel::Scale => &TRANSFORM_SCALE,
        };
        (spec.write)(scene, self.id, v)
    }

    fn name(&self) -> &'static str {
        match self.channel {
            TransformChannel::Position => TRANSFORM_POSITION.name,
            TransformChannel::Rotation => TRANSFORM_ROTATION.name,
            TransformChannel::Scale => TRANSFORM_SCALE.name,
        }
    }

    fn target_id(&self) -> ObjectId {
        self.id
    }
}
