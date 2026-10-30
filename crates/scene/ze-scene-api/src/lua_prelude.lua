-- lua_prelude.lua — 场景脚本的**前置块**（宿主在用户脚本之前执行）。
--
-- 为什么在前置块里而不是能力函数：`scene.text` 是**组合调用**（`content.text` 编译
-- 字形 → 逐字形 `PathShape` + `Parent`），它同时跨 `scene` 与 `content` 两个能力。
-- 做成能力函数会让 `zela.scene` 的签名变化 ⇒ 所有作品 payload 改名，
-- 而它的定位恰恰是"改写法不改产物"。
--
-- 契约：本文件**只**用已注入的脚本面（`scene.*` / `content.*` / 构造器全局表），
-- 不引入新的宿主调用；默认参数必须与"手写循环"逐位一致（`crates/app/zela-app/scripts`
-- 的四个文字作品就是这条契约的对拍基准）。

--- 编译一段文字并铺成"每个字形一个对象"的组。
--
-- @param source string  文字正文
-- @param opts table|nil 选项：
--   size             字号（世界单位；缺省 1.0）
--   x, y             摆放位置（缺省 0）
--   name             非 nil ⇒ 建一个组对象，并给它 `Name { value = name }`
--   group            true ⇒ 建组但不加 `Name`
--   parent           已存在的对象：字形挂到它下面（`Parent { parent = parent }`）
--   glyph_transform  true ⇒ **每个字形**各带一个 `Transform { x, y }`
--   style            组件句柄数组（如 `{ Fill { … }, Stroke { … } }`），逐字形追加
--   opacity          非 nil ⇒ 组对象带 `Opacity { value = opacity }`（组级淡出的起点）
--   inherit_opacity  true ⇒ 字形的 `Parent { parent = 组, opacity = 1.0 }`
--                    （缺省不继承：`Parent` 的样式权重默认为 0，与既有作品一致）
-- @return 组对象句柄（没建组 ⇒ nil）
--
-- 组级控制：组模式下拿到返回句柄后，只动它的 `Opacity.value`（需
-- `inherit_opacity = true`）/ `Transform` / `Parent.channel.*`，就能让**全部字形**
-- 一起变化 —— 每个字形都 `Parent` 到它，这是"整组一个 Transform"最直接的兑现。
--
-- 位置口径（与既有手写版一致）：
--   · 有组 / 有 `glyph_transform` ⇒ 内容在原点烘焙，位置由容器（组 Transform /
--     逐字形 Transform）承担；
--   · 否则（含只给 `parent` 的情形）⇒ 内容直接按 (x, y) 烘焙（`Text { x, y }`），
--     字形不再带 Transform（`parent` 只管层级，不改坐标）。
function scene.text(source, opts)
    opts = opts or {}
    local size = opts.size or 1.0
    local x = opts.x or 0.0
    local y = opts.y or 0.0
    local grouped = opts.group or opts.name ~= nil
    local text_x, text_y = x, y
    if grouped or opts.glyph_transform then
        text_x, text_y = 0.0, 0.0
    end

    local root = nil
    if grouped then
        local components = {}
        if opts.name ~= nil then
            components[#components + 1] = Name { value = opts.name }
        end
        components[#components + 1] = Transform { x = x, y = y }
        if opts.opacity ~= nil then
            components[#components + 1] = Opacity { value = opts.opacity }
        end
        root = scene.spawn(components)
    end

    local glyphs = content.text(Text {
        source = source,
        font_size = size,
        x = text_x,
        y = text_y,
    })
    for i = 1, #glyphs do
        local components = { PathShape { path = glyphs[i] } }
        if opts.style ~= nil then
            for j = 1, #opts.style do
                components[#components + 1] = opts.style[j]
            end
        end
        if opts.glyph_transform then
            components[#components + 1] = Transform { x = x, y = y }
        end
        local parent = root
        if parent == nil then
            parent = opts.parent
        end
        if parent ~= nil then
            if opts.inherit_opacity then
                components[#components + 1] = Parent { parent = parent, opacity = 1.0 }
            else
                components[#components + 1] = Parent { parent = parent }
            end
        end
        scene.spawn(components)
    end
    return root
end
