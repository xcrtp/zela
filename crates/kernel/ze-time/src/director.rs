// director.rs — 时间推进控制
//
// Director 是唯一可推进时间的对象。advance / seek / set_speed / pause / resume
// 全部可序列化、可复制。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Director {
    pub global_t: f64,
    pub speed: f64,
    pub playing: bool,
    /// 已推进的帧数 —— **一等通道**（S3）。
    ///
    /// 暂停时冻结；`seek` 不改它（seek 是时间轴上的平移，不是帧号重置）。
    #[serde(default)]
    pub frame: u64,
    /// 折叠基准：`global_t` 在 `base_frame` 处的值。
    ///
    /// `t` 由 `(frame - base_frame) × base_dt × speed` **派生**，不再逐帧累加 ⇒
    /// 长片不会漂移；`seek` / 变速 / dt 变化只移动基准。
    #[serde(default)]
    base_t: f64,
    #[serde(default)]
    base_frame: u64,
    /// 基准处的帧步长（秒）；`0.0` = 尚未建立基准。
    #[serde(default)]
    base_dt: f64,
}

impl Default for Director {
    fn default() -> Self {
        Self::new()
    }
}

impl Director {
    pub fn new() -> Self {
        Self {
            global_t: 0.0,
            speed: 1.0,
            playing: true,
            frame: 0,
            base_t: 0.0,
            base_frame: 0,
            base_dt: 0.0,
        }
    }

    /// 时间推进（只在 playing 时）。
    ///
    /// 语义 = **由帧号派生**：`t = base_t + (frame − base_frame) × base_dt × speed`。
    /// `dt` 恒定（固定 fps）时就是 `t = frame × dt × speed`，不存在逐帧累加误差。
    /// `dt` 变化（实时渲染 / fps 切换）时先折叠基准，等价于旧行为但不再跨段漂移。
    pub fn advance(&mut self, dt: f64) {
        if !self.playing || dt <= 0.0 {
            return;
        }
        if self.base_dt == 0.0 || dt != self.base_dt {
            self.rebase(dt);
        }
        self.frame += 1;
        self.global_t =
            self.base_t + (self.frame - self.base_frame) as f64 * self.base_dt * self.speed;
    }

    /// 把当前 `global_t` 折叠成新基准（变速 / seek / dt 变化时调用）。
    fn rebase(&mut self, dt: f64) {
        self.base_t = self.global_t;
        self.base_frame = self.frame;
        self.base_dt = dt;
    }

    /// 直接置位（纯 seek，不累积当前状态）。
    pub fn seek(&mut self, t: f64) {
        self.global_t = t;
        self.base_t = t;
        self.base_frame = self.frame;
    }

    /// 回到起点：`t = 0`、帧号 = 0（REPLAY / Reset 用）。
    pub fn rewind(&mut self) {
        self.global_t = 0.0;
        self.frame = 0;
        self.base_t = 0.0;
        self.base_frame = 0;
        self.base_dt = 0.0;
    }

    pub fn set_speed(&mut self, s: f64) {
        // 先折叠：变速之前的 t 不能被新速度重算。
        self.base_t = self.global_t;
        self.base_frame = self.frame;
        self.speed = s;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn resume(&mut self) {
        self.playing = true;
    }

    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_accumulates() {
        let mut d = Director::new();
        d.advance(1.0);
        d.advance(0.5);
        assert!((d.global_t - 1.5).abs() < 1e-9);
    }

    #[test]
    fn seek_overrides() {
        let mut d = Director::new();
        d.advance(5.0);
        d.seek(1.0);
        assert!((d.global_t - 1.0).abs() < 1e-9);
        d.advance(1.0);
        assert!((d.global_t - 2.0).abs() < 1e-9);
    }

    #[test]
    fn pause_stops() {
        let mut d = Director::new();
        d.pause();
        d.advance(100.0);
        assert!((d.global_t - 0.0).abs() < 1e-9);
        d.resume();
        d.advance(1.0);
        assert!((d.global_t - 1.0).abs() < 1e-9);
    }

    #[test]
    fn negative_speed_plays_backwards() {
        let mut d = Director::new();
        d.seek(10.0);
        d.set_speed(-1.0);
        d.advance(3.0);
        assert!((d.global_t - 7.0).abs() < 1e-9);
    }

    #[test]
    fn serde_roundtrip() {
        let mut d = Director {
            global_t: std::f64::consts::PI,
            speed: 2.5,
            playing: false,
            ..Director::new()
        };
        d.advance(0.0);
        d.frame = 7;
        let json = serde_json::to_string(&d).unwrap();
        let restored: Director = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.global_t, d.global_t);
        assert_eq!(restored.speed, d.speed);
        assert_eq!(restored.playing, d.playing);
        assert_eq!(restored.frame, d.frame);
    }

    /// S3：`t` 由帧号派生 —— 固定 dt 下逐位等于 `frame × dt`，不靠累加。
    #[test]
    fn time_is_derived_from_frame() {
        let dt = 1.0 / 30.0;
        let mut d = Director::new();
        for _ in 0..300 {
            d.advance(dt);
        }
        assert_eq!(d.frame, 300);
        assert_eq!(d.global_t, 300.0 * dt);
    }

    /// 变速只折叠基准：之前的 t 不被新速度重算。
    #[test]
    fn speed_change_folds_previous_time() {
        let mut d = Director::new();
        for _ in 0..10 {
            d.advance(0.1);
        }
        assert!((d.global_t - 1.0).abs() < 1e-12);
        d.set_speed(2.0);
        for _ in 0..10 {
            d.advance(0.1);
        }
        assert!((d.global_t - 3.0).abs() < 1e-12);
        assert_eq!(d.frame, 20);
    }

    /// seek 是时间轴平移：帧号不变，后续推进从新基准继续。
    #[test]
    fn seek_keeps_frame_and_offsets_time() {
        let mut d = Director::new();
        for _ in 0..5 {
            d.advance(0.1);
        }
        d.seek(10.0);
        assert_eq!(d.frame, 5);
        d.advance(0.1);
        assert!((d.global_t - 10.1).abs() < 1e-12);
        assert_eq!(d.frame, 6);
    }

    /// 重播回到起点：`t = 0` 且帧号归零。
    #[test]
    fn rewind_resets_frame_and_time() {
        let mut d = Director::new();
        for _ in 0..3 {
            d.advance(0.5);
        }
        d.rewind();
        assert_eq!(d.frame, 0);
        assert_eq!(d.global_t, 0.0);
        d.advance(0.5);
        assert_eq!(d.frame, 1);
        assert_eq!(d.global_t, 0.5);
    }

    /// dt 变化（fps 切换）折基，不发生累加漂移。
    #[test]
    fn dt_change_rebases_without_drift() {
        let mut d = Director::new();
        d.advance(1.0 / 30.0);
        d.advance(1.0 / 60.0);
        assert!((d.global_t - (1.0 / 30.0 + 1.0 / 60.0)).abs() < 1e-12);
        assert_eq!(d.frame, 2);
    }
}
