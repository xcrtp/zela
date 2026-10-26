# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。
from typing import Any, List

# 在 `[from, to]` 上均匀取 `samples` 个点（Python 执行）。
def linspace(from_: float, to: float, samples: int) -> List[float]: ...

# Python 侧调用注入的 `scene.*` 建一个圆（**真实 Pod → 宿主调用**）。
def make_dot(x: float, y: float, r: float) -> None: ...

