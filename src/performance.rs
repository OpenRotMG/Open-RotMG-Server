use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

const RETAINED: usize = 256;

#[derive(Default)]
struct Samples {
    recent: VecDeque<u64>,
    count: u64,
    total: u64,
    maximum: u64,
    over_budget: u64,
}
impl Samples {
    fn record(&mut self, us: u64) {
        self.count += 1;
        self.total += us;
        self.maximum = self.maximum.max(us);
        self.over_budget += u64::from(us > 50_000);
        if self.recent.len() == RETAINED {
            self.recent.pop_front();
        }
        self.recent.push_back(us);
    }
    fn snapshot(&self) -> Value {
        let mut sorted: Vec<_> = self.recent.iter().copied().collect();
        sorted.sort_unstable();
        let percentile =
            |p: f64| sorted[((sorted.len() as f64 * p).ceil() as usize).saturating_sub(1)];
        json!({"count":self.count,"retained_samples":sorted.len(),"mean_us":self.total/self.count,
            "p50_us":percentile(0.5),"p95_us":percentile(0.95),"p99_us":percentile(0.99),
            "max_us":self.maximum,"over_50ms":self.over_budget})
    }
}

#[derive(Default)]
pub struct Window {
    started: Option<Instant>,
    samples: BTreeMap<&'static str, Samples>,
}
impl Window {
    pub fn record(&mut self, name: &'static str, elapsed: Duration) {
        self.started.get_or_insert_with(|| Instant::now() - elapsed);
        self.samples
            .entry(name)
            .or_default()
            .record(elapsed.as_micros().try_into().expect("timing exceeds u64"));
    }
    pub fn report(&mut self) -> Option<Value> {
        let elapsed = self.started?.elapsed();
        if elapsed < Duration::from_secs(5) {
            return None;
        }
        let achieved_step_hz = self
            .samples
            .get("step")
            .map(|s| s.count as f64 / elapsed.as_secs_f64());
        let missed_tick_deadlines = self
            .samples
            .get("scheduling_lateness")
            .map(|s| s.over_budget);
        let timings: BTreeMap<_, _> = std::mem::take(&mut self.samples)
            .into_iter()
            .map(|(name, s)| (name, s.snapshot()))
            .collect();
        self.started = Some(Instant::now());
        Some(
            json!({"pid":std::process::id(),"window_seconds":elapsed.as_secs_f64(),"achieved_step_hz":achieved_step_hz,"missed_tick_deadlines":missed_tick_deadlines,
            "percentiles":"latest 256 samples; count/max cover full window","timings":timings}),
        )
    }
}

pub fn memory() -> Value {
    fn read() -> anyhow::Result<Value> {
        let status = std::fs::read_to_string("/proc/self/status")?;
        let field = |name: &str| -> anyhow::Result<u64> {
            let line = status
                .lines()
                .find(|l| l.starts_with(name))
                .ok_or_else(|| anyhow::anyhow!("missing {name}"))?;
            Ok(line
                .split_whitespace()
                .nth(1)
                .ok_or_else(|| anyhow::anyhow!("invalid {name}"))?
                .parse()?)
        };
        Ok(json!({"resident_kib":field("VmRSS:")?,"high_water_kib":field("VmHWM:")?}))
    }
    match read() {
        Ok(value) => value,
        Err(error) => json!({"unavailable":error.to_string()}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_storage_is_bounded_while_totals_and_tail_are_retained() {
        let mut samples = Samples::default();
        for _ in 0..1000 {
            samples.record(1000);
        }
        samples.record(100_000);
        let value = samples.snapshot();
        assert_eq!(value["count"], 1001);
        assert_eq!(value["retained_samples"], 256);
        assert_eq!(value["max_us"], 100_000);
        assert_eq!(value["over_50ms"], 1);
        let mut window = Window {
            started: Some(Instant::now() - Duration::from_secs(6)),
            ..Window::default()
        };
        window.record("step", Duration::from_millis(7));
        window.record("scheduling_lateness", Duration::from_millis(60));
        let report = window.report().unwrap();
        assert_eq!(report["timings"]["step"]["count"], 1);
        assert_eq!(
            report["achieved_step_hz"].as_f64().unwrap(),
            1. / report["window_seconds"].as_f64().unwrap()
        );
        assert_eq!(report["missed_tick_deadlines"], 1);
        assert!(window.report().is_none());
        window.started = Some(window.started.unwrap() - Duration::from_secs(6));
        assert_eq!(window.report().unwrap()["timings"], json!({}));
    }
}
