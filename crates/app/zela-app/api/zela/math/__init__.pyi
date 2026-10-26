# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。
from typing import Any, List

from zela import Handle


def Add(a: Handle, b: Handle) -> Handle: ...

class Bind:
    object: Handle
    target: Channel
    root: Handle
    duration: float
    def __init__(self, object: Handle, target: Channel, root: Handle, duration: float) -> None: ...

class Channel:
    namespace: str
    name: str
    def __init__(self, namespace: str, name: str) -> None: ...

def Cos(of: Handle) -> Handle: ...

def Mul(a: Handle, b: Handle) -> Handle: ...

def Scale(k: float, of: Handle) -> Handle: ...

def Sin(of: Handle) -> Handle: ...

def Sum(terms: List[Handle]) -> Handle: ...

