-- 跨语言 fixture runner（阶段 4，Lua 5.3+）：在同一套 JSON 上复算值序列哈希。
--
-- 哈希口径与 `ze-comb::Value::canonical_bytes` 逐字节一致（小端、f64/f32 位模式）。
-- 需要 Lua 5.3+（string.pack / 整数位运算 / 64 位整数回绕）。

local function read_file(path)
  local file, error_message = io.open(path, "rb")
  if not file then error(error_message) end
  local text = file:read("*a")
  file:close()
  return text
end

-- ─── 最小 JSON 解析（fixture 子集：对象 / 数组 / 数字 / 字符串 / 布尔）───
local function parse_json(text)
  local pos = 1

  local function skip()
    while text:sub(pos, pos):match("%s") do pos = pos + 1 end
  end

  local parse_value

  local function parse_string()
    assert(text:sub(pos, pos) == '"', "期望字符串")
    pos = pos + 1
    local out = {}
    while true do
      local c = text:sub(pos, pos)
      if c == '"' then
        pos = pos + 1
        break
      elseif c == "\\" then
        local e = text:sub(pos + 1, pos + 1)
        pos = pos + 2
        if e == "n" then out[#out + 1] = "\n"
        elseif e == "t" then out[#out + 1] = "\t"
        elseif e == "r" then out[#out + 1] = "\r"
        else out[#out + 1] = e end
      else
        out[#out + 1] = c
        pos = pos + 1
      end
    end
    return table.concat(out)
  end

  local function parse_number()
    local start = pos
    while text:sub(pos, pos):match("[-+0-9%.eE]") do pos = pos + 1 end
    local token = text:sub(start, pos - 1)
    local value = tonumber(token)
    if not value then error("非法数字：" .. token) end
    if not token:find("[%.eE]") then
      return math.tointeger(value) or value
    end
    return value
  end

  parse_value = function()
    skip()
    local c = text:sub(pos, pos)
    if c == "{" then
      pos = pos + 1
      local object = {}
      skip()
      if text:sub(pos, pos) == "}" then pos = pos + 1 return object end
      while true do
        skip()
        local key = parse_string()
        skip()
        assert(text:sub(pos, pos) == ":", "期望冒号")
        pos = pos + 1
        object[key] = parse_value()
        skip()
        local separator = text:sub(pos, pos)
        pos = pos + 1
        if separator == "}" then break end
        assert(separator == ",", "期望逗号")
      end
      return object
    elseif c == "[" then
      pos = pos + 1
      local array = {}
      skip()
      if text:sub(pos, pos) == "]" then pos = pos + 1 return array end
      while true do
        array[#array + 1] = parse_value()
        skip()
        local separator = text:sub(pos, pos)
        pos = pos + 1
        if separator == "]" then break end
        assert(separator == ",", "期望逗号")
      end
      return array
    elseif c == '"' then
      return parse_string()
    elseif text:sub(pos, pos + 3) == "true" then
      pos = pos + 4
      return true
    elseif text:sub(pos, pos + 4) == "false" then
      pos = pos + 5
      return false
    elseif text:sub(pos, pos + 3) == "null" then
      pos = pos + 4
      return nil
    else
      return parse_number()
    end
  end

  local value = parse_value()
  skip()
  return value
end

-- ─── 规范字节编码（与 Rust 逐字节一致）──────────────────────────────
local function f32(value)
  return (string.unpack("<f", string.pack("<f", value)))
end

local function canonical(value)
  if value.k == "f" then
    return "\0" .. string.pack("<d", value.v)
  elseif value.k == "i" then
    return "\6" .. string.pack("<i8", value.v)
  elseif value.k == "b" then
    return string.char(1, value.v and 1 or 0)
  elseif value.k == "t" then
    return string.char(2) .. string.pack("<I4", #value.v) .. value.v
  elseif value.k == "v2" then
    return string.char(3) .. string.pack("<f", value.v[1]) .. string.pack("<f", value.v[2])
  elseif value.k == "v3" then
    return string.char(4)
      .. string.pack("<f", value.v[1])
      .. string.pack("<f", value.v[2])
      .. string.pack("<f", value.v[3])
  end
  error("未知值种类 " .. tostring(value.k))
end

local PRIME = 0x100000001b3
local function hash_values(values)
  local hash = 0xcbf29ce484222325
  for _, value in ipairs(values) do
    local bytes = canonical(value)
    for index = 1, #bytes do
      hash = hash ~ string.byte(bytes, index)
      hash = hash * PRIME
    end
  end
  return hash
end

-- ─── 求值器 ─────────────────────────────────────────────────────────
local function as_number(value)
  assert(value.k == "f", "期望数值，实际 " .. tostring(value.k))
  return value.v
end
local function as_i64(value)
  assert(value.k == "i", "期望 I64，实际 " .. tostring(value.k))
  return value.v
end
local function as_vec2(value)
  assert(value.k == "v2", "期望 Vec2，实际 " .. tostring(value.k))
  return value.v
end
local function as_vec3(value)
  assert(value.k == "v3", "期望 Vec3，实际 " .. tostring(value.k))
  return value.v
end

local function literal(lit)
  if lit.type == "f32" or lit.type == "f64" or lit.type == "i32" then
    return { k = "f", v = lit.data }
  elseif lit.type == "i64" then
    return { k = "i", v = math.tointeger(lit.data) or lit.data }
  elseif lit.type == "bool" then
    return { k = "b", v = lit.data }
  elseif lit.type == "text" then
    return { k = "t", v = lit.data }
  elseif lit.type == "vec2" then
    return { k = "v2", v = { f32(lit.data[1]), f32(lit.data[2]) } }
  elseif lit.type == "vec3" then
    return { k = "v3", v = { f32(lit.data[1]), f32(lit.data[2]), f32(lit.data[3]) } }
  end
  error("未知字面量 " .. tostring(lit.type))
end

local function call_op(op, args)
  local name = op.name
  if name == "sin" then return { k = "f", v = math.sin(as_number(args[1])) } end
  if name == "cos" then return { k = "f", v = math.cos(as_number(args[1])) } end
  if name == "add" then return { k = "f", v = as_number(args[1]) + as_number(args[2]) } end
  if name == "mul" or name == "scale" then
    return { k = "f", v = as_number(args[1]) * as_number(args[2]) }
  end
  if name == "sum" then
    local total = 0.0
    for _, value in ipairs(args) do total = total + as_number(value) end
    return { k = "f", v = total }
  end
  if name == "clamp" then
    local x, lo, hi = as_number(args[1]), as_number(args[2]), as_number(args[3])
    if x < lo then return { k = "f", v = lo } end
    if x > hi then return { k = "f", v = hi } end
    return { k = "f", v = x }
  end
  if name == "vec2" then
    return { k = "v2", v = { f32(as_number(args[1])), f32(as_number(args[2])) } }
  end
  if name == "vec3" then
    return { k = "v3", v = { f32(as_number(args[1])), f32(as_number(args[2])), f32(as_number(args[3])) } }
  end
  if name == "x" or name == "y" then
    local index = name == "x" and 1 or 2
    local v = args[1].k == "v2" and as_vec2(args[1]) or as_vec3(args[1])
    return { k = "f", v = v[index] }
  end
  if name == "z" then return { k = "f", v = as_vec3(args[1])[3] } end
  if name == "length" then
    local v = args[1].k == "v2" and as_vec2(args[1]) or as_vec3(args[1])
    local total = 0.0
    for _, component in ipairs(v) do total = total + component * component end
    return { k = "f", v = math.sqrt(total) }
  end
  if name == "round_f32" then return { k = "f", v = f32(as_number(args[1])) } end
  if name == "lt" then return { k = "b", v = as_number(args[1]) < as_number(args[2]) } end
  if name == "gt" then return { k = "b", v = as_number(args[1]) > as_number(args[2]) } end
  if name == "le" then return { k = "b", v = as_number(args[1]) <= as_number(args[2]) } end
  if name == "ge" then return { k = "b", v = as_number(args[1]) >= as_number(args[2]) } end
  if name == "eq" or name == "ne" then
    local equal
    if args[1].k == "b" and args[2].k == "b" then
      equal = args[1].v == args[2].v
    else
      equal = as_number(args[1]) == as_number(args[2])
    end
    return { k = "b", v = (name == "eq") and equal or not equal }
  end
  if name == "and" then return { k = "b", v = args[1].v and args[2].v } end
  if name == "or" then return { k = "b", v = args[1].v or args[2].v } end
  if name == "not" then return { k = "b", v = not args[1].v } end
  error("未知算子 " .. tostring(op.namespace) .. "::" .. tostring(name))
end

local function children(expr)
  local node = expr.node
  if node.type == "call" then return node.data.args end
  if node.type == "select" then return { node.data.cond, node.data.then, node.data["else"] } end
  if node.type == "nat_rec" then return { node.data.count, node.data.init, node.data.step } end
  return {}
end

local function collect_defs(expr, defs)
  if expr.id then defs[expr.id] = expr end
  for _, child in ipairs(children(expr)) do collect_defs(child, defs) end
end

local eval_expr

eval_expr = function(expr, env, defs, memo)
  if expr.id and memo[expr.id] then return memo[expr.id] end
  local node = expr.node
  local value

  if node.type == "pure" then
    value = literal(node.data.value)
  elseif node.type == "channel" then
    local series = (env.channels or {})[node.data.channel.name]
    assert(series, "缺少通道 " .. tostring(node.data.channel.name))
    value = { k = "f", v = series[env.sample] }
  elseif node.type == "read" then
    local fields = (env.objects or {})[tostring(node.data.object)]
    assert(fields and fields[node.data.channel.name] ~= nil, "缺少组件")
    value = { k = "f", v = fields[node.data.channel.name] }
  elseif node.type == "ref" then
    local target = defs[node.data.to]
    assert(target, "悬空 Ref " .. tostring(node.data.to))
    value = eval_expr(target, env, defs, memo)
  elseif node.type == "call" then
    local args = {}
    for _, arg in ipairs(node.data.args) do args[#args + 1] = eval_expr(arg, env, defs, memo) end
    value = call_op(node.data.op, args)
  elseif node.type == "select" then
    local cond = eval_expr(node.data.cond, env, defs, memo)
    assert(cond.k == "b", "select 条件必须是 Bool")
    value = eval_expr(cond.v and node.data.then or node.data["else"], env, defs, memo)
  elseif node.type == "nat_rec" then
    local count = as_i64(eval_expr(node.data.count, env, defs, memo))
    assert(count >= 0, "NatRec.count 不能为负")
    local state = eval_expr(node.data.init, env, defs, memo)
    local kind = state.k
    for index = 1, count do
      local step_env = {
        sample = env.sample,
        channels = env.channels,
        objects = env.objects,
        rec = { prev = state, index = index },
      }
      local next_value = eval_expr(node.data.step, step_env, defs, {})
      assert(next_value.k == kind, "NatRec 状态类型不一致")
      state = next_value
    end
    value = state
  elseif node.type == "rec_prev" then
    assert(env.rec, "RecPrev 不在 NatRec.step 内")
    value = env.rec.prev
  elseif node.type == "rec_index" then
    assert(env.rec, "RecIndex 不在 NatRec.step 内")
    value = { k = "i", v = env.rec.index }
  else
    error("未知节点 " .. tostring(node.type))
  end

  if expr.id then memo[expr.id] = value end
  return value
end

local function expected_value(raw)
  local kind = type(raw)
  if kind == "number" then return { k = "f", v = raw } end
  if kind == "boolean" then return { k = "b", v = raw } end
  if kind == "string" then return { k = "t", v = raw } end
  if kind == "table" then
    if #raw == 2 then
      return { k = "v2", v = { f32(raw[1]), f32(raw[2]) } }
    end
    return { k = "v3", v = { f32(raw[1]), f32(raw[2]), f32(raw[3]) } }
  end
  error("无法理解的期望值")
end

local function same_value(a, b)
  if a.k ~= b.k then return false end
  if a.k == "b" or a.k == "i" then return a.v == b.v end
  if a.k == "t" then return a.v == b.v end
  if a.k == "f" then return a.v == b.v end
  if a.k == "v2" or a.k == "v3" then
    for index = 1, #a.v do
      if a.v[index] ~= b.v[index] then return false end
    end
    return true
  end
  return false
end

-- ─── 主流程 ─────────────────────────────────────────────────────────
-- 调用方（xtask）按排序后的顺序把 fixture 路径作为参数传入；
-- 没有参数时显式失败（不猜目录，避免 Windows / POSIX 列目录差异）。
local files = {}
for index = 1, #arg do files[#files + 1] = arg[index] end
if #files == 0 then
  error("用法：lua run.lua <fixture.json> ...")
end

local ok = 0
local skipped = 0
for _, path in ipairs(files) do
  local fixture = parse_json(read_file(path))
  -- W5：超越函数不保证跨语言逐位一致 ⇒ `cross_language = false` 的 fixture 只跑 Rust。
  if fixture.cross_language == false then
    skipped = skipped + 1
  else
  local samples = #fixture.expect
  for _, series in pairs(fixture.channels or {}) do
    if #series > samples then samples = #series end
  end
  local defs = {}
  collect_defs(fixture.expr, defs)

  local values = {}
  for sample = 0, samples - 1 do
    local env = {
      sample = sample,
      channels = fixture.channels or {},
      objects = fixture.objects or {},
      rec = nil,
    }
    local value = eval_expr(fixture.expr, env, defs, {})
    local expected = expected_value(fixture.expect[sample + 1])
    assert(same_value(value, expected), fixture.name .. "[" .. sample .. "] 值不符")
    values[#values + 1] = value
  end

  local actual = hash_values(values)
  local actual_text = string.format("%016x", actual)
  local expected_text = fixture.expect_hash:gsub("^0x", ""):lower()
  assert(actual_text == expected_text,
    fixture.name .. " 哈希不符：" .. actual_text .. " != " .. expected_text)
  ok = ok + 1
  end
end

print(string.format("[fixtures-lua] %d/%d ok（跳过 %d 个跨语言不安全 fixture）",
  ok, #files - skipped, skipped))
