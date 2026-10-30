// resource_cache.rs
//
// GPU 资源缓存（G2–G4）：
//   G2: ResourceKey（内容寻址 / 版本寻址）
//   G3: DeferredDestroyer（graveyard 3 帧延迟销毁）
//   G4: ResourceCache 重写：generation-aware slot + key_to_id 索引
//       + 所有 GPU 资源释放都走 schedule_destroy，绝不直接 drop。
//
// 本模块位于 kernel 层：纯 std，不依赖 wgpu / hecs / egui。
// `get_or_insert` / `upload_dirty` 的后端参数为泛型 `B: ?Sized`（通常传 `&mut dyn RenderBackend`，
// 但 kernel 层不知道 RenderBackend 的存在）。
// 需要 hecs::Entity 的旧 CacheKey 兼容层（From<CacheKey> for ResourceKey）留在 zela-render。

use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use crate::render_backend::{AccessError, GpuHandle, GpuResourceId};

// ─── ResourceKey (G2) ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ResourceKey {
    Content { type_id: TypeId, hash: u64 },
    Versioned {
        type_id: TypeId,
        id: u64,
        version: u64,
    },
    /// 名字寻址（BP-1）：同一类资源里按**稳定名字 + 版本**取。
    ///
    /// 用于"本来就有名字"的资源（绑定组、pass 的组），
    /// 生命周期与另外两个变体完全一致 —— 只换 key 维度，不动 generation / 延迟销毁 / dirty / 驱逐。
    Named {
        type_id: TypeId,
        name: &'static str,
        version: u64,
    },
}

impl ResourceKey {
    pub fn content<C: 'static>(hash: u64) -> Self {
        Self::Content {
            type_id: TypeId::of::<C>(),
            hash,
        }
    }

    pub fn versioned<C: 'static>(id: u64, version: u64) -> Self {
        Self::Versioned {
            type_id: TypeId::of::<C>(),
            id,
            version,
        }
    }

    /// 名字 + 版本寻址（BP-1）。
    ///
    /// `name` 必须是稳定字面量（与 `IMAGE_SLOTS` 同源），`version` 是它对外的代数：
    /// 版本一变就是另一条 key ⇒ 缓存重建，旧句柄走 3 帧延迟销毁。
    pub fn named<C: 'static>(name: &'static str, version: u64) -> Self {
        Self::Named {
            type_id: TypeId::of::<C>(),
            name,
            version,
        }
    }

    /// 名字（只有 [`Self::Named`] 有）。
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Self::Named { name, .. } => Some(*name),
            _ => None,
        }
    }

    pub fn type_id(&self) -> TypeId {
        match self {
            Self::Content { type_id, .. }
            | Self::Versioned { type_id, .. }
            | Self::Named { type_id, .. } => *type_id,
        }
    }
}

// ─── DeferredDestroyer (G3) ───────────────────────────────────────────────

#[derive(Debug)]
pub struct DeferredDestroyer {
    pending: Vec<(u64, GpuHandle)>, // (destroyed_at_submitted_frame, handle)
    graveyard_period: u64,          // 保守值 = 3
    submitted_frame: u64,
}

impl DeferredDestroyer {
    pub const DEFAULT_GRAVEYARD_PERIOD: u64 = 3;

    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            graveyard_period: Self::DEFAULT_GRAVEYARD_PERIOD,
            submitted_frame: 0,
        }
    }

    pub fn with_period(period: u64) -> Self {
        Self {
            pending: Vec::new(),
            graveyard_period: period,
            submitted_frame: 0,
        }
    }

    pub fn schedule_destroy(&mut self, handle: GpuHandle) {
        self.pending.push((self.submitted_frame, handle));
    }

    pub fn on_submit(&mut self) {
        self.submitted_frame = self.submitted_frame.saturating_add(1);
    }

    /// 回收已经安全的 pending，返回实际 drop 的数量。
    ///
    /// 当「提交帧号 + graveyard_period <= 当前 submitted_frame」时认为可以安全销毁：
    /// 即资源在被 schedule_destroy 之后必须再经过 graveyard_period 次 submit 计数推进才会真正 drop。
    /// 测试语义：提交帧 0 的资源在 graveyard=3 时，在 submitted_frame 抵达 3 后（3 次 on_submit）开始回收。
    pub fn begin_frame(&mut self) -> usize {
        let submitted = self.submitted_frame;
        let graveyard = self.graveyard_period;
        let mut count = 0usize;
        self.pending.retain(|(submitted_at, _)| {
            let safe_frame = submitted_at.saturating_add(graveyard);
            if safe_frame <= submitted {
                count += 1;
                false
            } else {
                true
            }
        });
        count
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn submitted_frame(&self) -> u64 {
        self.submitted_frame
    }

    pub fn graveyard_period(&self) -> u64 {
        self.graveyard_period
    }
}

impl Default for DeferredDestroyer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 缓存条目 + Slot (G4) ─────────────────────────────────────────────────

pub struct CacheEntry<T> {
    pub handle: GpuHandle,
    pub meta: T,
    last_used_frame: u64,
}

impl<T> CacheEntry<T> {
    pub fn idle_frames(&self, current_frame: u64) -> u64 {
        current_frame - self.last_used_frame
    }
}

struct Slot {
    generation: u32,
    free: bool,
}

// ─── ResourceCache (G4) ───────────────────────────────────────────────────

pub struct ResourceCache<T> {
    key_to_id: HashMap<ResourceKey, GpuResourceId>,
    slots: Vec<Slot>,
    free: Vec<u32>,
    entries: HashMap<GpuResourceId, CacheEntry<T>>,
    destroyer: DeferredDestroyer,
    dirty: HashSet<ResourceKey>,
    frame_clock: u64,
}

impl<T> Default for ResourceCache<T>
where
    T: 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ResourceCache<T>
where
    T: 'static,
{
    pub fn new() -> Self {
        Self {
            key_to_id: HashMap::new(),
            slots: Vec::new(),
            free: Vec::new(),
            entries: HashMap::new(),
            destroyer: DeferredDestroyer::new(),
            dirty: HashSet::new(),
            frame_clock: 0,
        }
    }

    // ── slot management ────────────────────────────────────────────────

    fn alloc_slot(&mut self) -> GpuResourceId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            if slot.generation == 0 {
                slot.generation = 1; // 避免 generation=0 与哨兵混淆
            }
            slot.free = false;
            GpuResourceId {
                index,
                generation: slot.generation,
            }
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 1,
                free: false,
            });
            GpuResourceId {
                index,
                generation: 1,
            }
        }
    }

    /// 释放 slot：把 handle 移入 destroyer，然后标记 slot free。
    fn release_slot(&mut self, id: GpuResourceId) {
        if id.is_sentinel() {
            return;
        }
        let idx = id.index() as usize;
        if idx >= self.slots.len() {
            return;
        }

        // 先把 handle 从 entries 取出来并投入 destroyer（延迟销毁），
        // 绝对不能让 CacheEntry 直接 drop 内部的 GpuHandle。
        if let Some(entry) = self.entries.remove(&id) {
            self.destroyer.schedule_destroy(entry.handle);
            // meta 直接丢弃（它不是 GPU 资源）
        }

        let slot = &mut self.slots[idx];
        slot.free = true;
        self.free.push(id.index());
    }

    // ── public API ─────────────────────────────────────────────────────

    pub fn begin_frame(&mut self) {
        self.frame_clock = self.frame_clock.saturating_add(1);
        // 每帧开始，收割 graveyard 中已安全的 pending GPU 资源
        let _dropped = self.destroyer.begin_frame();
    }

    pub fn notify_submit(&mut self) {
        self.destroyer.on_submit();
    }

    pub fn current_frame(&self) -> u64 {
        self.frame_clock
    }

    pub fn get<K: Into<ResourceKey>>(&self, key: K) -> Option<&GpuHandle> {
        let rk = key.into();
        let id = *self.key_to_id.get(&rk)?;
        self.entries.get(&id).map(|e| &e.handle)
    }

    pub fn key_id_of(&self, key: &ResourceKey) -> Option<&GpuResourceId> {
        self.key_to_id.get(key)
    }

    pub fn contains<K: Into<ResourceKey>>(&self, key: K) -> bool {
        let rk = key.into();
        self.key_to_id.contains_key(&rk)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn dirty_len(&self) -> usize {
        self.dirty.len()
    }

    /// graveyard 中等待延迟销毁的 GPU 资源数（G7 诊断用，见 [`DeferredDestroyer::pending_count`]）。
    ///
    /// 稳态上界 = `graveyard_period × 每帧被驱逐的条目数`：条目在 `evict_unused` 时
    /// 投入 pending，之后需再经过 `graveyard_period` 次 `notify_submit` 才会真正 drop。
    pub fn pending_destroy_count(&self) -> usize {
        self.destroyer.pending_count()
    }

    /// Generation 校验的句柄访问入口。
    pub fn get_by_id(&self, id: GpuResourceId) -> Result<&GpuHandle, AccessError> {
        if id.is_sentinel() {
            return Err(AccessError::Dropped);
        }
        let idx = id.index() as usize;
        let slot = self.slots.get(idx).ok_or(AccessError::Dropped)?;
        if slot.free || slot.generation != id.generation() {
            return Err(AccessError::Stale { id });
        }
        match self.entries.get(&id) {
            Some(entry) => Ok(&entry.handle),
            None => Err(AccessError::Dropped),
        }
    }

    pub fn get_or_insert<K, B>(
        &mut self,
        key: K,
        meta: T,
        upload: impl FnOnce(&T, &mut B) -> GpuHandle,
        backend: &mut B,
    ) -> &GpuHandle
    where
        K: Into<ResourceKey> + Copy,
        B: ?Sized,
    {
        let rk = key.into();
        let now = self.frame_clock;
        if !self.key_to_id.contains_key(&rk) {
            let mut handle = upload(&meta, backend);
            let id = self.alloc_slot();
            handle.assign_id(id);
            self.key_to_id.insert(rk, id);
            self.entries.insert(
                id,
                CacheEntry {
                    handle,
                    meta,
                    last_used_frame: now,
                },
            );
        }

        let id = *self.key_to_id.get(&rk).expect("inserted above");
        let entry = self.entries.get_mut(&id).expect("inserted above");
        entry.last_used_frame = now;
        &entry.handle
    }

    /// 直接插入一个已经构造好的 [`GpuHandle`]，不走 upload 回调，也不依赖任何后端 trait。
    ///
    /// 当 key 已经存在时，不替换已有条目，直接返回现有 handle 并更新 last_used_frame。
    /// 当 key 未命中时，为传入的 handle 分配 generation slot、写入 entries、记录 key→id 映射。
    pub fn insert_handle<K: Into<ResourceKey>>(
        &mut self,
        key: K,
        meta: T,
        mut handle: GpuHandle,
    ) -> &GpuHandle {
        let rk = key.into();
        let now = self.frame_clock;
        if !self.key_to_id.contains_key(&rk) {
            let id = self.alloc_slot();
            handle.assign_id(id);
            self.key_to_id.insert(rk, id);
            self.entries.insert(
                id,
                CacheEntry {
                    handle,
                    meta,
                    last_used_frame: now,
                },
            );
        }
        let id = *self.key_to_id.get(&rk).expect("inserted above");
        let entry = self.entries.get_mut(&id).expect("inserted above");
        entry.last_used_frame = now;
        &entry.handle
    }

    pub fn mark_dirty<K: Into<ResourceKey>>(&mut self, key: K) {
        let rk = key.into();
        if self.key_to_id.contains_key(&rk) {
            self.dirty.insert(rk);
        }
    }

    pub fn upload_dirty<B: ?Sized>(
        &mut self,
        backend: &mut B,
        mut rebuild: impl FnMut(&ResourceKey, &T, &mut B) -> GpuHandle,
    ) -> usize {
        let keys: Vec<ResourceKey> = self.dirty.drain().collect();
        let now = self.frame_clock;
        let mut count = 0;
        for rk in keys {
            let Some(&id) = self.key_to_id.get(&rk) else {
                continue;
            };
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            let mut new_handle = rebuild(&rk, &entry.meta, backend);
            new_handle.assign_id(id);
            // 旧句柄投入延迟销毁，绝不直接 drop
            let old_handle = std::mem::replace(&mut entry.handle, new_handle);
            self.destroyer.schedule_destroy(old_handle);
            entry.last_used_frame = now;
            count += 1;
        }
        count
    }

    pub fn mark_used<K>(&mut self, key: &K) -> bool
    where
        K: Into<ResourceKey> + Copy,
    {
        let rk = (*key).into();
        let now = self.frame_clock;
        let Some(&id) = self.key_to_id.get(&rk) else {
            return false;
        };
        match self.entries.get_mut(&id) {
            Some(entry) => {
                entry.last_used_frame = now;
                true
            }
            None => false,
        }
    }

    pub fn evict_unused(&mut self, max_idle_frames: u64) -> usize {
        let now = self.frame_clock;
        // 找到需要 evict 的 id 列表
        let mut evict_ids: Vec<(ResourceKey, GpuResourceId)> = Vec::new();
        for (rk, id) in &self.key_to_id {
            if let Some(entry) = self.entries.get(id) {
                if entry.idle_frames(now) > max_idle_frames {
                    evict_ids.push((*rk, *id));
                }
            }
        }
        let count = evict_ids.len();
        for (rk, id) in evict_ids {
            self.key_to_id.remove(&rk);
            self.dirty.remove(&rk);
            // release_slot 内部会 schedule_destroy(handle) + 回收 slot
            self.release_slot(id);
        }
        count
    }
}

/// 无元数据的 GPU 资源缓存（最常用形态）。
pub type GpuResourceCache = ResourceCache<()>;

#[cfg(test)]
mod tests {
    use super::*;

    // ── G3 单测：graveyard 3 帧 ────────────────────────────────────────

    #[test]
    fn test_deferred_destroyer_graveyard_3() {
        let mut d = DeferredDestroyer::new();
        let h = GpuHandle::new(1_u32);
        d.schedule_destroy(h); // submitted_frame = 0
        for frame in 0..3 {
            // 模拟每帧 1 次 begin_frame + submit
            let dropped = d.begin_frame();
            d.on_submit();
            assert_eq!(dropped, 0, "frame {frame} should not drop");
        }
        // 第 4 次 begin_frame：completed = 3-3 = 0，submitted_at 0 <= 0，drop
        let dropped = d.begin_frame();
        assert_eq!(dropped, 1);
        assert_eq!(d.pending_count(), 0);
    }

    // ── G7 诊断：动画内容逐帧变化时 pending / 条目数有界 ──────────────
    /// 🔴 R5-2（G7 最关键一条）：复刻 `wgpu_2d::finish_frame` 的每帧调用序列 ——
    /// `begin_frame` → 插入本帧全部条目 → `evict_unused(0)` → `notify_submit` ——
    /// 并让"动画内容"每帧哈希都变（几何全 miss、全重建、全驱逐），
    /// 断言 pending 队列**不随时间增长**，而是稳定在
    /// `graveyard_period × 每帧条目数` 附近。
    #[test]
    fn per_frame_churn_keeps_pending_and_entries_bounded() {
        struct GeomTag;
        /// 每帧条目数（与 2D 后端一帧的几何条数同量级）。
        const PER_FRAME: u64 = 3;
        let mut cache = ResourceCache::<()>::new();

        let mut peak = 0usize;
        let mut pending_at_100 = 0usize;
        for frame in 0..200u64 {
            cache.begin_frame();
            for i in 0..PER_FRAME {
                // 内容哈希每帧都不同 = 动画内容（形变的圆 / 带相位的曲线）。
                let key = ResourceKey::content::<GeomTag>(frame * PER_FRAME + i);
                cache.insert_handle(key, (), GpuHandle::new(0u32));
            }
            cache.evict_unused(0);
            cache.notify_submit();
            peak = peak.max(cache.pending_destroy_count());
            if frame == 99 {
                pending_at_100 = cache.pending_destroy_count();
            }
        }

        // 1) 缓存条目恒等于"本帧条目数"（上一帧的已全部被驱逐）。
        assert_eq!(cache.len(), PER_FRAME as usize, "条目数不得随帧数增长");
        // 2) pending 在稳态是常数：第 100 帧与第 200 帧完全相同。
        assert_eq!(
            cache.pending_destroy_count(),
            pending_at_100,
            "pending 队列不得随时间增长"
        );
        // 3) 实测峰值上界 = (graveyard_period + 1) × 每帧条目数。
        let bound = ((DeferredDestroyer::DEFAULT_GRAVEYARD_PERIOD + 1) * PER_FRAME) as usize;
        assert!(peak <= bound, "pending 峰值 {peak} 越界（上界 {bound}）");
    }

    // ── BP-1：名字寻址（Named）与既有两变体语义一致 ─────────────────────

    struct Groups;

    /// 同名同版本 ⇒ 命中同一条目（句柄 id 不变）。
    #[test]
    fn named_key_hits_the_same_entry() {
        let mut cache = ResourceCache::<()>::new();
        let key = ResourceKey::named::<Groups>("scene", 1);
        let first = cache
            .insert_handle(key, (), GpuHandle::new(7_u32))
            .id();
        assert_eq!(key.name(), Some("scene"));
        let again = cache.get(key).expect("命中").id();
        assert_eq!(first, again, "同名同版本必须是同一条目");
        assert_eq!(
            cache.get(key).and_then(|h| h.downcast_ref::<u32>()),
            Some(&7),
            "再插入不得替换已命中的句柄"
        );
        assert_eq!(cache.len(), 1, "不得因再取而新增条目");
    }

    /// 版本变化 ⇒ 未命中 ⇒ 插入后是新条目（旧条目仍在，由驱逐回收）。
    #[test]
    fn named_key_version_change_misses() {
        let mut cache = ResourceCache::<()>::new();
        let old = ResourceKey::named::<Groups>("lighting", 1);
        let new = ResourceKey::named::<Groups>("lighting", 2);
        let old_id = cache.insert_handle(old, (), GpuHandle::new(1_u32)).id();
        assert!(cache.get(new).is_none(), "版本变了就是另一条 key");
        let new_id = cache.insert_handle(new, (), GpuHandle::new(2_u32)).id();
        assert_ne!(old_id, new_id);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(old).expect("旧条目还在").id(), old_id);
    }

    /// `evict_unused` 之后重取：先 miss，重新插入得到新 id，旧句柄进延迟销毁。
    #[test]
    fn named_key_after_evict_is_rebuilt() {
        let mut cache = ResourceCache::<()>::new();
        let key = ResourceKey::named::<Groups>("image_groups", 3);
        cache.begin_frame();
        let first = cache.insert_handle(key, (), GpuHandle::new(1_u32)).id();

        // 空闲 1 帧 ⇒ 超过 max_idle_frames = 0，被驱逐。
        cache.begin_frame();
        assert_eq!(cache.evict_unused(0), 1);
        assert!(cache.get(key).is_none(), "驱逐后旧 key 不得命中");
        assert_eq!(cache.pending_destroy_count(), 1, "旧句柄走延迟销毁");

        let second = cache.insert_handle(key, (), GpuHandle::new(1_u32)).id();
        assert_ne!(first, second, "重建必须换 slot（旧 id 已失效）");
        assert!(cache.get_by_id(first).is_err(), "旧 id 必须是 Stale / Dropped");
        cache.get_by_id(second).expect("新 id 可访问");
    }

    /// 同名不同类型的资源互不命中（type_id 参与 key）。
    #[test]
    fn named_key_carries_the_type() {
        struct OtherGroups;
        let a = ResourceKey::named::<Groups>("scene", 1);
        let b = ResourceKey::named::<OtherGroups>("scene", 1);
        assert_ne!(a, b);
        let mut cache = ResourceCache::<()>::new();
        cache.insert_handle(a, (), GpuHandle::new(1_u32));
        assert!(cache.get(b).is_none());
    }

    /// 三个变体只在 key 维度不同：同一个名字不会与内容 / 版本寻址的 key 撞上。
    #[test]
    fn named_key_is_distinct_from_the_other_two() {
        assert_ne!(
            ResourceKey::named::<Groups>("x", 0),
            ResourceKey::content::<Groups>(0)
        );
        assert_ne!(
            ResourceKey::named::<Groups>("x", 0),
            ResourceKey::versioned::<Groups>(0, 0)
        );
        assert_eq!(ResourceKey::content::<Groups>(0).name(), None);
        assert_eq!(ResourceKey::versioned::<Groups>(0, 0).name(), None);
    }
}
