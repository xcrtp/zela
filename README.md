# zela

**用脚本写动画，渲染成 mp4 / 序列帧，也能在浏览器里回放。**
同一份作品在 native 与 WebGPU 浏览器两端逐帧一致 —— 不是"差不多"，
而是以帧哈希与结构指纹为判据的逐位一致。

- 写动画：Lua / Python / JS 脚本（装配期执行），产出 `.zsb` 装配产物；
- 渲染：headless GPU（wgpu / 矢量后端），mp4 或 PNG 序列帧；
- 回放：native GUI 与浏览器（WASM + WebGPU）共用一条回放路径。

> 想改引擎、接后端、复现确定性基线？看文末「我接下来读什么」。
> 缺口与未接线项：[`GAPS.md`](GAPS.md)。

## 5 分钟看到第一个 mp4

前置：

- Rust stable（`rustup` 安装即可）；
- `ffmpeg` 在 PATH（只做 PNG 序列帧时不需要）；
- Python 可选：`.py` 脚本需要构建时加 `--features python`。

```shell
# 1) 渲染内置作品（PNG 序列帧，不需要 ffmpeg）
cargo run -p zela-cli --bin zela -- render \
  crates/app/zela-app/scripts/storyboard.lua \
  --frames out/storyboard --size 640x480

# 2) 有 ffmpeg 时直接出 mp4
cargo run -p zela-cli --bin zela -- render \
  crates/app/zela-app/scripts/storyboard.lua \
  --out out/storyboard.mp4 --size 640x480
```

产物：`out/storyboard/*.png` 或 `out/storyboard.mp4`（135 帧 / 4.5s）。

- 打开 GUI：`cargo run -p zela-cli --features gui --bin zela -- gui`
- 查看全部内置作品：`zela list`
- 环境自检（字体 / ffmpeg / 后端）：`zela doctor`

> `zela` 是伞命令（package `zela-cli`，bin 名 `zela`）。上面用 `cargo run` 只是为了
> 从源码直接跑；`cargo install --path crates/app/zela-cli` 之后可以直接写 `zela render …`。

## 写第一个动画

新建 `hello.lua`：

```lua
-- 1) 一台 2D 相机：中心 (0,0)，缩放 40 px / 世界单位
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 40.0 },
})

-- 2) 一个圆：圆心 (0,0)，半径 0.2；填充浅蓝
local dot = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.2 },
    Fill { color = Color { r = 96, g = 196, b = 255, a = 255 } },
})

-- 3) 线性插值：from → to，用时 dur 秒（t 是时钟的局部时间）
local function ramp(from, to, dur)
    return Mix {
        a = expr.pure(from),
        b = expr.pure(to),
        t = Clamp {
            x = Scale { k = 1.0 / dur, of = clock.time() },
            lo = expr.pure(0.0),
            hi = expr.pure(1.0),
        },
    }
end

-- 4) 把半径从 0.2 动画到 1.0，2 秒后停住
expr.bind {
    object = dot,
    target = Circle.channel.radius,
    root = ramp(0.2, 1.0, 2.0),
    clock = clock.scale(1.0),
    duration = 2.0,
}
```

```shell
cargo run -p zela-cli --bin zela -- render hello.lua --out out/hello.mp4
```

逐行说明：

1. **场景 = 对象 + 组件**：`scene.spawn({...})` 创建对象，表里每一项是一个组件；
   `Camera2D` 决定取景，`Circle` + `Fill` 决定画什么。
2. **返回句柄**：`local dot = ...` 拿到对象句柄，后续动画用它定位目标。
3. **动画是表达式**：`Mix` / `Scale` / `Clamp` / `clock.time()` 都是纯表达式节点；
   `ramp` 只是把它们拼成"从 a 到 b、用时 dur"的常见形状。
4. **绑定**：`expr.bind { object, target, root }` 把表达式接到某个组件的某个通道；
   `duration` 让绑定在 2 秒后停住（内部展开成时间谓词）。

脚本在**装配期**执行一次，产出场景与绑定表；渲染时每帧只做纯函数求值，
所以 seek / 单帧导出 / 断点续渲都不会累积误差。

## 常用命令

| 命令 | 用途 |
|---|---|
| `zela render <script>` | 渲染成 mp4（`--frames DIR` 改为 PNG 序列帧） |
| `zela frames <script>` | 渲染成 PNG 序列帧（支持断点续渲） |
| `zela compile <script> --out x.zsb` | 编译成装配产物 |
| `zela check <script>` | 只检查脚本能否装配（不渲染，快速反馈） |
| `zela inspect <file.zsb>` | 查看产物元信息与可回放性 |
| `zela list` | 列出内置作品 |
| `zela play <id>` | 在 GUI 里播放某件内置作品（需 `--features gui`） |
| `zela api` | 生成 LuaLS / TypeScript / Python 补全声明 |
| `zela fonts` | 下载并校验确定性字体（自动设置 `ZELA_TYPST_FONT_DIR`） |
| `zela doctor` | 自检字体 / ffmpeg / 后端 / Python |
| `zela gui` | 打开 GUI 播放器（需 `--features gui`） |

`render` / `frames` 支持 `--from` / `--to` 只渲一段帧；`--frames` 产物带
`.zela-render.json`（参数指纹 + 每帧哈希），中断后重跑同一命令会从断点继续，
并校验 `Seek` 与顺序播放逐位一致。

## 能力索引

**不在这里复制能力清单**（复制 = 第二真源 = 必然腐烂）：

- 动画怎么写（时钟 / 谓词 / 通道 / 批量绑定）：[`docs/guides/animation.md`](docs/guides/animation.md)
- 能画什么（图元 / 样式 / 3D / 内容纹理）：[`docs/architecture/runtime.md`](docs/architecture/runtime.md)
- 确定性的四层口径（什么必须逐位一致）：[`docs/guides/determinism.md`](docs/guides/determinism.md)
- 后端选型与路由（wgpu / 矢量）：[`docs/guides/render-backends.md`](docs/guides/render-backends.md)
- 每个 crate 的职责 / 边界 / 运行机制：[`docs/modules/`](docs/modules/)

## 我接下来读什么

按角色入口（**同一份**定义在 [`docs/README.md`](docs/README.md#按角色阅读)，这里只给指针）：

- **想做动画**：先看上面的「写第一个动画」，再读
  [`docs/guides/animation.md`](docs/guides/animation.md) 与 `zela api` 生成的补全声明。
- **想改引擎**：[`docs/guides/adding-a-component.md`](docs/guides/adding-a-component.md)
  （加一个组件要改哪几处）与 [`docs/principles.md`](docs/principles.md)（门禁有哪些）。
- **想接后端**：[`docs/guides/render-backends.md`](docs/guides/render-backends.md)
  （后端契约、`feature_bits`、路由口径）。
- **想复现论文 / 基线**：[`docs/guides/determinism.md`](docs/guides/determinism.md)
  （确定性四层口径、基线怎么重录）。

## 开发与验证

```shell
# 本机能跑的一切（check / test / wasm / structure / docs / demo / smoke / hashes）
cargo verify

# 单步：structure | docs | hashes | ...
cargo verify --step structure

# 独立示例（不经过 zela-app 的完整装配）
cargo run -p zela-render-backends --example cube_3d       # 3D 后端最小 demo
cargo run -p zela-node-egui --example node_editor_demo    # 节点编辑器 demo
```

判据的权威说明在 [`docs/principles.md`](docs/principles.md) 与 [`CONTRIBUTING.md`](CONTRIBUTING.md)。

## 许可

MIT OR Apache-2.0；全文见 [LICENSE-MIT](LICENSE-MIT) 与 [LICENSE-APACHE](LICENSE-APACHE)。
