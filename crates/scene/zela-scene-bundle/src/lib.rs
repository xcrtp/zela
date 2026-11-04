// zela-scene-bundle — ZSB（Zela Scene Bundle）格式 crate。
//
// # 它是什么
//
// 「脚本 → 已装配场景」的**规范产物**：脚本在 Native 侧执行完，产出纯数据的 `.zsb`；
// 回放端只做「读产物 → 可还原性检查 → 还原 Scene → 调度推进 → 渲染」。
// 它不依赖任何语言适配器（mlua / pyo3），因此可以单独带进浏览器。
//
// # 两段布局
//
// ```text
// ┌──────────────────────────────┐
// │ Header（24 B）                │ 魔数 + 容器版本 + 段长
// ├──────────────────────────────┤
// │ Payload（hash 只覆盖这段）    │ SceneScript 的规范 JSON
// ├──────────────────────────────┤
// │ Meta（不进 hash）             │ cap / pod 版本 + 降级记录 + 源脚本 hash
// └──────────────────────────────┘
// ```
//
// # 两个断言必须分开跑（见执行计划 §2）
//
// - **内容 hash**：只覆盖 payload 字节 ⇒ 跨运行时比对渲染确定性；
// - **可还原性检查**：依据 Meta 的 cap / pod 版本 ⇒ 逐项确认当前运行时能解释。
//   hash 对得上**不代表**能还原；缺失项必须**完整列出**，不得静默降级。
//
// # 字节确定性
//
// payload 是规范 JSON：`serde_json` 的对象键按序（默认 `BTreeMap`）输出，
// 且不存在 `HashMap` 迭代；`encode` 会做一次 **round-trip 等价**检查
// （反序列化 → 再序列化必须逐字节相同），NaN / Infinity 会在这一步暴露为错误。

use serde::{Deserialize, Serialize};

pub mod frame_hash;
pub mod input_stream;
pub mod sha256;

pub use input_stream::{
    FrameEvents, INPUT_FORMAT, InputError, InputStream, LibraryId, Pairing, Recorder,
    WINDOW_INPUT_FORMAT, WindowFrame, WindowRecorder, WindowStream,
};

use zela_scene_ecs::scene_script::SceneScript;

/// B4 Step 2：渲染参数（Meta 的 `render_params` 字段）。
///
/// 转发 kernel 层的纯数据类型 —— 回放端可直接用
/// `bundle.meta().render_params` 构造后端，不必再引一层依赖。
pub use ze_render_ir::RenderParams;

/// 文件魔数（4 字节）。
pub const MAGIC: &[u8; 4] = b"ZSB1";
/// 当前容器版本（决定"怎么读字节"，不属于内容 hash）。
///
/// v2：Meta 增加 `required_pods`（回放期所需），`pods` 降级为装配期诊断字段。
/// v3：项目改名 XSB → ZSB；魔数与版本号同步，避免 "ZSB1 + version 2" 的自相矛盾。
/// v4：类型标签命名空间统一为 `zela.*`（payload 内容变化，旧 payload 不兼容）。
pub const CONTAINER_VERSION: u16 = 4;
/// payload 的格式标识（写入 Meta）。
pub const PAYLOAD_FORMAT: &str = "zsb.script/1";
const HEADER_LEN: usize = 24;

/// **CD-a / CI-c**：能力在回放端的**角色**。
///
/// 一条判据即可归位：**回放端要执行的是「结果」、「源」还是「逻辑」？**
/// - 结果 ⇒ [`Baked`](Self::Baked)：不校验（payload 里已有烘焙数据）；
/// - 源 ⇒ [`Source`](Self::Source)：有同名能力可**重建**，无则用烘焙数据（不失败）；
/// - 逻辑 ⇒ [`Runtime`](Self::Runtime)：必须有且版本相等（既有行为）。
///
/// **CI-c（§0.6）**：`role` 是**必填**字段 —— 缺失即解码期报错，不再用
/// `#[serde(default)]` 给不存在的旧产物留分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityRole {
    /// 只带结果：回放端不需要该能力。
    Baked,
    /// 带源：有同名能力时可重建；没有则直接用烘焙数据。
    Source,
    /// 回放端必须执行的逻辑。
    Runtime,
}

/// 一条能力版本声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    /// `None` = 该能力的实现没有声明版本（仍要求名字存在）。
    pub version: Option<String>,
    /// **CD-a / CI-c**：回放端角色（**必填**，缺失即解码期报错）。
    pub role: CapabilityRole,
}

impl Capability {
    pub fn new(name: impl Into<String>, version: Option<&str>) -> Self {
        Self {
            name: name.into(),
            version: version.map(str::to_string),
            role: CapabilityRole::Runtime,
        }
    }

    /// **CD**：显式声明角色（链式，构造点就近表达）。
    pub fn with_role(mut self, role: CapabilityRole) -> Self {
        self.role = role;
        self
    }
}

/// 一条 Pod 版本声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pod {
    pub name: String,
    pub version: String,
}

impl Pod {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// 一条**降级记录**：哪个 Custom 规格走了哪条降级路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Downgrade {
    pub spec: String,
    pub path: String,
}

/// **CD-c**：回放端本次消费内容的方式（诊断输出用，不写进产物）。
///
/// 两者**不是**同一种操作（见 [`Meta`] 文档）：
/// - [`Restore`](Self::Restore)：用 payload 里的烘焙轮廓 ⇒ 逐位一致；
/// - [`Rebuild`](Self::Rebuild)：用 payload 里的源 + 本地能力重算 ⇒ 可复现，
///   但**不保证**与原产物逐位相同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    /// 直接用烘焙数据。
    Restore,
    /// 用源重新烘焙。
    Rebuild,
}

/// Meta 段（不进内容 hash）。
/// # restore 与 rebuild 是两个操作（CD）
///
/// - **restore（恢复）**：直接用 payload 里的烘焙数据 ⇒ 逐位一致，可哈希校验；
/// - **rebuild（重建）**：用 payload 里的**源**（如 `ContentOrigin`）+ 本地能力
///   重新烘焙 ⇒ 可复现，但**不保证**与原产物逐位相同。
///
/// `Source` 能力的版本差异因此是**可解释的差异**，不是错误。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// 容器版本（冗余写入，便于诊断；读取以 Header 为准）。
    pub container: u16,
    /// payload 格式标识。
    pub payload_format: String,
    /// **装配期参与者**（诊断信息：装配时调用了哪些能力）。
    ///
    /// **不参与** [`Bundle::check_restorable`]：payload 是已烘焙的纯数据，
    /// 回放端不需要重新执行这些能力。
    pub capabilities: Vec<Capability>,
    /// **回放期所需**能力（可还原性检查的唯一能力依据）。
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    /// **装配期参与者**（诊断信息：谁生成了这份 payload）。
    ///
    /// **不参与** [`Bundle::check_restorable`]：内容/字体等 Pod 只在编译期需要，
    /// 回放端拿到的是已烘焙的纯数据。
    pub pods: Vec<Pod>,
    /// **回放期所需** Pod（可还原性检查的唯一 Pod 依据）。
    #[serde(default)]
    pub required_pods: Vec<Pod>,
    /// 降级记录（影响"一比一还原"能力）。
    pub downgrades: Vec<Downgrade>,
    /// 源脚本 SHA-256（诊断用，不进内容 hash）。
    pub source_sha256: Option<String>,
    /// **渲染参数**（B4 Step 2）：回放端照着它构造后端（D1）。
    ///
    /// 纯记录 + 消费，**不进内容 hash**（不做 payload 校验）；`#[serde(default)]`
    /// 让既有 ZSB 仍可读（缺字段 ⇒ `RenderParams::default()` = 当前行为）。
    #[serde(default)]
    pub render_params: crate::RenderParams,
    /// 对象数（交叉校验）。
    pub object_count: usize,
    /// payload 的 SHA-256（读取时校验；不参与自身的计算）。
    pub payload_sha256: String,
}

/// `encode` 的元信息输入（payload 侧的信息由实现自行填充）。
#[derive(Debug, Clone, Default)]
pub struct MetaInput {
    /// 装配期参与者（诊断，不参与可还原性检查）。
    pub capabilities: Vec<Capability>,
    /// 回放期所需能力（只有它参与可还原性检查）。
    pub required_capabilities: Vec<Capability>,
    /// 装配期参与者（诊断，不参与可还原性检查）。
    pub pods: Vec<Pod>,
    /// 回放期所需 Pod（只有它参与可还原性检查）。
    pub required_pods: Vec<Pod>,
    pub downgrades: Vec<Downgrade>,
    pub source_sha256: Option<String>,
    /// 渲染参数（B4 Step 2）：默认 = 当前行为（§0.2 默认等效）。
    pub render_params: RenderParams,
}

/// 当前运行时**能解释什么**（可还原性检查的输入）。
#[derive(Debug, Clone, Default)]
pub struct Available {
    pub capabilities: Vec<Capability>,
    pub pods: Vec<Pod>,
}

impl Available {
    pub fn new(capabilities: Vec<Capability>, pods: Vec<Pod>) -> Self {
        Self {
            capabilities,
            pods,
        }
    }

    /// **CD-a 协商查询**：本地是否有能**重建**该能力产物的同名实现。
    ///
    /// 判据只看名字：`Source` 的语义是"有同名能力就能从源重算"，
    /// 版本差异由重建方在自己的诊断里报告（§3.2：rebuild 不保证逐位相同）。
    pub fn can_rebuild(&self, capability: &Capability) -> bool {
        self.capabilities
            .iter()
            .any(|available| available.name == capability.name)
    }

    /// **CI-c 口子①**：按**源种类**问「本地能不能重建它」。
    ///
    /// 种类 → 能力名的映射写在
    /// [`ContentOrigin::capability_name`](ze_scene_core::components::ContentOrigin::capability_name)，
    /// 不在这里再铺一份 match —— 将来拆成两个 capability 时只改那一处。
    pub fn can_rebuild_source(
        &self,
        origin: &ze_scene_core::components::ContentOrigin,
    ) -> bool {
        self.capabilities
            .iter()
            .any(|available| available.name == origin.capability_name())
    }
}

/// 写入 / 读取失败的**显式**错误。
#[derive(Debug, Clone, PartialEq)]
pub enum BundleError {
    /// 字节数不足一个 Header。
    TooShort { len: usize },
    /// 魔数不匹配。
    BadMagic { found: [u8; 4] },
    /// 容器版本不受支持。
    UnsupportedContainer { found: u16 },
    /// 段长越界 / 文件被截断 / 尾部多余字节。
    MalformedLayout { detail: String },
    /// payload / meta 不是合法 JSON。
    Json { section: &'static str, message: String },
    /// `SceneScript` 还原失败（组件未登记 / 类型不匹配等）。
    Script { message: String },
    /// 序列化不具确定性（round-trip 不是逐字节相同，通常是 NaN / Infinity）。
    NonCanonical { message: String },
    /// payload hash 与 Meta 记录不一致。
    HashMismatch { stored: String, actual: String },
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len } => write!(f, "ZSB 太短（{len} B < {HEADER_LEN} B）"),
            Self::BadMagic { found } => {
                write!(f, "ZSB 魔数不匹配（找到 {found:?}，期望 {MAGIC:?}）")
            }
            Self::UnsupportedContainer { found } => write!(
                f,
                "不支持的 ZSB 容器版本 {found}（本实现支持 {CONTAINER_VERSION}）"
            ),
            Self::MalformedLayout { detail } => write!(f, "ZSB 布局非法：{detail}"),
            Self::Json { section, message } => write!(f, "ZSB {section} 段 JSON 非法：{message}"),
            Self::Script { message } => write!(f, "ZSB payload 还原失败：{message}"),
            Self::NonCanonical { message } => write!(f, "ZSB payload 不具确定性：{message}"),
            Self::HashMismatch { stored, actual } => {
                write!(f, "ZSB payload hash 不一致（记录 {stored}，实际 {actual}）")
            }
        }
    }
}

impl std::error::Error for BundleError {}

/// 可还原性检查失败：**完整清单**（缺哪个 cap / 版本不匹配）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreError {
    pub missing: Vec<String>,
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "ZSB 无法在当前运行时还原（共 {} 项）：",
            self.missing.len()
        )?;
        for item in &self.missing {
            writeln!(f, "  - {item}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RestoreError {}

/// 一个已解析的 ZSB 产物（payload 保持原始字节，hash 因此可复算）。
#[derive(Debug, Clone)]
pub struct Bundle {
    payload: Vec<u8>,
    meta: Meta,
}

impl Bundle {
    /// 从装配产物 + 元信息编码。
    ///
    /// - payload = `SceneScript` 的规范 JSON；
    /// - 会做一次 round-trip 等价检查（防止 NaN / Infinity / 非确定序列化）；
    /// - `duration` 必须有限。
    pub fn encode(script: &SceneScript, input: MetaInput) -> Result<Self, BundleError> {
        let text = script
            .to_json()
            .map_err(|e| BundleError::Script {
                message: e.to_string(),
            })?;

        // 字节确定性 / 有限性：反序列化再序列化必须逐字节相同。
        // NaN / Infinity 在 serde_json 中会退化成 `null`，round-trip 会在
        // 反序列化时报错或被下面的比较抓住。
        let back = SceneScript::from_json(&text).map_err(|e| BundleError::NonCanonical {
            message: e.to_string(),
        })?;
        let again = back.to_json().map_err(|e| BundleError::NonCanonical {
            message: e.to_string(),
        })?;
        if again != text {
            return Err(BundleError::NonCanonical {
                message: "反序列化后再序列化的字节不同（含 NaN / Infinity 或非确定顺序）"
                    .to_string(),
            });
        }

        // **静止时刻不进产物**（U3）：它是派生量、可能不可求；需要时由
        // `SceneScript::still_time()` 从 payload 现算（日志 / 默认帧数提示）。
        let payload = text.into_bytes();
        let payload_sha256 = sha256::hex(&sha256::sha256(&payload));
        let meta = Meta {
            container: CONTAINER_VERSION,
            payload_format: PAYLOAD_FORMAT.to_string(),
            capabilities: input.capabilities,
            required_capabilities: input.required_capabilities,
            pods: input.pods,
            required_pods: input.required_pods,
            downgrades: input.downgrades,
            source_sha256: input.source_sha256,
            render_params: input.render_params,
            object_count: script.len(),
            payload_sha256,
        };
        Ok(Self { payload, meta })
    }

    /// 二进制布局：`Header(24) | payload | meta`。
    pub fn to_bytes(&self) -> Vec<u8> {
        let meta = serde_json::to_vec(&self.meta).expect("Meta 只含可序列化字段");
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len() + meta.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&CONTAINER_VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // flags（保留）
        out.extend_from_slice(&(self.payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&(meta.len() as u64).to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_LEN);
        out.extend_from_slice(&self.payload);
        out.extend_from_slice(&meta);
        out
    }

    /// 解析二进制布局并**校验 payload hash**。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BundleError> {
        if bytes.len() < HEADER_LEN {
            return Err(BundleError::TooShort { len: bytes.len() });
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&bytes[0..4]);
        if &magic != MAGIC {
            return Err(BundleError::BadMagic { found: magic });
        }
        let container = u16::from_le_bytes([bytes[4], bytes[5]]);
        if container != CONTAINER_VERSION {
            return Err(BundleError::UnsupportedContainer { found: container });
        }
        // 段长是 u64；**必须先转 usize 再参与分配/切片**。
        // 32 位平台（wasm32）直接 `as usize` 会截断，巨大长度可能伪装成小值，
        // 后面的长度校验就失去意义。
        let payload_len = usize::try_from(u64::from_le_bytes(bytes[8..16].try_into().unwrap()))
            .map_err(|_| BundleError::MalformedLayout {
                detail: "payload 段长超出当前平台地址空间".to_string(),
            })?;
        let meta_len = usize::try_from(u64::from_le_bytes(bytes[16..24].try_into().unwrap()))
            .map_err(|_| BundleError::MalformedLayout {
                detail: "Meta 段长超出当前平台地址空间".to_string(),
            })?;
        let total = HEADER_LEN
            .checked_add(payload_len)
            .and_then(|v| v.checked_add(meta_len))
            .ok_or_else(|| BundleError::MalformedLayout {
                detail: "段长溢出".to_string(),
            })?;
        if bytes.len() != total {
            return Err(BundleError::MalformedLayout {
                detail: format!("文件长 {} B ≠ header 声明 {} B", bytes.len(), total),
            });
        }

        let payload = bytes[HEADER_LEN..HEADER_LEN + payload_len].to_vec();
        let meta_bytes = &bytes[HEADER_LEN + payload_len..];
        let meta: Meta =
            serde_json::from_slice(meta_bytes).map_err(|e| BundleError::Json {
                section: "Meta",
                message: e.to_string(),
            })?;

        let actual = sha256::hex(&sha256::sha256(&payload));
        if meta.payload_sha256 != actual {
            return Err(BundleError::HashMismatch {
                stored: meta.payload_sha256.clone(),
                actual,
            });
        }

        // 容器版本以 Header 为准；Meta 里的副本只做诊断。
        if meta.container != container {
            return Err(BundleError::MalformedLayout {
                detail: format!("Meta 容器版本 {} ≠ Header {container}", meta.container),
            });
        }
        Ok(Self { payload, meta })
    }

    /// 还原装配产物（纯数据，不需要脚本运行时）。
    pub fn script(&self) -> Result<SceneScript, BundleError> {
        let text = std::str::from_utf8(&self.payload).map_err(|e| BundleError::Script {
            message: e.to_string(),
        })?;
        SceneScript::from_json(text).map_err(|e| BundleError::Script {
            message: e.to_string(),
        })
    }

    /// payload 原始字节（hash 的输入）。
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// **CD-b**：payload 里记录的内容源（`rebuild` 的输入；`restore` 不需要）。
    ///
    /// 没有内容源的产物（纯几何 / 老 ZSB）返回空表 —— 不失败。
    pub fn text_sources(&self) -> Result<Vec<ze_scene_core::components::ContentOrigin>, BundleError> {
        Ok(self
            .script()?
            .content_origins()
            .into_iter()
            .cloned()
            .collect())
    }

    /// **CD-c 诊断**：本次回放走 `restore` 还是 `rebuild`。
    ///
    /// 判据只有两条，且都从产物与本地能力推出（不猜）：
    /// 1. Meta 里有 `role = Source` 的能力，且本地有同名实现（[`Available::can_rebuild`]）；
    /// 2. payload 里确实有内容源（没有源就无从重建）。
    ///
    /// 两条都成立 ⇒ [`ContentMode::Rebuild`]；否则 （无本字库/排版引擎、或无源）
    /// ⇒ [`ContentMode::Restore`] —— 回放端用烘焙数据，行为不变。
    pub fn content_mode(&self, available: &Available) -> Result<ContentMode, BundleError> {
        let sources = self.text_sources()?;
        // **CI-c**：按**每个源自己的种类**问能力 —— 源不再隐含"一定是 Typst"。
        let can_rebuild = sources.iter().any(|origin| {
            self.meta.required_capabilities.iter().any(|capability| {
                capability.role == CapabilityRole::Source
                    && capability.name == origin.capability_name()
                    && available.can_rebuild(capability)
            })
        });
        if can_rebuild {
            Ok(ContentMode::Rebuild)
        } else {
            Ok(ContentMode::Restore)
        }
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    /// 内容 hash（只覆盖 payload 段）。
    pub fn payload_hash(&self) -> [u8; 32] {
        sha256::sha256(&self.payload)
    }

    pub fn payload_hash_hex(&self) -> String {
        sha256::hex(&self.payload_hash())
    }

    /// **可还原性检查**：逐项确认当前运行时能解释 Meta 声明的
    /// **回放期所需能力/ Pod**（`required_capabilities` / `required_pods`）。
    ///
    /// 装配期字段（`capabilities` / `pods`）**不参与**本检查：它们只是
    /// "装配时用了什么"的诊断；回放端消费的是已烘焙的纯数据，不需要重新装配
    /// 内容/字体等能力。
    ///
    /// 失败时返回**完整清单**（不是遇到第一个就返回）。
    pub fn check_restorable(&self, available: &Available) -> Result<(), RestoreError> {
        let mut missing = Vec::new();

        for required in &self.meta.required_capabilities {
            match required.role {
                // 结果：payload 已有烘焙数据，回放端不需要执行它。
                CapabilityRole::Baked => {}
                // 源：能重建更好，不能就用烘焙数据 —— 两种情况都**不失败**。
                CapabilityRole::Source => {}
                // 逻辑：既有行为（名字 + 版本严格相等）。
                CapabilityRole::Runtime => {
                    match available
                        .capabilities
                        .iter()
                        .find(|c| c.name == required.name)
                    {
                        None => missing.push(format!("缺少能力 `{}`", required.name)),
                        Some(found) => {
                            if required.version.is_some() && found.version != required.version {
                                missing.push(format!(
                                    "能力 `{}` 版本不匹配（产物要求 {:?}，当前 {:?}）",
                                    required.name, required.version, found.version
                                ));
                            }
                        }
                    }
                }
            }
        }

        for required in &self.meta.required_pods {
            match available.pods.iter().find(|p| p.name == required.name) {
                None => missing.push(format!("缺少 Pod `{}`", required.name)),
                Some(found) => {
                    if found.version != required.version {
                        missing.push(format!(
                            "Pod `{}` 版本不匹配（产物要求 {}，当前 {}）",
                            required.name, required.version, found.version
                        ));
                    }
                }
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(RestoreError { missing })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_scene_core::Circle;
    use zela_scene_ecs::scene_script::SceneEncoder;
    use zela_scene_ecs::scene::Scene;

    fn tiny_script() -> SceneScript {
        use ze_comb::{ChannelRef, Expr, OpRef};
        let mut enc = SceneEncoder::new();
        let circle = enc.spawn(vec![Box::new(Circle::new((0.0, 0.0), 1.0))]);
        // `radius = 1 + 2 · clamp(t/2, 0, 1)`：旧的 `Keyframes (1 → 3) over 2s`。
        let progress = Expr::call(
            OpRef::new("zela.math", "clamp"),
            [
                Expr::call(
                    OpRef::new("zela.math", "scale"),
                    [
                        Expr::pure(0.5),
                        Expr::channel(ChannelRef::builtin("time")),
                    ],
                ),
                Expr::pure(0.0),
                Expr::pure(1.0),
            ],
        );
        let root = Expr::call(
            OpRef::new("zela.math", "add"),
            [
                Expr::pure(1.0),
                Expr::call(OpRef::new("zela.math", "scale"), [Expr::pure(2.0), progress]),
            ],
        );
        // 窗口 `[0, 2)`：静止时刻 = 2.0（与旧 `play(dur = 2.0)` 的段语义一致）。
        let window = Expr::call(
            OpRef::new("zela.math", "and"),
            [
                Expr::call(
                    OpRef::new("zela.math", "ge"),
                    [
                        Expr::channel(ChannelRef::builtin("time")),
                        Expr::pure(0.0),
                    ],
                ),
                Expr::call(
                    OpRef::new("zela.math", "lt"),
                    [
                        Expr::channel(ChannelRef::builtin("time")),
                        Expr::pure(2.0),
                    ],
                ),
            ],
        );
        enc.bind_expr_gated(
            circle,
            "Circle.radius",
            root,
            zela_scene_ecs::animation::LibraryRef {
                namespace: "zela.math".to_string(),
                version: "17".to_string(),
                op_revisions: Default::default(),
            },
            zela_scene_ecs::ClockProgram::new(),
            Some(window),
            zela_scene_ecs::animation::ResetBehavior::Hold,
            0.0,
        )
        .expect("表达式绑定合法");
        enc.finish()
    }

    fn meta_input() -> MetaInput {
        MetaInput {
            capabilities: vec![
                Capability::new("scene", Some("4")),
                Capability::new("zela.content", Some("1")),
            ],
            required_capabilities: vec![Capability::new("scene", Some("4"))],
            pods: vec![Pod::new("scene", "0.1.0"), Pod::new("content", "0.1.0")],
            required_pods: vec![Pod::new("scene", "0.1.0")],
            downgrades: Vec::new(),
            source_sha256: Some("deadbeef".to_string()),
            render_params: RenderParams::default(),
        }
    }

    #[test]
    fn roundtrip_preserves_payload_and_hash() {
        let script = tiny_script();
        let bundle = Bundle::encode(&script, meta_input()).expect("encode");
        let bytes = bundle.to_bytes();
        let read = Bundle::from_bytes(&bytes).expect("decode");
        assert_eq!(read.payload_hash(), bundle.payload_hash());
        assert_eq!(read.script().expect("script").to_json().unwrap(), script.to_json().unwrap());
        assert_eq!(read.meta().object_count, script.len());
    }

    #[test]
    fn meta_changes_do_not_change_content_hash() {
        let script = tiny_script();
        let a = Bundle::encode(&script, meta_input()).expect("encode a");
        let mut other = meta_input();
        other.source_sha256 = Some("cafebabe".to_string());
        other.downgrades.push(Downgrade {
            spec: "x".to_string(),
            path: "to_path".to_string(),
        });
        let b = Bundle::encode(&script, other).expect("encode b");
        assert_eq!(a.payload_hash_hex(), b.payload_hash_hex());
        assert_ne!(a.to_bytes(), b.to_bytes(), "Meta 段应不同");
    }

    #[test]
    fn determinism_is_byte_stable() {
        let script = tiny_script();
        let a = Bundle::encode(&script, meta_input()).expect("a");
        let b = Bundle::encode(&script, meta_input()).expect("b");
        assert_eq!(a.to_bytes(), b.to_bytes());
    }

    #[test]
    fn missing_required_capability_is_listed() {
        let script = tiny_script();
        let bundle = Bundle::encode(&script, meta_input()).expect("encode");
        let available = Available::new(vec![], vec![Pod::new("scene", "0.1.0")]);
        let error = bundle
            .check_restorable(&available)
            .expect_err("缺少 scene 能力");
        assert_eq!(error.missing.len(), 1, "必须列出全部缺失项：{error}");
        assert!(error.missing.iter().any(|m| m.contains("scene")));
    }

    /// **P0-1 验收**：装配期用到 content Pod，不影响只有 scene Pod 的回放端。
    #[test]
    fn assembly_pods_do_not_block_playback() {
        let script = tiny_script();
        let bundle = Bundle::encode(
            &script,
            MetaInput {
                capabilities: vec![Capability::new("scene", Some("4"))],
                required_capabilities: vec![Capability::new("scene", Some("4"))],
                // 编译期确实用了 content……
                pods: vec![Pod::new("scene", "0.1.0"), Pod::new("content", "0.1.0")],
                // ……但回放只需要 scene（payload 是已烘焙的纯数据）。
                required_pods: vec![Pod::new("scene", "0.1.0")],
                downgrades: Vec::new(),
                source_sha256: None,
                render_params: RenderParams::default(),
            },
        )
        .expect("encode");
        let browser_like = Available::new(
            vec![Capability::new("scene", Some("4"))],
            vec![Pod::new("scene", "0.1.0")],
        );
        bundle
            .check_restorable(&browser_like)
            .expect("只有 scene Pod 的回放端必须接受该 bundle");
    }

    #[test]
    fn version_mismatch_is_explicit() {
        let script = tiny_script();
        let bundle = Bundle::encode(&script, meta_input()).expect("encode");
        let available = Available::new(
            vec![
                Capability::new("scene", Some("3")),
                Capability::new("zela.content", Some("1")),
            ],
            vec![Pod::new("scene", "0.1.0"), Pod::new("content", "0.1.0")],
        );
        let error = bundle.check_restorable(&available).expect_err("版本不匹配");
        assert!(error.missing[0].contains("版本不匹配"));
    }

    // ─── CD-a：role 协商 ────────────────────────────────────────────────

    /// `Source` / `Baked` 缺失 ⇒ 不失败；`Runtime` 缺失仍失败。
    #[test]
    fn role_decides_whether_missing_capability_fails() {
        let script = tiny_script();
        let bundle = Bundle::encode(
            &script,
            MetaInput {
                capabilities: Vec::new(),
                required_capabilities: vec![
                    Capability::new("scene", Some("4")),
                    Capability::new("zela.content", Some("1")).with_role(CapabilityRole::Source),
                    Capability::new("fonts", Some("2")).with_role(CapabilityRole::Baked),
                ],
                pods: Vec::new(),
                required_pods: Vec::new(),
                downgrades: Vec::new(),
                source_sha256: None,
                render_params: RenderParams::default(),
            },
        )
        .expect("encode");

        // 只具备 scene：Source/Baked 缺失都不算失败。
        let only_scene = Available::new(vec![Capability::new("scene", Some("4"))], Vec::new());
        bundle
            .check_restorable(&only_scene)
            .expect("Source/Baked 缺失不得失败");

        // Runtime 缺失 ⇒ 仍失败（既有行为）。
        let nothing = Available::new(Vec::new(), Vec::new());
        let error = bundle.check_restorable(&nothing).expect_err("Runtime 必须存在");
        assert!(error.missing.iter().any(|m| m.contains("scene")));
    }

    #[test]
    fn can_rebuild_is_a_name_query() {
        let available = Available::new(
            vec![Capability::new("zela.content", Some("9"))],
            Vec::new(),
        );
        assert!(available.can_rebuild(&Capability::new("zela.content", Some("1"))));
        assert!(!available.can_rebuild(&Capability::new("fonts", Some("1"))));
    }

    // ─── CD-b：源与烘焙同时进产物 ──────────────────────────────────────

    fn text_script(text: &str) -> SceneScript {
        let mut enc = SceneEncoder::new();
        enc.spawn(vec![Box::new(ze_scene_core::components::ContentOrigin {
            kind: ze_scene_core::components::ContentOriginKind::Text,
            text: text.to_string(),
            font_size: 24.0,
            x: 10.0,
            y: 20.0,
            path_index: 0,
        })]);
        enc.finish()
    }

    /// payload 里能取回源文本（`rebuild` 的输入），且经字节往返仍一致。
    #[test]
    fn payload_keeps_content_source_for_rebuild() {
        let bundle = Bundle::encode(&text_script("Hi"), meta_input()).expect("encode");
        let read = Bundle::from_bytes(&bundle.to_bytes()).expect("decode");

        let sources = read.text_sources().expect("内容源可读");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].text, "Hi");
        assert_eq!(sources[0].path_index, 0);
        // 源进 payload ⇒ 内容 hash 覆盖它；meta 变化不影响。
        assert_eq!(read.payload_hash(), bundle.payload_hash());
    }

    /// **CI-c 口子①**：源种类随 payload 一起往返；文本 / 公式不混。
    #[test]
    fn content_origin_kind_round_trips_and_selects_the_capability() {
        use ze_scene_core::components::ContentOriginKind;
        let mut enc = SceneEncoder::new();
        enc.spawn(vec![Box::new(ze_scene_core::components::ContentOrigin {
            kind: ContentOriginKind::Formula,
            text: "x^2 + 1".to_string(),
            font_size: 18.0,
            x: 0.0,
            y: 0.0,
            path_index: 2,
        })]);
        let bundle = Bundle::encode(&enc.finish(), meta_input()).expect("encode");
        let read = Bundle::from_bytes(&bundle.to_bytes()).expect("decode");
        let sources = read.text_sources().expect("内容源可读");
        assert_eq!(sources[0].kind, ContentOriginKind::Formula);
        assert_eq!(sources[0].capability_name(), "zela.content");
    }

    /// **CI-c / §0.6**：`role` 必填 —— 缺失即解码期显式报错（无静默默认）。
    #[test]
    fn capability_role_is_required_at_decode_time() {
        let json = r#"{"name":"zela.scene","version":"4"}"#;
        let error = serde_json::from_str::<Capability>(json).expect_err("缺 role 必须失败");
        assert!(
            error.to_string().contains("role"),
            "错误必须点名缺失字段：{error}"
        );
    }

    /// 不含内容源的产物（纯几何 / 老 ZSB）仍可读，源表为空而不是失败。
    #[test]
    fn payload_without_content_source_still_decodes() {
        let bundle = Bundle::encode(&tiny_script(), meta_input()).expect("encode");
        let read = Bundle::from_bytes(&bundle.to_bytes()).expect("decode");
        assert!(read.text_sources().expect("源表可为空").is_empty());
        assert_eq!(read.script().expect("script").len(), 1);
    }

    // ─── CD-c：restore / rebuild 诊断 ──────────────────────────────────

    fn source_bundle() -> Bundle {
        let mut input = meta_input();
        // 内容能力标为 Source（CD-d 起由 zela-app 自动写入；此处显式构造）。
        input.required_capabilities = vec![
            Capability::new("scene", Some("4")),
            Capability::new("zela.content", Some("1")).with_role(CapabilityRole::Source),
        ];
        Bundle::encode(&text_script("Hi"), input).expect("encode")
    }

    /// 本地有内容能力 + payload 有源 ⇒ rebuild；缺任一条 ⇒ restore。
    #[test]
    fn content_mode_reports_restore_or_rebuild() {
        let bundle = source_bundle();
        let with_content = Available::new(
            vec![
                Capability::new("scene", Some("4")),
                Capability::new("zela.content", Some("1")),
            ],
            Vec::new(),
        );
        assert_eq!(
            bundle.content_mode(&with_content).expect("诊断"),
            ContentMode::Rebuild
        );

        // 只有 scene（例如浏览器无内容引擎）⇒ 用烘焙数据。
        let scene_only = Available::new(vec![Capability::new("scene", Some("4"))], Vec::new());
        assert_eq!(
            bundle.content_mode(&scene_only).expect("诊断"),
            ContentMode::Restore
        );
    }

    /// 没有内容源的产物：即使本地有内容能力，也没有可重建的输入 ⇒ restore。
    #[test]
    fn content_mode_without_source_is_restore() {
        let mut input = meta_input();
        input.required_capabilities = vec![
            Capability::new("scene", Some("4")),
            Capability::new("zela.content", Some("1")).with_role(CapabilityRole::Source),
        ];
        let bundle = Bundle::encode(&tiny_script(), input).expect("encode");
        let with_content = Available::new(
            vec![
                Capability::new("scene", Some("4")),
                Capability::new("zela.content", Some("1")),
            ],
            Vec::new(),
        );
        assert_eq!(
            bundle.content_mode(&with_content).expect("诊断"),
            ContentMode::Restore
        );
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let script = tiny_script();
        let bundle = Bundle::encode(&script, meta_input()).expect("encode");
        let mut bytes = bundle.to_bytes();
        let last = bytes.len() - 1;
        // 改 payload 一个字节（payload 在 Header 之后、Meta 之前）。
        bytes[HEADER_LEN] ^= 0x01;
        let error = Bundle::from_bytes(&bytes).expect_err("hash 必须不一致");
        assert!(matches!(error, BundleError::HashMismatch { .. }), "error={error}");
        let _ = last;
    }

    #[test]
    fn legacy_xsb_magic_is_rejected() {
        let mut bytes = Bundle::encode(&tiny_script(), meta_input())
            .expect("encode")
            .to_bytes();
        // 旧 .xsb 的魔数必须被明确拒绝（不得出现"能读但语义不明"的中间态）。
        bytes[0..4].copy_from_slice(b"XSB1");
        assert!(matches!(
            Bundle::from_bytes(&bytes),
            Err(BundleError::BadMagic { .. })
        ));
    }

    /// 还原后的场景可提交（纯数据 → Scene）。
    #[test]
    fn restored_script_commits_into_scene() {
        let bundle = Bundle::encode(&tiny_script(), meta_input()).expect("encode");
        let script = bundle.script().expect("script");
        let mut scene = Scene::new();
        let recipe = script.clone().commit(&mut scene).expect("commit");
        assert_eq!(recipe.handles.len(), 1);
        assert_eq!(recipe.bindings.bindings.len(), 1);
        assert_eq!(script.still_time(), Some(2.0));
    }

    // J：不可信输入（上传预览）
    //
    // wasm panic ⇒ `unreachable` trap ⇒ 实例 abort ⇒ 页面白屏无提示。
    // 开放上传后输入从"我们自己编的"变成"任意人的"：以下用例只断言
    // **`from_bytes` 返回 Err、绝不 panic**（不要求具体错误种类时用 `is_err`）。

    fn encoded_bytes() -> Vec<u8> {
        Bundle::encode(&tiny_script(), meta_input())
            .expect("encode")
            .to_bytes()
    }

    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    #[test]
    fn rejects_empty_bytes() {
        assert!(matches!(
            Bundle::from_bytes(&[]),
            Err(BundleError::TooShort { len: 0 })
        ));
    }

    /// 任意截断都必须显式失败（不能越界切片）。
    #[test]
    fn rejects_every_truncation_without_panic() {
        let bytes = encoded_bytes();
        for len in 0..bytes.len() {
            assert!(
                Bundle::from_bytes(&bytes[..len]).is_err(),
                "截断到 {len} B 仍被接受"
            );
        }
        Bundle::from_bytes(&bytes).expect("完整字节必须可读");
    }

    /// 长度字段被篡改成极大值：**先与剩余字节数比对，再分配**。
    #[test]
    fn rejects_huge_length_fields_without_allocating() {
        let bytes = encoded_bytes();

        let mut huge_payload = bytes.clone();
        huge_payload[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            Bundle::from_bytes(&huge_payload),
            Err(BundleError::MalformedLayout { .. })
        ));

        let mut huge_meta = bytes.clone();
        huge_meta[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            Bundle::from_bytes(&huge_meta),
            Err(BundleError::MalformedLayout { .. })
        ));

        // 32 位平台（wasm32）上 `as usize` 会截断这个值；必须走 try_from 拒绝。
        let mut beyond_usize = bytes;
        beyond_usize[8..16].copy_from_slice(&(u64::MAX / 2).to_le_bytes());
        assert!(matches!(
            Bundle::from_bytes(&beyond_usize),
            Err(BundleError::MalformedLayout { .. })
        ));
    }

    /// 版本字段非法 ⇒ 显式拒绝（不要"尽量猜着读"）。
    #[test]
    fn rejects_unsupported_container_version() {
        let mut bytes = encoded_bytes();
        bytes[4..6].copy_from_slice(&(CONTAINER_VERSION + 1).to_le_bytes());
        assert!(matches!(
            Bundle::from_bytes(&bytes),
            Err(BundleError::UnsupportedContainer { .. })
        ));
    }

    /// 随机字节 / 合法魔数 + 随机正文：**全部不得 panic**。
    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for len in [
            0usize, 1, 3, 4, 7, 8, 15, 16, 23, 24, 25, 48, 64, 128, 512, 4096,
        ] {
            for attempt in 0..64 {
                let mut bytes = vec![0u8; len];
                for byte in &mut bytes {
                    *byte = xorshift(&mut state) as u8;
                }
                // 一半用例加合法魔数：让随机正文真正走到容器版本 / 段长分支。
                if attempt % 2 == 0 && len >= 4 {
                    bytes[0..4].copy_from_slice(MAGIC);
                }
                // 只验"不炸"；返回值不作断言。
                let _ = Bundle::from_bytes(&bytes);
            }
        }
    }
}
