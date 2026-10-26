# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。
from typing import Any, List
from zela import Formula, Path, Text

# 编译公式源码（Typst）为描边路径。
def formula(spec: Formula) -> List[Path]: ...

# 编译纯文本为描边路径（正文外观）。
def text(spec: Text) -> List[Path]: ...

