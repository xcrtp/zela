-- shaded_meshes.lua — B10 Step 4：**着色栈生效**的作品级证据。
--
-- 为什么需要一件新作品：B6 移植了 bloom / AO / AgX，但当时只有"纯 2D 路径 + 平面
-- 路径记录"能跑到着色栈，而合成 pass 的入场券要求**记录里有网格**（`Kind::Mesh`）。
-- 本作品用真网格（立方体）搭一个内角场景，让 `--backend vector` 第一次画出
-- "真 3D 网格 + 着色栈"的画面。
--
-- 画面上每项能力各有一处可判读的区域（判据见 `tests/vector3d_shading.rs`）：
--   tone   整幅（AgX 压回高光）
--   bloom  中央亮立方体外侧的辉光
--   ao     墙（+Z 面）与地板（−Y 面）交汇的内角
--
-- 环境图（IBL）默认关：PG Step 3 起由**宿主开关**触发（native `ZELA_IBL=1` / 浏览器
-- `set_environment`），用内置影棚（程序化生成、零资产）；脚本层仍没有"选哪个环境"的表达。

-- 相机：从 +Z 斜上方看向内角。
scene.spawn({
    Camera3D {
        eye = scene.vec3(2.6, 2.2, 6.0),
        target = scene.vec3(0.0, 0.8, -0.4),
        fov_y = 0.75,
    },
})

-- 灯：一盏强方向光（从左上前方射向场景）+ 一盏点光作补光。
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = scene.vec3(0.45, -0.55, -0.7),
        },
        color = Color { r = 255, g = 246, b = 232, a = 255 },
        intensity = 2.6,
    },
})
scene.spawn({
    Light {
        kind = LightKind.Point { range = 9.0 },
        color = Color { r = 120, g = 170, b = 255, a = 255 },
        intensity = 1.2,
    },
    Transform3D {
        position = scene.vec3(-2.4, 2.0, 2.4),
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = scene.vec3(1.0, 1.0, 1.0),
    },
})

-- 墙：薄板贴在 z = −1.2（面向 +Z）。
scene.spawn({
    Cube { half_extent = 1.0, solid = true },
    Material3D {
        color = Color { r = 150, g = 158, b = 172, a = 255 },
        variant = ShaderVariant.Pbr,
        roughness = 0.85,
    },
    Transform3D {
        position = scene.vec3(0.0, 1.6, -1.2),
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = scene.vec3(3.6, 1.6, 0.08),
    },
})

-- 地板：薄板贴在 y = 0（面向 +Y）。
scene.spawn({
    Cube { half_extent = 1.0, solid = true },
    Material3D {
        color = Color { r = 120, g = 124, b = 132, a = 255 },
        variant = ShaderVariant.Pbr,
        roughness = 0.95,
    },
    Transform3D {
        position = scene.vec3(0.0, 0.0, 0.4),
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = scene.vec3(3.6, 0.08, 2.0),
    },
})

-- 中央亮立方体：高光 + 低粗糙 ⇒ bloom 的外溢源。
scene.spawn({
    Cube { half_extent = 1.0, solid = true },
    Material3D {
        color = Color { r = 255, g = 250, b = 235, a = 255 },
        variant = ShaderVariant.Pbr,
        roughness = 0.18,
    },
    Transform3D {
        position = scene.vec3(0.0, 0.85, 0.2),
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = scene.vec3(0.7, 0.7, 0.7),
    },
})
