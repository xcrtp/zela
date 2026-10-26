# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。
from typing import Any, List
from zela import Handle
from zela.math import Bind

# 数值字面量（装配期算术的结果落在这里）。
def pure(value: float) -> Handle: ...

# 时间通道（零元组合子；运行期由宿主注入）。
def time() -> Handle: ...

# 帧号通道（一等通道）。
def frame() -> Handle: ...

# 输入事件通道（`ViewportClick` / `ViewportHover` 的第一个消费方）。
def event() -> Handle: ...

# 组件状态通道（经 `SceneRead` 读取）。
def component_state() -> Handle: ...

# 运行环境通道（`RuntimeEval`）。
def env() -> Handle: ...

# `Sin { of = ... }`（表构造器；与 `spawn{}` 同构）。
def sin(spec: Any) -> Handle: ...

# `Cos { of = ... }`。
def cos(spec: Any) -> Handle: ...

# `Add { a = ..., b = ... }`。
def add(spec: Any) -> Handle: ...

# `Mul { a = ..., b = ... }`。
def mul(spec: Any) -> Handle: ...

# `Scale { k = ..., of = ... }`（`k` 进产物时是字面量叶子）。
def scale(spec: Any) -> Handle: ...

# `Sum { terms = { ... } }`（空数组 = 零元 `Pure(0)`）。
def sum(spec: Any) -> Handle: ...

# 把表达式导出为 JSON（**产物侧证据**：含库 namespace + version）。
def expression(root: Handle) -> str: ...

# 登记一条**表达式动画**（S3）：对象名 + 目标名 + 根句柄 + 时长。
# 
# 这里只记录；宿主在脚本结束后把它并入 `SceneScript`（对象名在装配期解析）。
# 
# ⚠️ 语义：表达式读**全局** `Time` / `Frame`；`duration` 只决定段区间（`Hold`）。
# 需要段内局部时间时应新增通道，而不是改 `Time` 的含义。
def bind(spec: Bind) -> None: ...

