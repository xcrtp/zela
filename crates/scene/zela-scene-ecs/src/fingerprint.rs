// fingerprint.rs — **结构指纹**（执行计划阶段 1）：把「状态本身」做成回归判据。
//
// # 为什么要有它
//
// 像素序列哈希保护的是**投影**（GPU / 字体 / 编码器钉死在本机），而第一性承诺是
// `state = f(t)`。本模块只吃**纯数值状态**：
//
// - 组件字段层：`TARGETS`（唯一地址表）逐对象读回，值按精确位入摘要；
// - `WorldTransforms` 层：父子累乘后的世界矩阵（同样是纯数值）。
//
// 两层的输入都是 f32/f64 数值 ⇒ 这是本项目第一个**可能跨设备**的回归判据。
//
// # 硬约束（计划 §五）
//
// 1. **不混入派生几何**：字形轮廓 / `Path` 展平是 payload 的一部分，脱离本机不可复现。
//    [`AnimValue::Path`] 只写一个占位标记，**不读内容**（`derived_excluded` 计数可见）。
// 2. **摘要确定**：对象按 `ObjectId` 升序；每对象内组件名按字典序；
//    遍历 `TARGETS` 用其声明顺序（`'static` 表，与运行环境无关）。
//    全程无 `HashMap` 迭代序参与。
// 3. **每帧独立**：不链式 —— 哈希只覆盖本帧状态，差异能直接定位到帧号。
//    （整段序列的汇总用的是"逐帧摘要再摘要"，见 [`sequence_digest`]，仍可逆推帧。）
//
// # 与 `WorldSnapshot` 的分工
//
// `WorldSnapshot`（`zela_schedule::telemetry`）是 UI 专用：Debug 文本 + 200 字符截断 +
// 未注册类型静默降级。**本模块不读它**，也不允许它接进指纹线。

use ze_scene_contract::ObjectId;
use ze_scene_core::animation::target::TARGETS;
use ze_scene_core::scene_write::SceneRead;
use ze_scene_core::WorldTransforms;
use ze_time::AnimValue;

// `known_component_table!` 展开时用**裸类型名**（与 `contract.rs` 同一张表），
// 因此这里必须把表里出现的类型全部引入作用域。
use std::sync::Arc;

use ze_scene_contract::{Group, LifetimeTracked, Name, Parent, Visibility};
use ze_scene_core::{
    Arrow, Axes2D, AxesLine, Camera2D, Camera3D, Circle, FpsView, Framing2D, FunctionGraph, Line,
    Path, Projection, Rect, Surface, SurfaceRenderer, SurfaceTexture, SurfaceTextureRef, Transform,
    ZOrder,
};
use ze_render_ir::{
    BackgroundStroke, CurveWindow, Dash, FillSolid, Gradient, Opacity, SecondPaint, StrokeSolid,
};

use crate::composer::Composer;
use crate::contract::{entity_of, object_id_of};
use crate::domain::CameraMode;
use crate::scene::components::{
    ContentTextureItem, Light, Material3D, MeshGeometry, MorphShape, PhaseAnim, Reveal,
};

/// FNV-1a（与 `ze_pod_core::capability_fingerprint` 同一算法：跨 Rust 版本稳定）。
#[derive(Debug, Clone, Copy)]
struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.byte(*byte);
        }
    }

    /// 字段分隔符：`0xFF` 不是合法 UTF-8 字节，避免拼接歧义。
    fn sep(&mut self) {
        self.byte(0xFF);
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }
}

/// 一帧的结构摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureFingerprint {
    /// 本帧摘要（FNV-1a 64）。
    pub digest: u64,
    /// 参与摘要的对象数。
    pub objects: usize,
    /// 命中的字段数（`TARGETS` read 返回 `Some` 的次数）。
    pub fields: usize,
    /// 被排除的派生几何数（`AnimValue::Path`；恒 0 是当前期望，非 0 说明有人给
    /// `TARGETS` 加了 Path 读端 —— 那会破坏跨设备承诺）。
    pub derived_excluded: usize,
}

impl StructureFingerprint {
    /// 十六进制（16 位小写，报告 / 基线用）。
    pub fn hex(&self) -> String {
        format!("{:016x}", self.digest)
    }
}

/// **整段序列的摘要**：逐帧摘要再摘要（每帧仍独立、可反推帧号）。
///
/// 与像素序列哈希不同：这里没有像素、没有 GPU，只有数值状态。
pub fn sequence_digest(frames: &[StructureFingerprint]) -> u64 {
    let mut hasher = Fnv1a::new();
    hasher.u64(frames.len() as u64);
    hasher.sep();
    for frame in frames {
        hasher.u64(frame.digest);
        hasher.sep();
    }
    hasher.0
}

/// 只读适配器：`&hecs::World` → [`SceneRead`]（与 `WorldScene` 同一张登记表）。
///
/// 写适配器在 `contract::WorldScene`；读路径没有 `&mut` 需求，故单独一个薄包装，
/// 避免为了读一个字段去借可变引用。
pub struct WorldRead<'a> {
    world: &'a hecs::World,
}

impl<'a> WorldRead<'a> {
    pub fn new(world: &'a hecs::World) -> Self {
        Self { world }
    }
}

impl SceneRead for WorldRead<'_> {
    fn with_component_ref(
        &self,
        id: ObjectId,
        type_id: std::any::TypeId,
        f: &mut dyn FnMut(&dyn std::any::Any),
    ) -> bool {
        let Some(entity) = entity_of(id) else {
            return false;
        };
        macro_rules! ref_arms {
            (clone: $($c:ty),+ $(,)?; manual: $($m:ty => $ctor:expr),* $(,)?) => {{
                $(
                    if type_id == std::any::TypeId::of::<$c>() {
                        return match self.world.get::<&$c>(entity) {
                            Ok(value) => {
                                f(&*value);
                                true
                            }
                            Err(_) => false,
                        };
                    }
                )+
                $(
                    if type_id == std::any::TypeId::of::<$m>() {
                        return match self.world.get::<&$m>(entity) {
                            Ok(value) => {
                                f(&*value);
                                true
                            }
                        };
                    }
                )*
            }};
        }
        ze_scene_core::known_component_table!(ref_arms);
        false
    }
}

/// 计算**一帧**的结构指纹。
///
/// `transforms` 是每帧派生的 `WorldTransforms` Resource；`None` 时摘要里写入
/// 「该层缺席」标记（而不是当作全零矩阵 —— 缺席与恒等必须可区分）。
pub fn fingerprint_world(
    world: &hecs::World,
    transforms: Option<&WorldTransforms>,
) -> StructureFingerprint {
    let scene = WorldRead::new(world);
    let mut hasher = Fnv1a::new();
    let mut fields = 0usize;
    let mut derived_excluded = 0usize;

    // 对象按 `ObjectId` 升序（`hecs::World::iter` 的顺序不属于契约）。
    let mut ids: Vec<ObjectId> = world
        .iter()
        .map(|entity_ref| object_id_of(entity_ref.entity()))
        .collect();
    ids.sort();

    hasher.u64(ids.len() as u64);
    hasher.sep();

    for id in &ids {
        // 身份本身进摘要：对象的增删 / 复用 index 都能被看见。
        hasher.u64(id.index as u64);
        hasher.u64(id.generation as u64);
        hasher.sep();

        // 字段层：`TARGETS` 的声明顺序（唯一地址表，`'static` ⇒ 与环境无关）。
        // 顺带收集命中的**组件结构**（去重后按字典序入摘要）：对象上有哪些可读
        // 组件，和字段值一样属于状态，增删必须可见。
        let mut components: Vec<&'static str> = Vec::new();
        for entry in TARGETS {
            let Some(value) = (entry.spec.read)(&scene, *id) else {
                continue;
            };
            fields += 1;
            if !components.contains(&entry.spec.component) {
                components.push(entry.spec.component);
            }
            hasher.bytes(entry.spec.address.as_bytes());
            hasher.sep();
            derived_excluded += hash_anim_value(&mut hasher, &value);
        }
        components.sort_unstable();
        hasher.u64(components.len() as u64);
        for component in components {
            hasher.bytes(component.as_bytes());
            hasher.sep();
        }
        hasher.sep();
    }

    // `WorldTransforms` 层：父子累乘后的世界矩阵（纯数值，可跨设备）。
    match transforms {
        None => {
            hasher.byte(0);
            hasher.sep();
        }
        Some(transforms) => {
            hasher.byte(1);
            hasher.sep();
            hasher.u64(ids.len() as u64);
            hasher.sep();
            for id in &ids {
                match transforms.get(*id) {
                    None => {
                        hasher.byte(0);
                        hasher.sep();
                    }
                    Some(transform) => {
                        hasher.byte(1);
                        // 16 个 f32，列主序（glam `to_cols_array`）。
                        for value in transform.matrix.to_cols_array() {
                            hasher.f32(value);
                        }
                        hasher.sep();
                    }
                }
            }
        }
    }

    StructureFingerprint {
        digest: hasher.0,
        objects: ids.len(),
        fields,
        derived_excluded,
    }
}

/// 把一个 [`AnimValue`] 写进摘要；返回被排除的派生几何数（0 或 1）。
fn hash_anim_value(hasher: &mut Fnv1a, value: &AnimValue) -> usize {
    match value {
        AnimValue::Scalar(v) => {
            hasher.byte(b's');
            hasher.f32(*v);
            0
        }
        AnimValue::Vec2(v) => {
            hasher.byte(b'2');
            hasher.f32(v.x);
            hasher.f32(v.y);
            0
        }
        AnimValue::Vec3(v) => {
            hasher.byte(b'3');
            hasher.f32(v.x);
            hasher.f32(v.y);
            hasher.f32(v.z);
            0
        }
        AnimValue::Color(c) => {
            hasher.byte(b'c');
            for channel in c {
                hasher.f32(*channel);
            }
            0
        }
        AnimValue::Path(_) => {
            // 硬约束 1：派生几何不进跨设备层。只写标记，**不读内容**。
            hasher.byte(b'p');
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_scene_core::Circle;

    fn circle_world(radius: f32) -> hecs::World {
        let mut world = hecs::World::new();
        world.spawn((Circle::new((1.0, 2.0), radius),));
        world
    }

    #[test]
    fn same_state_same_digest() {
        let a = fingerprint_world(&circle_world(3.0), None);
        let b = fingerprint_world(&circle_world(3.0), None);
        assert_eq!(a, b, "同一状态必须产出同一摘要");
        assert_eq!(a.objects, 1);
        assert!(a.fields > 0, "Circle.radius / center 应命中");
        assert_eq!(a.derived_excluded, 0, "当前 TARGETS 没有 Path 读端");
    }

    #[test]
    fn field_change_changes_digest() {
        let a = fingerprint_world(&circle_world(3.0), None);
        let b = fingerprint_world(&circle_world(3.5), None);
        assert_ne!(a.digest, b.digest, "字段变了摘要必须变");
    }

    #[test]
    fn negative_zero_differs_from_zero() {
        // 精确位口径：`-0.0` 与 `0.0` 不是同一个状态（NaN 也一样按位比）。
        let a = fingerprint_world(&circle_world(0.0), None);
        let b = fingerprint_world(&circle_world(-0.0), None);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn transform_layer_is_part_of_digest() {
        let world = circle_world(1.0);
        let id = ObjectId::new(0, 1);
        let identity = WorldTransforms::from_map([(id, Default::default())]);
        let moved = WorldTransforms::from_map([(
            id,
            ze_notion::Transform3D::translate(ze_notion::Vec3::new(5.0, 0.0, 0.0)),
        )]);
        let a = fingerprint_world(&world, Some(&identity));
        let b = fingerprint_world(&world, Some(&moved));
        assert_ne!(a.digest, b.digest, "世界矩阵变了摘要必须变");
        let absent = fingerprint_world(&world, None);
        assert_ne!(absent.digest, a.digest, "层缺席与恒等必须可区分");
    }

    #[test]
    fn sequence_digest_is_order_and_length_sensitive() {
        let a = fingerprint_world(&circle_world(1.0), None);
        let b = fingerprint_world(&circle_world(2.0), None);
        assert_ne!(sequence_digest(&[a, b]), sequence_digest(&[b, a]));
        assert_ne!(sequence_digest(&[a]), sequence_digest(&[a, a]));
        assert_eq!(sequence_digest(&[a, b]), sequence_digest(&[a, b]));
    }
}
