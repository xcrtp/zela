// 跨语言 fixture runner（阶段 4）：在同一套 JSON 上复算值序列哈希。
//
// 它不是"另一份实现"的偶然副本：哈希口径与 `ze-comb::Value::canonical_bytes`
// 逐字节一致，任何一边漂移都会在这里变成红色。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const MASK = (1n << 64n) - 1n;

const f64 = (value) => {
  const b = Buffer.alloc(9);
  b[0] = 0;
  b.writeDoubleLE(value, 1);
  return b;
};
const i64 = (value) => {
  const b = Buffer.alloc(9);
  b[0] = 6;
  b.writeBigInt64LE(BigInt(value), 1);
  return b;
};
const bool = (value) => Buffer.from([1, value ? 1 : 0]);
const text = (value) => {
  const bytes = Buffer.from(value, "utf8");
  const b = Buffer.alloc(5 + bytes.length);
  b[0] = 2;
  b.writeUInt32LE(bytes.length, 1);
  bytes.copy(b, 5);
  return b;
};
const vec2 = (value) => {
  const b = Buffer.alloc(9);
  b[0] = 3;
  b.writeFloatLE(value[0], 1);
  b.writeFloatLE(value[1], 5);
  return b;
};
const vec3 = (value) => {
  const b = Buffer.alloc(13);
  b[0] = 4;
  b.writeFloatLE(value[0], 1);
  b.writeFloatLE(value[1], 5);
  b.writeFloatLE(value[2], 9);
  return b;
};
const bytesValue = (value) => {
  const ty = Buffer.from(value.ty, "utf8");
  const head = Buffer.alloc(1 + 4 + ty.length + 4);
  head[0] = 5;
  head.writeUInt32LE(ty.length, 1);
  ty.copy(head, 5);
  head.writeUInt32LE(value.bytes.length, 5 + ty.length);
  return Buffer.concat([head, Buffer.from(value.bytes)]);
};
const canonical = (v) => {
  switch (v.k) {
    case "f": return f64(v.v);
    case "i": return i64(v.v);
    case "b": return bool(v.v);
    case "t": return text(v.v);
    case "v2": return vec2(v.v);
    case "v3": return vec3(v.v);
    case "bytes": return bytesValue(v);
    default: throw new Error(`未知值种类 ${v.k}`);
  }
};
const hashValues = (values) => {
  let h = FNV_OFFSET;
  for (const v of values) {
    for (const byte of canonical(v)) {
      h ^= BigInt(byte);
      h = (h * FNV_PRIME) & MASK;
    }
  }
  return h;
};

const asNumber = (v) => {
  if (v.k !== "f") throw new Error(`期望数值，实际 ${v.k}`);
  return v.v;
};
const asI64 = (v) => {
  if (v.k !== "i") throw new Error(`期望 I64，实际 ${v.k}`);
  return v.v;
};
const asVec2 = (v) => {
  if (v.k !== "v2") throw new Error(`期望 Vec2，实际 ${v.k}`);
  return v.v;
};
const asVec3 = (v) => {
  if (v.k !== "v3") throw new Error(`期望 Vec3，实际 ${v.k}`);
  return v.v;
};

const lerpF32 = (a, b, t) => Math.fround(a + (b - a) * Math.fround(t));

function evalLiteral(lit) {
  switch (lit.type) {
    case "f32": return { k: "f", v: lit.data };
    case "f64": return { k: "f", v: lit.data };
    case "i32": return { k: "f", v: lit.data };
    case "i64": return { k: "i", v: BigInt(lit.data) };
    case "bool": return { k: "b", v: lit.data };
    case "text": return { k: "t", v: lit.data };
    case "vec2": return { k: "v2", v: lit.data.map(Math.fround) };
    case "vec3": return { k: "v3", v: lit.data.map(Math.fround) };
    default: throw new Error(`未知字面量 ${lit.type}`);
  }
}

function callOp(op, args) {
  const num = (i) => asNumber(args[i]);
  switch (op.name) {
    case "sin": return { k: "f", v: Math.sin(num(0)) };
    case "cos": return { k: "f", v: Math.cos(num(0)) };
    case "add": return { k: "f", v: num(0) + num(1) };
    case "mul": return { k: "f", v: num(0) * num(1) };
    case "scale": return { k: "f", v: num(0) * num(1) };
    case "sum": return { k: "f", v: args.reduce((acc, v) => acc + asNumber(v), 0) };
    case "clamp": {
      const [x, lo, hi] = [num(0), num(1), num(2)];
      return { k: "f", v: x < lo ? lo : x > hi ? hi : x };
    }
    case "vec2": return { k: "v2", v: [Math.fround(num(0)), Math.fround(num(1))] };
    case "vec3": return { k: "v3", v: [Math.fround(num(0)), Math.fround(num(1)), Math.fround(num(2))] };
    case "x": return { k: "f", v: (args[0].k === "v2" ? asVec2(args[0]) : asVec3(args[0]))[0] };
    case "y": return { k: "f", v: (args[0].k === "v2" ? asVec2(args[0]) : asVec3(args[0]))[1] };
    case "z": return { k: "f", v: asVec3(args[0])[2] };
    case "splat": { const v = Math.fround(num(0)); return { k: "v3", v: [v, v, v] }; }
    case "vec3_from_vec2": return { k: "v3", v: [...asVec2(args[0]), Math.fround(num(1))] };
    case "vec2_from_vec3": { const v = asVec3(args[0]); return { k: "v2", v: [v[0], v[1]] }; }
    case "length": {
      const v = args[0].k === "v2" ? asVec2(args[0]) : asVec3(args[0]);
      return { k: "f", v: Math.hypot(...v) };
    }
    case "normalize": {
      const v = args[0].k === "v2" ? asVec2(args[0]) : asVec3(args[0]);
      const len = Math.hypot(...v);
      const unit = len > 1e-12 ? v.map((c) => c / len) : v.map(() => 0);
      return { k: args[0].k, v: unit };
    }
    case "round_f32": return { k: "f", v: Math.fround(num(0)) };
    case "mix": {
      const t = num(2);
      if (t === 0) return args[0];
      if (t === 1) return args[1];
      if (args[0].k === "f" && args[1].k === "f") {
        return { k: "f", v: lerpF32(num(0), num(1), t) };
      }
      if (args[0].k === "v2" && args[1].k === "v2") {
        const [a, b] = [asVec2(args[0]), asVec2(args[1])];
        return { k: "v2", v: [lerpF32(a[0], b[0], t), lerpF32(a[1], b[1], t)] };
      }
      if (args[0].k === "v3" && args[1].k === "v3") {
        const [a, b] = [asVec3(args[0]), asVec3(args[1])];
        return { k: "v3", v: [lerpF32(a[0], b[0], t), lerpF32(a[1], b[1], t), lerpF32(a[2], b[2], t)] };
      }
      throw new Error("mix 端点不同型");
    }
    case "lt": return { k: "b", v: num(0) < num(1) };
    case "gt": return { k: "b", v: num(0) > num(1) };
    case "le": return { k: "b", v: num(0) <= num(1) };
    case "ge": return { k: "b", v: num(0) >= num(1) };
    case "eq":
    case "ne": {
      let equal;
      if (args[0].k === "b" && args[1].k === "b") equal = args[0].v === args[1].v;
      else equal = num(0) === num(1);
      return { k: "b", v: op.name === "eq" ? equal : !equal };
    }
    case "and": return { k: "b", v: args[0].v && args[1].v };
    case "or": return { k: "b", v: args[0].v || args[1].v };
    case "not": return { k: "b", v: !args[0].v };
    default: throw new Error(`未知算子 ${op.namespace}::${op.name}`);
  }
}

function collectDefs(expr, defs) {
  if (expr.id) defs.set(expr.id, expr);
  for (const child of children(expr)) collectDefs(child, defs);
}
function children(expr) {
  const n = expr.node;
  switch (n.type) {
    case "call": return n.data.args;
    case "select": return [n.data.cond, n.data.then, n.data.else];
    case "nat_rec": return [n.data.count, n.data.init, n.data.step];
    default: return [];
  }
}

function evalExpr(expr, env, defs, memo) {
  if (expr.id && memo.has(expr.id)) return memo.get(expr.id);
  const node = expr.node;
  let value;
  switch (node.type) {
    case "pure": value = evalLiteral(node.data.value); break;
    case "channel": {
      const series = env.channels[node.data.channel.name];
      if (!series) throw new Error(`缺少通道 ${node.data.channel.name}`);
      value = { k: "f", v: series[env.sample] };
      break;
    }
    case "read": {
      const fields = env.objects[String(node.data.object)];
      if (!fields || fields[node.data.channel.name] === undefined) {
        throw new Error(`缺少组件 ${node.data.channel.name}`);
      }
      value = { k: "f", v: fields[node.data.channel.name] };
      break;
    }
    case "ref": {
      const target = defs.get(node.data.to);
      if (!target) throw new Error(`悬空 Ref ${node.data.to}`);
      value = evalExpr(target, env, defs, memo);
      break;
    }
    case "call": {
      const args = node.data.args.map((arg) => evalExpr(arg, env, defs, memo));
      value = callOp(node.data.op, args);
      break;
    }
    case "select": {
      const cond = evalExpr(node.data.cond, env, defs, memo);
      if (cond.k !== "b") throw new Error("select 条件必须是 Bool");
      value = evalExpr(cond.v ? node.data.then : node.data.else, env, defs, memo);
      break;
    }
    case "nat_rec": {
      const count = asI64(evalExpr(node.data.count, env, defs, memo));
      if (count < 0n) throw new Error("NatRec.count 不能为负");
      let state = evalExpr(node.data.init, env, defs, memo);
      const kind = state.k;
      for (let index = 1n; index <= count; index += 1n) {
        const stepEnv = { ...env, rec: { prev: state, index } };
        const next = evalExpr(node.data.step, stepEnv, defs, new Map());
        if (next.k !== kind) throw new Error("NatRec 状态类型不一致");
        state = next;
      }
      value = state;
      break;
    }
    case "rec_prev": {
      if (!env.rec) throw new Error("RecPrev 不在 NatRec.step 内");
      value = env.rec.prev;
      break;
    }
    case "rec_index": {
      if (!env.rec) throw new Error("RecIndex 不在 NatRec.step 内");
      value = { k: "i", v: env.rec.index };
      break;
    }
    default: throw new Error(`未知节点 ${node.type}`);
  }
  if (expr.id) memo.set(expr.id, value);
  return value;
}

function expectedValue(raw) {
  if (typeof raw === "number") return { k: "f", v: raw };
  if (typeof raw === "boolean") return { k: "b", v: raw };
  if (typeof raw === "string") return { k: "t", v: raw };
  if (Array.isArray(raw)) {
    return raw.length === 2
      ? { k: "v2", v: raw.map(Math.fround) }
      : { k: "v3", v: raw.map(Math.fround) };
  }
  throw new Error(`无法理解的期望值 ${JSON.stringify(raw)}`);
}

const listed = process.argv.slice(2);
const files = listed.length > 0
  ? listed
  : fs.readdirSync(here)
      .filter((name) => name.endsWith(".json"))
      .sort()
      .map((name) => path.join(here, name));
if (files.length === 0) throw new Error("fixture 目录为空");

let ok = 0;
for (const file of files) {
  const fixture = JSON.parse(fs.readFileSync(file, "utf8"));
  const samples = Math.max(
    fixture.expect.length,
    ...Object.values(fixture.channels ?? {}).map((series) => series.length),
    0,
  );
  const defs = new Map();
  collectDefs(fixture.expr, defs);
  const values = [];
  for (let sample = 0; sample < samples; sample += 1) {
    const env = {
      sample,
      channels: fixture.channels ?? {},
      objects: fixture.objects ?? {},
      rec: null,
    };
    const value = evalExpr(fixture.expr, env, defs, new Map());
    const expected = expectedValue(fixture.expect[sample]);
    if (JSON.stringify(value) !== JSON.stringify(expected)) {
      throw new Error(
        `${fixture.name}[${sample}] 值不符：${JSON.stringify(value)} != ${JSON.stringify(expected)}`,
      );
    }
    values.push(value);
  }
  const actual = hashValues(values);
  const expectedHash = BigInt(fixture.expect_hash);
  if (actual !== expectedHash) {
    throw new Error(
      `${fixture.name} 哈希不符：0x${actual.toString(16)} != 0x${expectedHash.toString(16)}`,
    );
  }
  ok += 1;
}
console.log(`[fixtures-js] ${ok}/${files.length} ok`);
