//! Optional autosave scheduling. The native host owns eligibility, capture and storage.
use cyrus_mod_sdk::{Cx, Mod, Persist, SaveStatus};

#[derive(Default)]
struct Timer {
    elapsed: f32,
    waiting: bool,
    notice: f32,
    failed: bool,
}
impl Persist for Timer {
    fn pack(&self) -> Vec<f32> {
        vec![
            self.elapsed,
            f32::from(u8::from(self.waiting)),
            self.notice,
            f32::from(u8::from(self.failed)),
        ]
    }
    fn unpack(values: &[f32]) -> Option<Self> {
        let &[elapsed, waiting, notice, failed] = values else {
            return None;
        };
        if !elapsed.is_finite()
            || elapsed < 0.0
            || !notice.is_finite()
            || notice < 0.0
            || ![0.0, 1.0].contains(&waiting)
            || ![0.0, 1.0].contains(&failed)
        {
            return None;
        }
        Some(Self {
            elapsed,
            waiting: waiting == 1.0,
            notice,
            failed: failed == 1.0,
        })
    }
}
impl Timer {
    fn tick(&mut self, delta: f32, ready: bool, status: SaveStatus, interval: f32) -> bool {
        self.notice = (self.notice - delta).max(0.0);
        if self.waiting {
            match status {
                SaveStatus::Saved | SaveStatus::Failed => {
                    self.failed = status == SaveStatus::Failed;
                    self.elapsed = 0.0;
                    self.waiting = false;
                    self.notice = if self.failed { 6.0 } else { 3.0 };
                }
                SaveStatus::Idle => self.waiting = false,
                SaveStatus::Pending => {}
            }
            return false;
        }
        if ready {
            self.elapsed += delta;
        }
        ready && self.elapsed >= if self.failed { 30.0 } else { interval }
    }
}

struct Autosave;
impl Mod for Autosave {
    const ID: &'static str = "autosave";
    fn frame(cx: &Cx) {
        let mut timer = cx.memory.load::<Timer>("timer").unwrap_or_default();
        let delta = cx.input.delta();
        let delta = if delta.is_finite() {
            delta.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let interval = cx.settings.number("interval").clamp(60.0, 1800.0);
        if timer.tick(delta, cx.saves.ready(), cx.saves.status(), interval) && cx.saves.save() {
            timer.waiting = true;
        }
        if timer.notice > 0.0 && cx.settings.flag("notifications") {
            if timer.failed {
                cx.hud.show(&format!(
                    "Autosave failed: {}. Retrying after 30 seconds of eligible play.",
                    cx.saves.error()
                ));
            } else {
                cx.hud.show("Autosaved");
            }
        }
        cx.memory.save("timer", Some(&timer));
    }
}
cyrus_mod_sdk::export_mod!(Autosave);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_play_defers_a_due_save_and_pending_waits_for_storage() {
        let mut timer = Timer {
            elapsed: 299.0,
            ..Default::default()
        };
        assert!(!timer.tick(1.0, false, SaveStatus::Idle, 300.0));
        assert!(timer.tick(1.0, true, SaveStatus::Idle, 300.0));
        timer.waiting = true;
        assert!(!timer.tick(1.0, true, SaveStatus::Pending, 300.0));
        assert_eq!(timer.notice, 0.0);
        assert!(!timer.tick(1.0, true, SaveStatus::Saved, 300.0));
        assert_eq!(timer.elapsed, 0.0);
        assert_eq!(timer.notice, 3.0);
    }
    #[test]
    fn failure_retries_after_thirty_eligible_seconds_and_corrupt_memory_is_rejected() {
        let mut timer = Timer {
            waiting: true,
            ..Default::default()
        };
        timer.tick(1.0, true, SaveStatus::Failed, 300.0);
        assert!(timer.failed);
        assert!(!timer.tick(30.0, false, SaveStatus::Failed, 300.0));
        assert!(timer.tick(30.0, true, SaveStatus::Failed, 300.0));
        let restored = Timer::unpack(&timer.pack()).unwrap();
        assert_eq!(restored.elapsed, 30.0);
        assert!(restored.failed);
        assert!(Timer::unpack(&[f32::NAN, 0.0, 0.0, 0.0]).is_none());
    }
}
