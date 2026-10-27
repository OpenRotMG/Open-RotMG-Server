use serde_json::{Value, json};
use std::time::{Duration, Instant};

const SECONDS: u64 = 5;
const BUCKETS: usize = 24;
const REASONS: [&str; 8] = [
    "blocked_tile",
    "speed",
    "server_time_budget",
    "collision_slide_budget",
    "immobilized",
    "clock",
    "shot_origin",
    "other",
];
#[derive(Clone, Copy, Default)]
struct Bucket {
    number: Option<u64>,
    acks: u64,
    repeated_ticks: u64,
    gaps: u64,
    gap_sum_us: u64,
    max_gap_us: u64,
    max_gap_at: Option<Instant>,
    over_200ms: u64,
    over_500ms: u64,
    under_50ms: u64,
    corrections: [u64; 8],
}
pub(super) struct Window {
    started: Instant,
    last_ack: Option<(i32, Instant)>,
    buckets: [Bucket; BUCKETS],
}
impl Default for Window {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            last_ack: None,
            buckets: [Bucket::default(); BUCKETS],
        }
    }
}
impl Window {
    fn bucket(&mut self, now: Instant) -> &mut Bucket {
        let number = now.duration_since(self.started).as_secs() / SECONDS;
        let b = &mut self.buckets[(number % BUCKETS as u64) as usize];
        if b.number != Some(number) {
            *b = Bucket {
                number: Some(number),
                ..Bucket::default()
            };
        }
        b
    }
    pub fn acknowledge(&mut self, tick: i32, received: Instant) {
        self.acknowledge_at(tick, received, Instant::now());
    }
    fn acknowledge_at(&mut self, tick: i32, received: Instant, now: Instant) {
        let previous = self.last_ack;
        let b = self.bucket(now);
        b.acks += 1;
        if let Some((last_tick, last_received)) = previous {
            if tick <= last_tick {
                b.repeated_ticks += 1;
                return;
            }
            let us = u64::try_from(received.duration_since(last_received).as_micros())
                .expect("ACK gap exceeds u64 microseconds");
            b.gaps += 1;
            b.gap_sum_us += us;
            b.over_200ms += u64::from(us > 200_000);
            b.over_500ms += u64::from(us > 500_000);
            b.under_50ms += u64::from(us < 50_000);
            if b.max_gap_at.is_none() || us > b.max_gap_us {
                b.max_gap_us = us;
                b.max_gap_at = Some(received);
            }
        }
        self.last_ack = Some((tick, received));
    }
    pub fn correct(&mut self, reason: &str) {
        let index = match reason.split(':').next().expect("split has first field") {
            "movement across a blocked tile" => 0,
            "movement exceeds speed" => 1,
            "movement exceeds server time budget" => 2,
            "collision slide exceeds movement budget" => 3,
            "immobilized movement" => 4,
            "movement time reversed" | "negative movement time" | "movement time jump" => 5,
            "invalid shot origin" => 6,
            _ => 7,
        };
        self.bucket(Instant::now()).corrections[index] += 1;
    }
    pub fn snapshot(&self, now: Instant, history: bool) -> Value {
        let number = now.duration_since(self.started).as_secs() / SECONDS;
        let first = number.saturating_sub(BUCKETS as u64 - 1);
        let covered_ms = now
            .duration_since(self.started + Duration::from_secs(first * SECONDS))
            .as_secs_f64()
            * 1000.;
        let mut total = Bucket::default();
        let mut rows = Vec::new();
        for b in self
            .buckets
            .iter()
            .filter(|b| b.number.is_some_and(|n| n >= first && n <= number))
        {
            total.acks += b.acks;
            total.repeated_ticks += b.repeated_ticks;
            total.gaps += b.gaps;
            total.gap_sum_us += b.gap_sum_us;
            total.over_200ms += b.over_200ms;
            total.over_500ms += b.over_500ms;
            total.under_50ms += b.under_50ms;
            if b.max_gap_at.is_some()
                && (total.max_gap_at.is_none() || b.max_gap_us > total.max_gap_us)
            {
                total.max_gap_us = b.max_gap_us;
                total.max_gap_at = b.max_gap_at;
            }
            for (out, count) in total.corrections.iter_mut().zip(b.corrections) {
                *out += count;
            }
            if history {
                let mut row = b.summary(now);
                let start = self.started
                    + Duration::from_secs(b.number.expect("selected bucket") * SECONDS);
                row["start_age_ms"] = json!(now.duration_since(start).as_secs_f64() * 1000.);
                row["end_age_ms"] = json!(
                    now.saturating_duration_since(start + Duration::from_secs(SECONDS))
                        .as_secs_f64()
                        * 1000.
                );
                rows.push(row);
            }
        }
        rows.sort_by(|a, b| {
            b["start_age_ms"]
                .as_f64()
                .unwrap()
                .total_cmp(&a["start_age_ms"].as_f64().unwrap())
        });
        let mut out = total.summary(now);
        out["covered_ms"] = json!(covered_ms);
        out["corrections_per_minute"] = json!(
            (covered_ms > 0.)
                .then(|| total.corrections.iter().sum::<u64>() as f64 * 60_000. / covered_ms)
        );
        out["waiting_for_first_ack"] = json!(self.last_ack.is_none());
        if history {
            out["history"] = json!(rows);
        }
        out
    }
}
impl Bucket {
    fn summary(&self, now: Instant) -> Value {
        json!({"acks":self.acks,"repeated_ticks":self.repeated_ticks,"gap_samples":self.gaps,
            "mean_gap_ms":(self.gaps > 0).then(||self.gap_sum_us as f64 / self.gaps as f64 / 1000.),
            "max_gap_ms":self.max_gap_at.map(|_|self.max_gap_us as f64 / 1000.),
            "max_gap_end_age_ms":self.max_gap_at.map(|t|now.duration_since(t).as_secs_f64()*1000.),
            "over_200ms":self.over_200ms,"over_500ms":self.over_500ms,"under_50ms":self.under_50ms,
            "corrections":REASONS.iter().zip(self.corrections).collect::<std::collections::BTreeMap<_,_>>()})
    }
}

pub(crate) fn public_snapshot(value: &Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    let fields = |v: &Value| {
        let mut out = json!({"corrections":REASONS.iter().map(|r|(*r,v["corrections"][*r].as_u64())).collect::<std::collections::BTreeMap<_,_>>()});
        for key in [
            "acks",
            "repeated_ticks",
            "gap_samples",
            "over_200ms",
            "over_500ms",
            "under_50ms",
        ] {
            out[key] = json!(v[key].as_u64());
        }
        for key in ["mean_gap_ms", "max_gap_ms", "max_gap_end_age_ms"] {
            out[key] = json!(v[key].as_f64());
        }
        out
    };
    let mut out = fields(value);
    for key in ["covered_ms", "corrections_per_minute"] {
        out[key] = json!(value[key].as_f64());
    }
    out["waiting_for_first_ack"] = json!(value["waiting_for_first_ack"].as_bool());
    out["history"] = value["history"]
        .as_array()
        .map(|a| {
            a.iter()
                .take(BUCKETS)
                .map(|v| {
                    let mut row = fields(v);
                    for key in ["start_age_ms", "end_age_ms"] {
                        row[key] = json!(v[key].as_f64());
                    }
                    row
                })
                .collect::<Vec<_>>()
        })
        .into();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_ack_gaps_repeats_expiry_and_first_tick() {
        let mut w = Window::default();
        let t = w.started;
        assert!(w.snapshot(t, true)["max_gap_ms"].is_null());
        for (tick, ms) in [(0, 2000), (1, 2100), (2, 2800), (2, 2801), (3, 2810)] {
            let at = t + Duration::from_millis(ms);
            w.acknowledge_at(tick, at, at);
        }
        let s = w.snapshot(t + Duration::from_secs(3), true);
        assert_eq!(s["acks"], 5);
        assert_eq!(s["gap_samples"], 3);
        assert_eq!(s["repeated_ticks"], 1);
        assert_eq!(s["max_gap_ms"], 700.);
        assert_eq!(s["over_500ms"], 1);
        assert_eq!(s["under_50ms"], 1);
        assert_eq!(w.snapshot(t + Duration::from_secs(120), true)["acks"], 0);
        for n in 1..2000 {
            let at = t + Duration::from_secs(n);
            w.acknowledge_at(
                n as i32 + 3,
                at + Duration::from_secs(3),
                at + Duration::from_secs(3),
            );
        }
        assert!(
            w.snapshot(t + Duration::from_secs(2003), true)["history"]
                .as_array()
                .unwrap()
                .len()
                <= BUCKETS
        );
    }
    #[test]
    fn correction_counts_and_public_allowlist() {
        let mut w = Window::default();
        w.correct("movement exceeds speed: private coordinates");
        w.correct("movement across a blocked tile");
        w.correct("a new rejection reason");
        let mut s = w.snapshot(Instant::now(), true);
        s["private"] = json!("SECRET");
        s["history"][0]["private"] = json!("SECRET");
        s["corrections"]["private"] = json!("SECRET");
        let s = public_snapshot(&s);
        assert_eq!(s["corrections"]["speed"], 1);
        assert_eq!(s["corrections"]["blocked_tile"], 1);
        assert_eq!(s["corrections"]["other"], 1);
        assert!(!s.to_string().contains("SECRET"));
        assert!(public_snapshot(&Value::Null).is_null());
    }
}
