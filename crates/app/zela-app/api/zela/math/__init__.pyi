# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。
from typing import Any, List

from zela import Handle


def Add(a: Handle, b: Handle) -> Handle: ...

class Bind:
    object: str
    target: str
    root: Handle
    duration: float
    def __init__(self, object: str, target: str, root: Handle, duration: float) -> None: ...

def Cos(of: Handle) -> Handle: ...

def Mul(a: Handle, b: Handle) -> Handle: ...

def Scale(k: float, of: Handle) -> Handle: ...

def Sin(of: Handle) -> Handle: ...

def Sum(terms: List[Handle]) -> Handle: ...

