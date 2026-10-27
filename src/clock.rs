use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct Clock(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    paused: Option<Instant>,
    excluded: Duration,
}
impl Clock {
    pub fn now(&self) -> Instant {
        let state = self.0.lock().expect("game clock poisoned");
        state.paused.unwrap_or_else(Instant::now) - state.excluded
    }
    pub fn paused(&self) -> bool {
        self.0.lock().expect("game clock poisoned").paused.is_some()
    }
    pub fn set_paused(&self, paused: bool) {
        let mut state = self.0.lock().expect("game clock poisoned");
        match (paused, state.paused) {
            (true, None) => state.paused = Some(Instant::now()),
            (false, Some(at)) => {
                state.excluded += at.elapsed();
                state.paused = None;
            }
            _ => {}
        }
    }
}
