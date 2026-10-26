//! ze-pod-func: **函子库 Pod**（namespace `zela.math`）。
//!
//! # 层次
//!
//! ```text
//! 脚本 ──只提交结构──► Expr（ze-func）──► 产物（expression JSON）
//!                          ▲
//!               expr.sin / expr.mul …（本 Pod 的 capability）
//! ```
//!
//! 本 crate 是 `crates/ze/pod` 的成员：**不依赖任何 `zela-*`**
//! （`cargo verify --step structure` 的 `JQ_POD_LAYER` 判据守住）。
//!
//! # 为什么独立 namespace
//!
//! 指纹按 namespace 隔离。函子库加一百个算子，`zela.scene@10` 一个字节不动；
//! 声明文件也可以由「已注册 pod 清单」精确重建（§7.4）。
//!
//! # 本批边界
//!
//! - 只做**表达**：不采样、不求值、不读时间（通道是零元叶子，值由宿主注入）。
//! - 导出按树展开：`Ref` 折叠与帧内 memo 属 S3。

pub mod api;
pub mod expr_pod;
pub mod ops;

pub use api::{
    AddSpec, BindSpec, ChannelSpec, CosSpec, MathApi, MulSpec, ScaleSpec, SinSpec, SumSpec,
};
pub use expr_pod::{
    ExprPod, LIBRARY_NAMESPACE, LIBRARY_VERSION, PendingBinding, SCENE_NAMESPACE,
};
pub use ops::eval as eval_op;

/// 本函子库在 Broker 里的 Service 名（脚本侧全局表名也是它：`expr.*`）。
///
/// 与 namespace 分开：Service 是**装配协议**的名字，namespace 是**能力身份**。
pub const SERVICE: &str = "expr";
