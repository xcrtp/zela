//! **输入流重放**（D 批）：把「第 N 帧的事件」装进 `InputEvents`。
//!
//! # 与 ZSB 的分工
//!
//! - ZSB 只说「场景 + 时间线」（纯函数，无输入）；
//! - 输入流说「第几帧发生了什么事」；
//! - 本模块只做一件小事：在 `Advance` **之前**替换本帧的 `InputEvents`。
//!
//! 因此同一份 ZSB + 同一份输入流，从任意起点重放都得到同一结果；
//! 零输入（[`InputStream::is_empty`]）就是 `Render(ZSB, ∅)`。
//!
//! [`InputStream::is_empty`]: zela_scene_bundle::InputStream::is_empty

use zela_scene_ecs::DriverPod;
use zela_scene_ecs::domain::{InputEvents, SceneEvent};

/// 在推进一帧**之前**调用：整帧替换 `InputEvents`（空帧也会清空上一帧）。
pub fn apply_input_frame(driver: &DriverPod, events: &[SceneEvent]) {
    driver.with_scheduler_mut(|scheduler| {
        scheduler
            .resources_mut()
            .insert(InputEvents::from_events(events.to_vec()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use zela_schedule::resource::Resources;
    use zela_schedule::scheduler::{Phase, Scheduler};
    use zela_scene_bundle::{InputStream, Pairing};
    use zela_scene_ecs::director_state::{DirectorAdvanceSystem, DirectorState};
    use zela_scene_ecs::domain::ids;
    use zela_scene_ecs::input::TimeControlSystem;
    use zela_scene_ecs::{Director, DriverCommand, TimeState};

    fn pairing() -> Pairing {
        Pairing {
            zsb_payload_hash: "unit-test".to_string(),
            libraries: Vec::new(),
        }
    }

    /// 逐帧快照：`(global_t, speed, playing, frame)` —— 浮点取位模式，**逐位**比较。
    fn snapshot(director: &Director) -> (u64, u64, bool, u64) {
        (
            director.global_t.to_bits(),
            director.speed.to_bits(),
            director.playing,
            director.frame,
        )
    }

    fn replay(stream: &InputStream, frames: u32, dt: f32) -> Vec<(u64, u64, bool, u64)> {
        let mut resources = Resources::new();
        resources.insert(TimeState::default());
        resources.insert(DirectorState::new());
        resources.insert(InputEvents::default());
        let mut scheduler = Scheduler::new(hecs::World::new(), resources);
        scheduler.add_system(Phase::Input, DirectorAdvanceSystem);
        scheduler.add_system(Phase::LogicUpdate, TimeControlSystem);
        let driver = DriverPod::new(scheduler);

        let mut out = Vec::new();
        for frame in 0..frames {
            apply_input_frame(&driver, stream.events_at(frame));
            driver.submit(DriverCommand::Advance { dt }).wait();
            let director = driver.with_scheduler(|scheduler| {
                scheduler
                    .resources()
                    .get::<DirectorState>()
                    .expect("用例注入了 DirectorState")
                    .director
                    .clone()
            });
            out.push(snapshot(&director));
        }
        out
    }

    /// **D 批验收**：同一 ZSB + 同一输入流重放两次，逐位相同；零输入是另一条轨迹。
    #[test]
    fn replaying_the_same_stream_twice_is_bit_identical() {
        let mut stream = InputStream::new(pairing());
        stream.record(1, [SceneEvent::Button { id: ids::PLAY_PAUSE }]);
        stream.record(3, [SceneEvent::Slider {
            id: ids::SPEED,
            value: 0.5,
        }]);
        stream.record(6, [SceneEvent::Button { id: ids::PLAY_PAUSE }]);

        let first = replay(&stream, 10, 1.0 / 30.0);
        let second = replay(&stream, 10, 1.0 / 30.0);
        assert_eq!(first, second, "同一输入流重放两次必须逐位相同");

        let empty = replay(&InputStream::new(pairing()), 10, 1.0 / 30.0);
        assert_ne!(first, empty, "事件必须真的生效，否则门禁是空转");
    }
}
