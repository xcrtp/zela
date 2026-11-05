//! params.rs — 参数注册表 side table（CA-b）与参数地址回写（CA-c）。
//!
//! # 为什么在这里
//!
//! 参数的**值**来自绑定表（曲线字段 / `State` 仿射系数），而绑定表属于场景核心层
//! ⇒ 只有这一层能把「参数从哪来」翻译成 [`ze_property::Property`]。注册表本身是
//! kernel 级类型（`ze_property::ParameterRegistry`），这里只负责**装配**它。
//!
//! # 地址
//!
//! 每个参数有一个稳定字符串地址，形如：
//!
//! ```text
//! curve:0003:12.1:5:amp          // 第 3 条绑定的曲线 amp 字段
//! state:0007:21.1:5:0:stiff      // 第 7 条绑定的第 0 个 State 的 stiff
//! ```
//!
//! 冒号分段：`类别 : 装配期绑定序号 : ObjectId(index.generation) : ClockId : …`。
//! 绑定序号只用于**保证地址唯一**；回写按 `(ObjectId, ClockId)` 语义匹配，
//! 因此相机让位导致表内先后变化时地址仍指向同一个参数。
//!
//! # WidgetId 分配（CA-c 阶段 1）
//!
//! `WidgetId` 是 `SceneEvent` 的公共契约，本批**不替换**它。注册表按**地址升序**
//! 分配 `WidgetId(PARAM_WIDGET_BASE + index)`：同一参数地址每帧同一 id，场景侧
//! 既有 13 个手写 id 原样保留（渐进式）。
//!
//! # R6：改到发散必须是显式错误
//!
//! `State` 系数写回走 [`StatePlan::set_linear`]，它复用 BZ-a 的谱半径门禁：
//! `|λ|max ≥ 1` ⇒ `AffineError::Unstable`，原图不动（绝不静默拿到 inf/NaN）。

use std::collections::BTreeMap;

use ze_property::{ParameterRegistry, Property, ScalarConfig};
use ze_scene_contract::ObjectId;
use ze_time::{ClockId, Curve};

use crate::animation::affine::{AffineError, StatePlan};
use crate::animation::binding::{BindingDriver, BindingTable};
use crate::animation::scope::ScopeId;
use crate::domain::WidgetId;

/// 动态参数 `WidgetId` 的起始值（既有手写 id 是 1–16）。
pub const PARAM_WIDGET_BASE: u32 = 100;

/// 地址升序的第 `index` 个参数对应的稳定 `WidgetId`。
pub fn widget_id(index: usize) -> WidgetId {
    WidgetId(PARAM_WIDGET_BASE + index as u32)
}

/// `WidgetId` → 参数在注册表里的升序下标（非动态参数返回 `None`）。
pub fn widget_index(id: WidgetId) -> Option<usize> {
    // `then_some` 会**先求值**参数 ⇒ 旧 id（1–16）相减会下溢，必须用 `then(|| …)`。
    (id.0 >= PARAM_WIDGET_BASE).then(|| (id.0 - PARAM_WIDGET_BASE) as usize)
}

/// 参数回写失败：地址不合法 / 绑定不在位 / 曲线种类不符 / 稳定性门禁拒绝。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParamError {
    /// 地址不是本模块生成的格式。
    #[error("参数地址无法解析：`{0}`")]
    Address(String),
    /// 地址指向的绑定当前不在表内（例如相机绑定被让位）。
    #[error("参数不属于任何在位绑定：`{0}`")]
    Unbound(String),
    /// 绑定的驱动种类 / 曲线变体与参数不匹配。
    #[error("参数与绑定种类不匹配：{0}")]
    Kind(String),
    /// `State` 系数写入会让仿射图发散（复用 BZ-a 门禁）。
    #[error(transparent)]
    Affine(#[from] AffineError),
}

/// 曲线字段（`Sin` / `Phase` / `Orbit` 的参数名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurveField {
    Base,
    Amp,
    Omega,
    Radius,
    Speed,
    Angle,
}

impl CurveField {
    fn as_str(self) -> &'static str {
        match self {
            CurveField::Base => "base",
            CurveField::Amp => "amp",
            CurveField::Omega => "omega",
            CurveField::Radius => "radius",
            CurveField::Speed => "speed",
            CurveField::Angle => "angle",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "base" => CurveField::Base,
            "amp" => CurveField::Amp,
            "omega" => CurveField::Omega,
            "radius" => CurveField::Radius,
            "speed" => CurveField::Speed,
            "angle" => CurveField::Angle,
            _ => return None,
        })
    }

    /// 当前曲线上的值（种类不符 ⇒ `None`）。
    fn read(self, curve: &Curve) -> Option<f64> {
        match (self, curve) {
            (CurveField::Base, Curve::Sin { base, .. }) => Some(f64::from(*base)),
            (CurveField::Amp, Curve::Sin { amp, .. }) => Some(f64::from(*amp)),
            (CurveField::Omega, Curve::Sin { omega, .. }) => Some(f64::from(*omega)),
            (CurveField::Omega, Curve::Phase { omega }) => Some(f64::from(*omega)),
            (CurveField::Radius, Curve::Orbit { radius, .. }) => Some(f64::from(*radius)),
            (CurveField::Speed, Curve::Orbit { speed, .. }) => Some(f64::from(*speed)),
            (CurveField::Angle, Curve::Orbit { initial_angle, .. }) => {
                Some(f64::from(*initial_angle))
            }
            _ => None,
        }
    }

    /// 写回当前曲线（种类不符 ⇒ 错误，不静默忽略）。
    fn write(self, curve: &mut Curve, value: f32) -> Result<(), ParamError> {
        match (self, curve) {
            (CurveField::Base, Curve::Sin { base, .. }) => *base = value,
            (CurveField::Amp, Curve::Sin { amp, .. }) => *amp = value,
            (CurveField::Omega, Curve::Sin { omega, .. }) => *omega = value,
            (CurveField::Omega, Curve::Phase { omega }) => *omega = value,
            (CurveField::Radius, Curve::Orbit { radius, .. }) => *radius = value,
            (CurveField::Speed, Curve::Orbit { speed, .. }) => *speed = value,
            (CurveField::Angle, Curve::Orbit { initial_angle, .. }) => *initial_angle = value,
            _ => {
                return Err(ParamError::Kind(format!(
                    "字段 `{}` 不属于当前曲线",
                    self.as_str()
                )));
            }
        }
        Ok(())
    }
}

/// `State` 的哪个系数：语义化的弹簧两参数，或通用矩阵单元。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateField {
    /// `x' = x + v; v' = damp·v − stiff·x` 的 `stiff`（= `-A[1][0]`）。
    Stiff,
    /// 同一形态的 `damp`（= `A[1][1]`）。
    Damp,
    /// 通用线性系数 `A[row][col]`。
    Cell(usize, usize),
}

impl StateField {
    fn label(self) -> String {
        match self {
            StateField::Stiff => "stiff".into(),
            StateField::Damp => "damp".into(),
            StateField::Cell(row, col) => format!("a[{row}][{col}]"),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "stiff" => Some(StateField::Stiff),
            "damp" => Some(StateField::Damp),
            other => {
                let rest = other.strip_prefix('a')?;
                let (row, col) = rest.split_once('_')?;
                Some(StateField::Cell(row.parse().ok()?, col.parse().ok()?))
            }
        }
    }

    fn address_suffix(self) -> String {
        match self {
            StateField::Stiff => "stiff".into(),
            StateField::Damp => "damp".into(),
            StateField::Cell(row, col) => format!("a{row}_{col}"),
        }
    }

    /// 当前值（越界 / 形态不符 ⇒ `None`）。
    fn read(self, plan: &StatePlan) -> Option<f64> {
        match self {
            StateField::Stiff => plan.linear(1, 0).map(|value| -value),
            StateField::Damp => plan.linear(1, 1),
            StateField::Cell(row, col) => plan.linear(row, col),
        }
    }

    /// 写回（`stiff` 取负号；稳定性门禁由 [`StatePlan::set_linear`] 负责）。
    fn write(self, plan: &mut StatePlan, value: f64) -> Result<(), AffineError> {
        match self {
            StateField::Stiff => plan.set_linear(1, 0, -value),
            StateField::Damp => plan.set_linear(1, 1, value),
            StateField::Cell(row, col) => plan.set_linear(row, col, value),
        }
    }
}

/// 解析后的回写目标。
#[derive(Debug, Clone, Copy, PartialEq)]
enum ParamWrite {
    Curve {
        target: ObjectId,
        clock: ClockId,
        field: CurveField,
    },
    State {
        target: ObjectId,
        clock: ClockId,
        plan: usize,
        field: StateField,
    },
}

fn parse_object(text: &str) -> Option<ObjectId> {
    let (index, generation) = text.split_once('.')?;
    Some(ObjectId {
        index: index.parse().ok()?,
        generation: generation.parse().ok()?,
    })
}

fn object_key(id: ObjectId) -> String {
    format!("{}.{}", id.index, id.generation)
}

fn parse_address(address: &str) -> Option<ParamWrite> {
    // **CC-a**：地址最前面是作用域序号段（`0:curve:…` / `2:state:…`）。
    // 回写按 `(ObjectId, ClockId)` 语义匹配 ⇒ 作用域段只用于区分地址，不参与匹配。
    let address = match address.split_once(':') {
        Some((head, rest)) if !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()) => rest,
        _ => address,
    };
    let mut parts = address.split(':');
    match parts.next()? {
        "curve" => {
            let _binding = parts.next()?;
            let target = parse_object(parts.next()?)?;
            let clock = ClockId(parts.next()?.parse().ok()?);
            let field = CurveField::parse(parts.next()?)?;
            Some(ParamWrite::Curve {
                target,
                clock,
                field,
            })
        }
        "state" => {
            let _binding = parts.next()?;
            let target = parse_object(parts.next()?)?;
            let clock = ClockId(parts.next()?.parse().ok()?);
            let plan = parts.next()?.parse().ok()?;
            let field = StateField::parse(parts.next()?)?;
            Some(ParamWrite::State {
                target,
                clock,
                plan,
                field,
            })
        }
        _ => None,
    }
}

/// 一条待登记的原始参数（label 在按地址排序后统一生成，保证重名后缀稳定）。
struct RawParam {
    address: String,
    base_label: String,
    value: f64,
    min: f32,
    max: f32,
    step: f32,
}

/// **装配期**：把绑定表里的可调参数枚举成注册表（升序 side table）。
pub fn build_registry(bindings: &BindingTable) -> ParameterRegistry {
    let mut raw: Vec<RawParam> = Vec::new();
    for (index, binding) in bindings.bindings.iter().enumerate() {
        let target = binding.target.target_id();
        // 悬垂目标（没有对应对象）不产生参数：滑块拖动也不会有任何效果。
        if target == ObjectId::DANGLING {
            continue;
        }
        let name = binding.target.name();
        let scope = binding.scope;
        let scope_path = bindings.scope_path(scope);
        if let BindingDriver::Curve(curve) = &binding.driver {
            collect_curve(
                index,
                target,
                binding.clock,
                scope,
                scope_path,
                name,
                curve,
                &mut raw,
            );
        }
        for (plan_index, plan) in binding.state_plans.iter().enumerate() {
            collect_state(
                index,
                target,
                binding.clock,
                scope,
                scope_path,
                name,
                plan_index,
                plan,
                &mut raw,
            );
        }
    }

    raw.sort_by(|a, b| a.address.cmp(&b.address));

    // 同一目标 / 同一参数名会出现多次（阵列里 8 个实例）⇒ 后缀编号稳定。
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut registry = ParameterRegistry::new();
    for param in raw {
        let count = seen.entry(param.base_label.clone()).or_insert(0);
        *count += 1;
        let label = if *count == 1 {
            param.base_label
        } else {
            format!("{} #{}", param.base_label, count)
        };
        let property = Property::Scalar(ScalarConfig {
            label,
            value: param.value as f32,
            min: param.min,
            max: param.max,
            step: Some(param.step),
            unit: None,
        });
        // 地址由 (绑定序号, 目标, 时钟, 字段) 组成 ⇒ 不会重复；真重复也不能静默覆盖。
        let _ = registry.insert(param.address, property);
    }
    registry
}

#[allow(clippy::too_many_arguments)]
fn collect_curve(
    binding: usize,
    target: ObjectId,
    clock: ClockId,
    scope: ScopeId,
    scope_path: &str,
    name: &str,
    curve: &Curve,
    out: &mut Vec<RawParam>,
) {
    let fields: &[(CurveField, f32, f32, f32)] = match curve {
        Curve::Sin { .. } => &[
            (CurveField::Base, 0.0, 3.0, 0.1),
            (CurveField::Amp, 0.0, 2.0, 0.05),
            (CurveField::Omega, 0.0, 8.0, 0.1),
        ],
        Curve::Phase { .. } => &[(CurveField::Omega, 0.0, 5.0, 0.1)],
        Curve::Orbit { .. } => &[
            (CurveField::Radius, 1.0, 20.0, 0.5),
            (CurveField::Speed, -2.0, 2.0, 0.1),
            (CurveField::Angle, -3.14, 3.14, 0.1),
        ],
        _ => &[],
    };
    for &(field, min, max, step) in fields {
        let Some(value) = field.read(curve) else {
            continue;
        };
        out.push(RawParam {
            address: format!(
                "{}:curve:{binding:04}:{}:{}:{}",
                scope.0,
                object_key(target),
                clock.0,
                field.as_str()
            ),
            base_label: scoped_label(scope_path, name, field.as_str()),
            value,
            min,
            max,
            step,
        });
    }
}

/// 参数显示名：非根作用域加路径前缀（`panel_a/Circle.radius · amp`）。
fn scoped_label(scope_path: &str, name: &str, field: &str) -> String {
    if scope_path.is_empty() {
        format!("{name} · {field}")
    } else {
        format!("{scope_path}/{name} · {field}")
    }
}

/// 半隐式欧拉弹簧的识别形态：`A[0] == [1, 1]`（`x' = x + v`）。
fn is_spring_form(plan: &StatePlan) -> bool {
    plan.width() == 2
        && plan.linear(0, 0).is_some_and(|v| (v - 1.0).abs() < 1e-12)
        && plan.linear(0, 1).is_some_and(|v| (v - 1.0).abs() < 1e-12)
}

#[allow(clippy::too_many_arguments)]
fn collect_state(
    binding: usize,
    target: ObjectId,
    clock: ClockId,
    scope: ScopeId,
    scope_path: &str,
    name: &str,
    plan_index: usize,
    plan: &StatePlan,
    out: &mut Vec<RawParam>,
) {
    let fields: Vec<(StateField, f32, f32)> = if is_spring_form(plan) {
        vec![(StateField::Stiff, 0.0, 0.5), (StateField::Damp, 0.0, 1.0)]
    } else {
        (0..plan.width())
            .flat_map(|row| {
                (0..plan.width()).map(move |col| (StateField::Cell(row, col), -2.0, 2.0))
            })
            .collect()
    };
    for (field, min, max) in fields {
        let Some(value) = field.read(plan) else {
            continue;
        };
        let step = match field {
            StateField::Stiff => 0.005,
            StateField::Damp => 0.001,
            StateField::Cell(..) => 0.01,
        };
        out.push(RawParam {
            address: format!(
                "{}:state:{binding:04}:{}:{}:{plan_index}:{}",
                scope.0,
                object_key(target),
                clock.0,
                field.address_suffix()
            ),
            base_label: scoped_label(scope_path, name, &field.label()),
            value,
            min,
            max,
            step,
        });
    }
}

/// 把 `address` 对应的参数写成 `value`（曲线直接写字段；`State` 系数过稳定性门禁）。
///
/// 调用方传入的是**当前帧的绑定表**：地址按 `(ObjectId, ClockId)` 语义匹配，
/// 不依赖表内下标。
pub fn apply(bindings: &mut BindingTable, address: &str, value: f32) -> Result<(), ParamError> {
    let target = parse_address(address).ok_or_else(|| ParamError::Address(address.into()))?;
    match target {
        ParamWrite::Curve {
            target,
            clock,
            field,
        } => {
            let binding = bindings
                .bindings
                .iter_mut()
                .find(|binding| binding.target.target_id() == target && binding.clock == clock)
                .ok_or_else(|| ParamError::Unbound(address.into()))?;
            let curve = binding
                .driver
                .as_curve_mut()
                .ok_or_else(|| ParamError::Kind(format!("绑定 {address} 不是曲线驱动")))?;
            field.write(curve, value)
        }
        ParamWrite::State {
            target,
            clock,
            plan,
            field,
        } => {
            let binding = bindings
                .bindings
                .iter_mut()
                .find(|binding| binding.target.target_id() == target && binding.clock == clock)
                .ok_or_else(|| ParamError::Unbound(address.into()))?;
            let plan = binding.state_plans.get_mut(plan).ok_or_else(|| {
                ParamError::Kind(format!("绑定 {address} 没有第 {plan} 个 State 计划"))
            })?;
            field.write(plan, f64::from(value))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ze_comb::{Expr, OpRef};
    use ze_time::ClockId;

    use super::*;
    use crate::animation::affine::plan_from_expr;
    use crate::animation::binding::{Binding, BindingTable, ResetBehavior, SeekKind};
    use crate::animation::target::CircleRadius;

    fn object(index: u32) -> ObjectId {
        ObjectId {
            index,
            generation: 1,
        }
    }

    fn prev(axis: Option<&str>) -> Expr {
        match axis {
            Some(name) => Expr::call(OpRef::new("zela.math", name), [Expr::rec_prev()]),
            None => Expr::rec_prev(),
        }
    }

    fn add(a: Expr, b: Expr) -> Expr {
        Expr::call(OpRef::new("zela.math", "add"), [a, b])
    }

    fn scale(k: f64, of: Expr) -> Expr {
        Expr::call(OpRef::new("zela.math", "scale"), [Expr::pure(k), of])
    }

    fn vec2(x: Expr, y: Expr) -> Expr {
        Expr::call(OpRef::new("zela.math", "vec2"), [x, y])
    }

    /// `x' = x + v; v' = 0.97·v − 0.02·x`（与 spring_state.lua 同形）。
    fn spring_plan() -> StatePlan {
        let init = vec2(Expr::pure(1.0), Expr::pure(0.0));
        let step = vec2(
            add(prev(Some("x")), prev(Some("y"))),
            add(scale(0.97, prev(Some("y"))), scale(-0.02, prev(Some("x")))),
        );
        plan_from_expr(&init, &step, None).unwrap()
    }

    fn binding_with(
        target: ObjectId,
        clock: ClockId,
        driver: BindingDriver,
        plans: Vec<StatePlan>,
    ) -> Binding {
        Binding {
            clock,
            scope: ScopeId::ROOT,
            priority: 0,
            driver,
            target: Arc::new(CircleRadius { id: target }),
            predicate: None,
            active: None,
            reset: ResetBehavior::Hold,
            initial: None,
            state_plans: plans,
            seek_kind: SeekKind::O1,
        }
    }

    #[test]
    fn state_coefficients_are_enumerable_and_named() {
        let mut table = BindingTable::new();
        table.add(binding_with(
            object(7),
            ClockId(3),
            BindingDriver::Expr(
                ze_comb::CompiledExpr::compile(&Expr::pure(0.0))
                    .unwrap()
                    .into(),
            ),
            vec![spring_plan()],
        ));
        let registry = build_registry(&table);
        let addresses: Vec<&str> = registry.addresses().collect();
        assert_eq!(
            addresses,
            vec!["0:state:0000:7.1:3:0:damp", "0:state:0000:7.1:3:0:stiff"]
        );
        let stiff = registry.value("0:state:0000:7.1:3:0:stiff").unwrap();
        let damp = registry.value("0:state:0000:7.1:3:0:damp").unwrap();
        assert!(matches!(stiff, ze_property::PropertyValue::Scalar(v) if (v - 0.02).abs() < 1e-6));
        assert!(matches!(damp, ze_property::PropertyValue::Scalar(v) if (v - 0.97).abs() < 1e-6));
        let label = registry
            .get("0:state:0000:7.1:3:0:stiff")
            .unwrap()
            .label()
            .to_string();
        assert!(label.ends_with("stiff"), "label = {label}");
    }

    #[test]
    fn state_write_back_persists_and_rejects_unstable() {
        let mut table = BindingTable::new();
        table.add(binding_with(
            object(7),
            ClockId(3),
            BindingDriver::Expr(
                ze_comb::CompiledExpr::compile(&Expr::pure(0.0))
                    .unwrap()
                    .into(),
            ),
            vec![spring_plan()],
        ));
        apply(&mut table, "0:state:0000:7.1:3:0:damp", 0.5).unwrap();
        assert!((table.bindings[0].state_plans[0].linear(1, 1).unwrap() - 0.5).abs() < 1e-6);

        // damp = 1、stiff = 0.03 ⇒ det = 1.03 ⇒ 谱半径 > 1 ⇒ 显式拒绝，原值不动。
        apply(&mut table, "0:state:0000:7.1:3:0:stiff", 0.03).unwrap();
        let err = apply(&mut table, "0:state:0000:7.1:3:0:damp", 1.0).unwrap_err();
        assert!(matches!(
            err,
            ParamError::Affine(AffineError::Unstable { .. })
        ));
        assert!((table.bindings[0].state_plans[0].linear(1, 1).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn dangling_target_yields_no_parameters() {
        let mut table = BindingTable::new();
        table.add(binding_with(
            ObjectId::DANGLING,
            ClockId(1),
            BindingDriver::Curve(Curve::Sin {
                base: 0.0,
                amp: 1.0,
                omega: 1.0,
            }),
            Vec::new(),
        ));
        assert!(build_registry(&table).is_empty());
    }

    #[test]
    fn duplicate_target_labels_get_stable_suffixes() {
        let mut table = BindingTable::new();
        for index in 0..2 {
            table.add(binding_with(
                object(index),
                ClockId(index),
                BindingDriver::Curve(Curve::Sin {
                    base: 0.0,
                    amp: 1.0,
                    omega: 1.0,
                }),
                Vec::new(),
            ));
        }
        let registry = build_registry(&table);
        let labels: Vec<String> = registry.iter().map(|e| e.property.label().into()).collect();
        assert_eq!(labels.len(), 6);
        assert!(labels.iter().any(|label| label.ends_with("· amp")));
        assert!(labels.iter().any(|label| label.ends_with("· amp #2")));
    }

    #[test]
    fn widget_ids_follow_ascending_addresses() {
        let mut table = BindingTable::new();
        table.add(binding_with(
            object(2),
            ClockId(1),
            BindingDriver::Curve(Curve::Sin {
                base: 0.0,
                amp: 1.0,
                omega: 1.0,
            }),
            Vec::new(),
        ));
        let registry = build_registry(&table);
        for (index, entry) in registry.iter().enumerate() {
            assert_eq!(registry.index_of(entry.address), Some(index));
            assert_eq!(widget_index(widget_id(index)), Some(index));
        }
    }

    /// **CC-a**：两个作用域的同名参数地址不同；标签带作用域路径。
    #[test]
    fn scoped_parameters_get_distinct_addresses_and_labels() {
        let mut table = BindingTable::new();
        let a = table.declare_scope("panel_a");
        let b = table.declare_scope("panel_b");
        for (index, scope) in [(0u32, a), (1u32, b)] {
            let mut binding = binding_with(
                object(index),
                ClockId(index),
                BindingDriver::Curve(Curve::Sin {
                    base: 0.0,
                    amp: 1.0,
                    omega: 1.0,
                }),
                Vec::new(),
            );
            binding.scope = scope;
            table.add(binding);
        }
        let registry = build_registry(&table);
        let addresses: Vec<&str> = registry.addresses().collect();
        assert!(
            addresses
                .iter()
                .all(|address| address.starts_with('1') || address.starts_with('2'))
        );
        assert_eq!(addresses.len(), 6);
        let labels: Vec<String> = registry.iter().map(|e| e.property.label().into()).collect();
        assert!(labels.iter().any(|label| label.starts_with("panel_a/")));
        assert!(labels.iter().any(|label| label.starts_with("panel_b/")));
    }
}
