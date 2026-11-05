// content.rs — **内容编译能力**（Injector 的第二次真实使用）。
//
// # 为什么是独立 Service，而不是 SceneApi 里的两个函数
//
// 公式 / 文字需要 `InMemoryEngine`（App 侧建一次的重资源）。把它塞进场景服务会让
// "场景装配" 与 "内容排版" 耦合；经 `Injector` 注入成第二个 Service，才说明
// 注入机制服务的是**能力**，不是某一个模块。
//
// # 为什么返回数据而不是对象
//
// `content.formula(...)` 返回 `Array<Path>`（按**阅读顺序**）。"阅读顺序" 是宿主
// 语义，不是通用场景操作 —— 脚本只消费结果，决定 spawn 几个对象。
//
// # 与 `zela-app::formula` 的关系
//
// 落点转换已下沉 `ze-content::stroke`（**唯一实现**，含常量）；
// 本模块只做引擎调用与类型适配。本模块不依赖 `zela-render`
// （那会让 scene 层反向依赖 render 层）。

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ze_content::StrokeOutline;
use ze_notion::{Color32 as NotionColor, Path2D, Vec2 as NotionVec2};
use ze_pod_core::{
    FunctionId, Handle, HandleTable, Pod, PodError, PodId, QualifiedName, RegistryPod,
};
use ze_prerender::InMemoryEngine;
use ze_scene_core::components::{ContentOrigin, ContentOriginKind};

use crate::types::{Color, Formula, Path, PathList, Text};

/// 注入时使用的 Service 名。
pub const CONTENT_SERVICE_NAME: &str = "content";
/// Service 版本（写入 ZSB Meta，用于可还原性检查）。
pub const CONTENT_SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// **内容路径表**：内容服务写入、场景服务读取的共享资源。
///
/// 句柄 owner 是本表自己的 `PodId`（不是两个 Service 之一）——
/// 两个 Service 都只是这张表的用户，跨 Service 传递因此不触发
/// [`ze_pod_core::HandleTable`] 的跨 Pod 检查。
#[derive(Clone)]
pub struct PathStore(Arc<Mutex<PathStoreInner>>);

/// 路径表里的一条：烘焙轮廓 + **可选**内容源（CD-b：源与烘焙同时进产物）。
#[derive(Clone)]
struct StoredPath {
    path: Path2D,
    origin: Option<ContentOrigin>,
}

struct PathStoreInner {
    owner: PodId,
    table: HandleTable<StoredPath>,
}

impl Default for PathStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PathStore {
    pub fn new() -> Self {
        let owner = PodId::next();
        Self(Arc::new(Mutex::new(PathStoreInner {
            owner,
            table: HandleTable::new(owner),
        })))
    }

    pub(crate) fn insert(&self, path: Path2D, origin: Option<ContentOrigin>) -> Handle {
        self.0
            .lock()
            .expect("路径表锁中毒")
            .table
            .insert(StoredPath { path, origin })
    }

    pub(crate) fn get(&self, handle: &Handle) -> Result<Path2D, PodError> {
        self.0
            .lock()
            .expect("路径表锁中毒")
            .table
            .get(handle)
            .map(|stored| stored.path.clone())
    }

    /// **CD-b**：取该路径记录的内容源（无源 ⇒ `None`，老产物 / 公式）。
    pub(crate) fn origin(&self, handle: &Handle) -> Option<ContentOrigin> {
        self.0
            .lock()
            .expect("路径表锁中毒")
            .table
            .get(handle)
            .ok()
            .and_then(|stored| stored.origin.clone())
    }

    /// 每轮脚本开始前清空（旧句柄随之失效）。
    pub(crate) fn reset(&self) {
        let mut inner = self.0.lock().expect("路径表锁中毒");
        inner.owner = PodId::next();
        inner.table = HandleTable::new(inner.owner);
    }
}

/// 宿主侧内容能力 Pod。
pub struct ContentServicePod {
    id: PodId,
    registry: RegistryPod,
    engine: Arc<InMemoryEngine>,
    paths: PathStore,
    /// 本 Service 是否**被脚本实际调用过**（`formula` / `text`）。
    ///
    /// 用途：ZSB Meta 记录"脚本实际用到的最小 capability 集"，
    /// 而不是"编译器注册的全部 capability"（后者会让回放端被要求
    /// 具备它根本用不到的 cap，`check_restorable` 误报）。
    used: AtomicBool,
}

impl ContentServicePod {
    /// 新建并注册 `formula` / `text`。
    pub fn new(engine: Arc<InMemoryEngine>, paths: PathStore) -> Self {
        let id = PodId::next();
        let mut registry = RegistryPod::new();
        registry.register_fn1::<Formula, PathList>("zela.content::formula", content_formula);
        registry.register_fn1::<Text, PathList>("zela.content::text", content_text);
        Self {
            id,
            registry,
            engine,
            paths,
            used: AtomicBool::new(false),
        }
    }

    /// 本 Service 是否被调用过（编译期元信息用）。
    pub fn used(&self) -> bool {
        self.used.load(Ordering::Relaxed)
    }
}

impl Pod for ContentServicePod {
    fn id(&self) -> PodId {
        self.id
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn functions(&self) -> &[QualifiedName] {
        self.registry.functions()
    }

    fn resolve(&self, name: &QualifiedName) -> Option<&(dyn Any + Send + Sync)> {
        self.registry.resolve(name)
    }

    fn call(&self, id: FunctionId, _args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        Err(PodError::CallFailed(format!(
            "ContentServicePod 的 `{id}` 只支持 typed 快路径（脚本经 ContentApi 调用）"
        )))
    }
}

fn service(pod: &dyn Pod) -> &ContentServicePod {
    pod.as_any()
        .downcast_ref::<ContentServicePod>()
        .expect("ContentApi 的调用者必须是 ContentServicePod")
}

// ─── typed 函数实现 ─────────────────────────────────────────────────────

fn content_formula(pod: &dyn Pod, spec: Formula) -> PathList {
    // **CI-c**：公式也是一个 `role = Source` 的内容源（kind = Formula）。
    // 源 = 作者写的 Typst 源码（不含自动 `$ $` 包装）；重建端按 kind 走数学路径。
    let origin = content_origin_meta(
        ContentOriginKind::Formula,
        &spec.source,
        spec.font_size,
        spec.x,
        spec.y,
    );
    compile(
        pod,
        &spec.source,
        spec.font_size,
        spec.x,
        spec.y,
        true,
        Some(origin),
    )
}

fn content_text(pod: &dyn Pod, spec: Text) -> PathList {
    let source = ze_content::text_literal_source(&spec.source);
    // **CD-b**：烘焙轮廓 + 原始文本**同时**进产物（后者供 rebuild）。
    let origin = content_origin_meta(
        ContentOriginKind::Text,
        &spec.source,
        spec.font_size,
        spec.x,
        spec.y,
    );
    compile(
        pod,
        &source,
        spec.font_size,
        spec.x,
        spec.y,
        false,
        Some(origin),
    )
}

/// 内容源的公共部分（`path_index` 在 `store_paths` 里按顺序补上）。
fn content_origin_meta(
    kind: ContentOriginKind,
    text: &str,
    font_size: f32,
    x: f32,
    y: f32,
) -> ContentOrigin {
    ContentOrigin {
        kind,
        text: text.to_string(),
        font_size,
        x,
        y,
        path_index: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn compile(
    pod: &dyn Pod,
    source: &str,
    font_size: f32,
    x: f32,
    y: f32,
    sort_reading_order: bool,
    origin: Option<ContentOrigin>,
) -> PathList {
    let this = service(pod);
    // 记录"内容能力被实际使用"（供 ZSB Meta 取最小 capability 集）。
    this.used.store(true, Ordering::Relaxed);
    let outlines = ze_scene_core::content::compile(
        &this.engine,
        source,
        font_size,
        NotionVec2::new(x, y),
        sort_reading_order,
    );
    PathList(store_paths(&this.paths, outlines, origin))
}

/// 世界坐标轮廓 → 脚本层 `Path`（句柄入表，颜色转脚本枚举）。
fn store_paths(
    store: &PathStore,
    outlines: Vec<StrokeOutline>,
    origin: Option<ContentOrigin>,
) -> Vec<Path> {
    // **文本是填充字形**：`stroke_style` 的默认细线宽是给"书写动画"（公式 / RevealPath）
    // 用的；若文本也带上它，`PathShape` 会在填充之外再描一圈 1pt 的边 ⇒ 字腔被填窄、
    // 开口闭合（实测 "s" / "a" 变成两个小孔的 8 字）。文本路径因此显式取 0 宽度，
    // 只走填充；仍要描边的调用方自己加 `Stroke` 组件覆盖。
    let fill_only = origin
        .as_ref()
        .is_some_and(|origin| origin.kind == ContentOriginKind::Text);
    outlines
        .into_iter()
        .enumerate()
        .map(|(index, outline)| Path {
            path: store.insert(
                outline.path,
                origin.clone().map(|mut origin| {
                    origin.path_index = index;
                    origin
                }),
            ),
            color: to_color(outline.color),
            width: if fill_only { 0.0 } else { outline.width },
        })
        .collect()
}

fn to_color(color: NotionColor) -> Color {
    Color {
        r: color.r(),
        g: color.g(),
        b: color.b(),
        a: color.a(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline(width: f32) -> StrokeOutline {
        StrokeOutline {
            path: Path2D::new(),
            color: NotionColor::WHITE,
            width,
        }
    }

    /// **回归（字腔闭合）**：文本路径必须是纯填充（宽度 0）；公式路径保留描边宽度
    /// （书写动画靠它可见）。
    #[test]
    fn text_paths_are_fill_only_while_formula_paths_keep_their_width() {
        let store = PathStore::new();
        let text = store_paths(
            &store,
            vec![outline(0.045)],
            Some(content_origin_meta(
                ContentOriginKind::Text,
                "a",
                1.0,
                0.0,
                0.0,
            )),
        );
        assert_eq!(text[0].width, 0.0, "文本不得带默认描边宽度");

        let formula = store_paths(
            &store,
            vec![outline(0.045)],
            Some(content_origin_meta(
                ContentOriginKind::Formula,
                "a",
                1.0,
                0.0,
                0.0,
            )),
        );
        assert!(
            (formula[0].width - 0.045).abs() < 1e-6,
            "公式路径的描边宽度必须保留：{}",
            formula[0].width
        );
    }
}
