//! **单态化路径对拍**：typed（按类型分槽 + 函数指针）必须与动态路径**逐位相同**。
//!
//! 三个求值器同源：`TypedProgram`（本批）/ `CompiledExpr`（lowered）/ 参考求值器。
//! 这里把前两者放在一起对拍，并逐算子覆盖（含 F64 / Vec2 / Vec3 / Bool 重载）。

use ze_comb::{
    ChannelRef, CompiledExpr, EvalError, Expr, Literal, Mono, OpRef, TypedProgram, Value,
    eval_reference, SlotType,
};

fn math(name: &str) -> OpRef {
    OpRef::new("zela.math", name)
}

/// 无状态宿主：`time` 固定、其余通道显式缺失。
struct MathOnly;

impl Mono for MathOnly {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        match channel.name.as_str() {
            "time" => Ok(Value::f64(1.25)),
            "frame" => Ok(Value::f64(30.0)),
            _ => Err(EvalError::MissingChannel {
                channel: channel.clone(),
            }),
        }
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        ze_func::eval(op, args)
    }
}

/// 表达式在三条路径上的值（typed / lowered / reference）。
fn evaluate(expr: &Expr) -> (Value, Value, Value) {
    let typed = TypedProgram::compile(expr, ze_func::monomorphizer())
        .expect("本用例的表达式必须可单态化");
    let lowered = CompiledExpr::compile(expr).expect("可编译");
    let mono = MathOnly;
    (
        typed.eval(&mono).expect("typed 可求值"),
        lowered.eval(&mono).expect("lowered 可求值"),
        eval_reference(expr, &mono).expect("reference 可求值"),
    )
}

fn assert_same(expr: &Expr, label: &str) {
    let (typed, lowered, reference) = evaluate(expr);
    assert_eq!(typed, lowered, "`{label}`：typed 与 lowered 必须逐位相同");
    assert_eq!(typed, reference, "`{label}`：typed 与 reference 必须逐位相同");
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::call(math(name), args)
}

fn pure(value: impl Into<Literal>) -> Expr {
    Expr::pure(value)
}

fn v2(x: f32, y: f32) -> Expr {
    pure([x, y])
}

fn v3(x: f32, y: f32, z: f32) -> Expr {
    pure([x, y, z])
}

#[test]
fn every_operator_matches_across_evaluators() {
    let cases: Vec<(&str, Expr)> = vec![
        ("sin", call("sin", vec![pure(0.5)])),
        ("cos", call("cos", vec![pure(0.5)])),
        ("add/f64", call("add", vec![pure(1.5), pure(2.25)])),
        ("add/vec2", call("add", vec![v2(1.0, 2.0), v2(3.0, 4.0)])),
        ("add/vec3", call("add", vec![v3(1.0, 2.0, 3.0), v3(4.0, 5.0, 6.0)])),
        ("mul/f64", call("mul", vec![pure(1.5), pure(3.0)])),
        ("mul/vec2", call("mul", vec![v2(1.0, 2.0), pure(2.0)])),
        ("mul/vec2_left", call("mul", vec![pure(2.0), v2(1.0, 2.0)])),
        ("scale/vec3", call("scale", vec![v3(1.0, 2.0, 3.0), pure(0.5)])),
        ("scale/vec3_left", call("scale", vec![pure(0.5), v3(1.0, 2.0, 3.0)])),
        ("sum/empty", call("sum", vec![])),
        ("sum/f64", call("sum", vec![pure(0.5), pure(0.25), pure(0.125)])),
        ("sum/vec2", call("sum", vec![v2(1.0, 2.0), v2(3.0, 4.0)])),
        ("mix/f64", call("mix", vec![pure(0.0), pure(10.0), pure(0.25)])),
        ("mix/vec2", call("mix", vec![v2(0.0, -2.0), v2(4.0, 2.0), pure(0.5)])),
        (
            "mix/vec3",
            call("mix", vec![v3(0.0, 0.0, 0.0), v3(2.0, 4.0, 6.0), pure(0.5)]),
        ),
        ("clamp", call("clamp", vec![pure(2.0), pure(0.0), pure(1.0)])),
        ("vec2", call("vec2", vec![pure(1.0), pure(2.0)])),
        ("vec3", call("vec3", vec![pure(1.0), pure(2.0), pure(3.0)])),
        ("x/vec2", call("x", vec![v2(1.0, 2.0)])),
        ("y/vec3", call("y", vec![v3(1.0, 2.0, 3.0)])),
        ("z/vec3", call("z", vec![v3(1.0, 2.0, 3.0)])),
        (
            "rotate3",
            call(
                "rotate3",
                vec![v3(0.0, 0.0, 5.0), v3(0.0, 1.0, 0.0), pure(std::f64::consts::FRAC_PI_2)],
            ),
        ),
        ("splat", call("splat", vec![pure(2.0)])),
        (
            "vec3_from_vec2",
            call("vec3_from_vec2", vec![v2(1.0, 2.0), pure(3.0)]),
        ),
        ("vec2_from_vec3", call("vec2_from_vec3", vec![v3(4.0, 5.0, 6.0)])),
        ("length/vec2", call("length", vec![v2(3.0, 4.0)])),
        ("length/vec3", call("length", vec![v3(3.0, 4.0, 0.0)])),
        ("normalize/vec2", call("normalize", vec![v2(0.0, 3.0)])),
        ("normalize/vec3", call("normalize", vec![v3(0.0, 0.0, 0.0)])),
        ("round_f32", call("round_f32", vec![pure(0.1 + 0.2)])),
        ("distance/vec2", call("distance", vec![v2(0.0, 0.0), v2(3.0, 4.0)])),
        (
            "distance/vec3",
            call("distance", vec![v3(0.0, 0.0, 0.0), v3(3.0, 4.0, 0.0)]),
        ),
        (
            "smoothstep",
            call("smoothstep", vec![pure(0.0), pure(1.0), pure(0.25)]),
        ),
        ("fract", call("fract", vec![pure(-0.25)])),
        ("lt", call("lt", vec![pure(1.0), pure(2.0)])),
        ("gt", call("gt", vec![pure(1.0), pure(2.0)])),
        ("le", call("le", vec![pure(2.0), pure(2.0)])),
        ("ge", call("ge", vec![pure(1.0), pure(2.0)])),
        ("eq/f64", call("eq", vec![pure(2.0), pure(2.0)])),
        ("ne/f64", call("ne", vec![pure(1.0), pure(2.0)])),
        ("eq/bool", call("eq", vec![pure(true), pure(true)])),
        ("ne/bool", call("ne", vec![pure(true), pure(false)])),
        ("and", call("and", vec![pure(true), pure(false)])),
        ("or", call("or", vec![pure(true), pure(false)])),
        ("not", call("not", vec![pure(true)])),
    ];
    for (label, expr) in cases {
        assert_same(&expr, label);
    }
}

/// 整棵表达式：通道 + 多次调用 + 共享节点。
#[test]
fn whole_expression_matches_with_channels_and_shared_nodes() {
    let shared = call(
        "sin",
        vec![call(
            "mul",
            vec![Expr::channel(ChannelRef::builtin("time")), pure(2.0)],
        )],
    )
    .with_id("h1");
    let expr = call(
        "add",
        vec![
            shared.clone(),
            call("scale", vec![pure(2.0), Expr::reference("h1")]),
        ],
    );
    assert_same(&expr, "shared/channels");

    let mixed = call(
        "length",
        vec![call(
            "vec2",
            vec![
                call("x", vec![v2(3.0, 4.0)]),
                call("sin", vec![Expr::channel(ChannelRef::builtin("time"))]),
            ],
        )],
    );
    assert_same(&mixed, "vec2/length");
}

/// 不支持的节点 ⇒ `compile` 返回 `None`（调用方回落动态路径，不半做）。
#[test]
fn unsupported_nodes_fall_back() {
    let select = Expr::select(pure(true), pure(1.0), pure(0.0));
    assert!(TypedProgram::compile(&select, ze_func::monomorphizer()).is_none());

    let nat_rec = Expr::nat_rec(pure(2_i64), pure(1.0), Expr::rec_prev());
    assert!(TypedProgram::compile(&nat_rec, ze_func::monomorphizer()).is_none());

    let read = Expr::read(ChannelRef::new("zela.scene", "Circle.radius"), 7);
    assert!(TypedProgram::compile(&read, ze_func::monomorphizer()).is_none());
}

/// 类型未知的通道 ⇒ 回落（typed 不做运行期猜类型）。
#[test]
fn unknown_channel_type_falls_back() {
    let channel = Expr::channel(ChannelRef::new("zela.scene", "Circle.radius"));
    assert!(TypedProgram::compile(&channel, ze_func::monomorphizer()).is_none());
}

/// 槽类型是编译期事实：根类型直接读出来。
#[test]
fn root_type_is_static() {
    let program = TypedProgram::compile(&v2(1.0, 2.0), ze_func::monomorphizer()).unwrap();
    assert_eq!(program.root_type(), SlotType::Vec2);

    let program = TypedProgram::compile(&pure(true), ze_func::monomorphizer()).unwrap();
    assert_eq!(program.root_type(), SlotType::Bool);
}
