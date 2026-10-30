use super::Measurement;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const BOUNDS_US: [u64; 23] = [
    0,
    10,
    25,
    50,
    100,
    250,
    500,
    1_000,
    2_000,
    4_000,
    8_000,
    12_000,
    16_000,
    24_000,
    32_000,
    50_000,
    75_000,
    100_000,
    250_000,
    500_000,
    1_000_000,
    5_000_000,
    u64::MAX,
];
const STAGES: [&str; 7] = [
    "send",
    "mailbox",
    "ownership",
    "pool",
    "work",
    "publish",
    "reply",
];
const EXAMPLES: usize = 4;
static NEXT: AtomicU64 = AtomicU64::new(1);
static LAST_INCIDENT_NS: AtomicU64 = AtomicU64::new(0);
static COALESCED_INCIDENTS: AtomicU64 = AtomicU64::new(0);

fn claim_incident(last: &AtomicU64, now: u64) -> bool {
    last.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |previous| {
        (previous == 0 || now.saturating_sub(previous) >= 1_000_000_000).then_some(now)
    })
    .is_ok()
}

thread_local! { static CURRENT: Cell<(&'static str, u64)> = const { Cell::new(("coordinator_tick", 0)) }; }
pub(crate) struct Context(
    (&'static str, u64),
    std::marker::PhantomData<std::rc::Rc<()>>,
);
impl Context {
    pub fn enter(operation: &'static str, id: u64) -> Self {
        Self(
            CURRENT.with(|c| c.replace((operation, id))),
            std::marker::PhantomData,
        )
    }
    pub fn current() -> (&'static str, u64) {
        CURRENT.with(Cell::get)
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        CURRENT.with(|c| c.set(self.0));
    }
}

pub(crate) fn micros(duration: Duration) -> u64 {
    duration
        .as_micros()
        .try_into()
        .expect("attribution duration exceeds u64")
}

#[derive(serde::Serialize)]
pub(crate) struct Execution {
    tid: i32,
    start_ns: u64,
    end_ns: u64,
}
impl Execution {
    #[cfg(target_os = "linux")]
    fn now() -> u64 {
        let mut ts = std::mem::MaybeUninit::<libc::timespec>::uninit();
        let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) };
        assert_eq!(
            result,
            0,
            "profiling monotonic clock failed: {}",
            std::io::Error::last_os_error()
        );
        let ts = unsafe { ts.assume_init() };
        u64::try_from(ts.tv_sec)
            .expect("negative monotonic seconds")
            .checked_mul(1_000_000_000)
            .and_then(|s| {
                s.checked_add(
                    ts.tv_nsec
                        .try_into()
                        .expect("negative monotonic nanoseconds"),
                )
            })
            .expect("profiling timestamp overflow")
    }
    pub(crate) fn start() -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            thread_local! { static TID: i32 = unsafe { libc::gettid() }; }
            Some(Self {
                tid: TID.with(|id| *id),
                start_ns: Self::now(),
                end_ns: 0,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
    pub(crate) fn finish(mut self) -> Self {
        #[cfg(target_os = "linux")]
        {
            self.end_ns = Self::now();
            assert!(self.end_ns >= self.start_ns, "profiling clock regressed");
        }
        self
    }
}

pub(crate) struct Sample {
    pub id: u64,
    pub operation: &'static str,
    pub kind: &'static str,
    pub tick: u64,
    pub stages: [Duration; 7],
    pub cpu: Option<Duration>,
    pub publish_cpu: Option<Duration>,
    pub phases: Vec<(&'static str, Measurement)>,
    pub blocker: Option<(&'static str, u64)>,
    pub execution: Option<Execution>,
}
impl Sample {
    pub fn new(operation: &'static str) -> Box<Self> {
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .expect("attribution ID exhausted");
        Box::new(Self {
            id,
            operation,
            kind: if matches!(operation, "tick" | "coordinator_tick") {
                "tick"
            } else {
                "job"
            },
            tick: 0,
            stages: [Duration::ZERO; 7],
            cpu: None,
            publish_cpu: None,
            phases: Vec::new(),
            blocker: None,
            execution: None,
        })
    }
    pub(crate) fn incident(&self, world: Option<i32>) {
        if let Some(execution) = &self.execution
            && self.total() > Duration::from_millis(50)
        {
            // Per-event storms can block CPU-worker completion and suppress unrelated journal diagnostics (#1250).
            if !claim_incident(&LAST_INCIDENT_NS, execution.end_ns) {
                COALESCED_INCIDENTS.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let coalesced_incidents = COALESCED_INCIDENTS.swap(0, Ordering::Relaxed);
            tracing::warn!(pid=std::process::id(), world, clock="CLOCK_MONOTONIC", coalesced_incidents,
                max_incidents_per_second=1, sample=%self.snapshot(), "latency incident");
        }
    }
    fn total(&self) -> Duration {
        self.stages.iter().sum()
    }
    fn snapshot(&self) -> Value {
        let phases: BTreeMap<_, _> = self
            .phases
            .iter()
            .map(|(name, m)| {
                (
                    *name,
                    json!({"wall_us":micros(m.wall), "cpu_us":m.cpu.map(micros)}),
                )
            })
            .collect();
        let stages: BTreeMap<_, _> = STAGES.into_iter().zip(self.stages.map(micros)).collect();
        json!({"id":self.id,"operation":self.operation,"kind":self.kind,"tick":self.tick,"stages_us":stages,"total_us":micros(self.total()),"work_cpu_us":self.cpu.map(micros),"publish_cpu_us":self.publish_cpu.map(micros),"phases":phases,"execution":self.execution,"execution_scope":"work_publish_reply","observed_owner":self.blocker.map(|(operation,id)|json!({"operation":operation,"id":id}))})
    }
}

#[derive(Default)]
struct Histogram {
    counts: [u64; BOUNDS_US.len()],
    sum: u64,
    maximum: u64,
}
impl Histogram {
    fn record(&mut self, value: Duration) {
        let us = micros(value);
        self.counts[BOUNDS_US.partition_point(|bound| *bound < us)] += 1;
        self.sum += us;
        self.maximum = self.maximum.max(us);
    }
    fn snapshot(&self) -> Value {
        let count: u64 = self.counts.iter().sum();
        let quantile = |percent: u64| {
            let mut seen = 0;
            let rank = (count * percent).div_ceil(100);
            self.counts.iter().enumerate().find_map(|(i, n)| {
                seen += n;
                (count > 0 && seen >= rank && i < BOUNDS_US.len() - 1).then_some(BOUNDS_US[i])
            })
        };
        json!({"counts":self.counts,"count":count,"sum_us":self.sum,"max_us":self.maximum,"p95_upper_us":quantile(95),"p99_upper_us":quantile(99)})
    }
}
#[derive(Default)]
struct Operation {
    stages: [Histogram; 7],
    total: Histogram,
    cpu: Histogram,
    publish_cpu: Histogram,
}

#[derive(Default)]
pub(crate) struct Attribution {
    started: Option<Instant>,
    // Operation labels come from compiled call-site types, never request data.
    operations: BTreeMap<(&'static str, &'static str), Operation>,
    examples: [Vec<(Duration, Box<Sample>)>; 3],
}
impl Attribution {
    #[cfg(test)]
    pub(crate) fn slowest(&self) -> impl Iterator<Item = &Sample> {
        self.examples
            .iter()
            .flatten()
            .map(|(_, sample)| sample.as_ref())
    }
    pub fn record(&mut self, sample: Box<Sample>) {
        let total = sample.total();
        self.started.get_or_insert_with(Instant::now);
        let operation = self
            .operations
            .entry((sample.kind, sample.operation))
            .or_default();
        for (histogram, value) in operation.stages.iter_mut().zip(sample.stages) {
            histogram.record(value);
        }
        operation.total.record(total);
        if let Some(cpu) = sample.cpu {
            operation.cpu.record(cpu);
        }
        if let Some(cpu) = sample.publish_cpu {
            operation.publish_cpu.record(cpu);
        }
        let index = match sample.kind {
            "tick" => 0,
            "job" => 1,
            "directory_loan" => 2,
            _ => panic!("unknown attribution kind"),
        };
        let examples = &mut self.examples[index];
        if examples.len() < EXAMPLES {
            examples.push((total, sample));
        } else {
            let (index, smallest) = examples
                .iter()
                .enumerate()
                .min_by_key(|(_, (elapsed, _))| *elapsed)
                .expect("examples empty");
            if total > smallest.0 {
                examples[index] = (total, sample);
            }
        }
    }
    pub fn emit(report: Value, world: Option<i32>) {
        let pid = std::process::id();
        let scope = if world.is_some() {
            "world"
        } else {
            "directory"
        };
        let window_seconds = report["window_seconds"]
            .as_f64()
            .expect("invalid attribution window");
        // One record per compiled operation avoids journal line truncation as callers grow.
        for operation in report["operations"]
            .as_array()
            .expect("invalid attribution operations")
        {
            tracing::info!(pid, scope, world, window_seconds, histogram_upper_bounds_us=%report["histogram_upper_bounds_us"],
                histogram=%operation, quantiles="upper bounds; noncumulative buckets; all completed operations", "latency histogram");
        }
        for kind in ["slowest_ticks", "slowest_jobs", "slowest_directory_loans"] {
            for sample in report[kind]
                .as_array()
                .expect("invalid attribution examples")
            {
                tracing::info!(pid, scope, world, window_seconds, sample=%sample, total_scope=%report["total_scope"],
                    phase_scope="inclusive; do not sum nested phases", owner_scope=%report["owner_scope"], "latency example");
            }
        }
    }
    pub fn report(&mut self) -> Option<Value> {
        let elapsed = self.started?.elapsed();
        if elapsed < Duration::from_secs(60) {
            return None;
        }
        let operations: Vec<_> = std::mem::take(&mut self.operations).into_iter().map(|((kind,name),o)| {
            let stages: BTreeMap<_,_> = STAGES.into_iter().zip(o.stages.iter().map(Histogram::snapshot)).collect();
            json!({"kind":kind,"operation":name,"stages":stages,"total":o.total.snapshot(),"work_cpu":o.cpu.snapshot(),"publish_cpu":o.publish_cpu.snapshot()})
        }).collect();
        let examples = std::mem::take(&mut self.examples).map(|samples| {
            samples
                .iter()
                .map(|(_, sample)| sample.snapshot())
                .collect::<Vec<_>>()
        });
        self.started = Some(Instant::now());
        Some(
            json!({"window_seconds":elapsed.as_secs_f64(),"histogram_upper_bounds_us":&BOUNDS_US[..],"histogram_counts":"noncumulative; every completed operation; last bucket is overflow","operations":operations,"slowest_ticks":examples[0],"slowest_jobs":examples[1],"slowest_directory_loans":examples[2],"example_limit_per_kind":EXAMPLES,"phase_scope":"inclusive; do not sum nested phases","total_scope":"sum of measured stages; excludes instrumentation gaps, caller scheduling/network and post-tick coordination; directory_loan work is ownership hold time, not CPU execution","owner_scope":"first Directory owner observed while acquisition blocked, not all intervening owners"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io, sync::Arc};

    #[test]
    fn incident_claim_is_atomic_and_handles_out_of_order_completion() {
        let last = AtomicU64::new(0);
        assert!(claim_incident(&last, 1));
        assert!(!claim_incident(&last, 1_000_000_000));
        assert!(claim_incident(&last, 1_000_000_001));
        assert!(!claim_incident(&last, 900_000_000));
        let claims = AtomicU64::new(0);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    if claim_incident(&last, 2_000_000_001) {
                        claims.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(claims.load(Ordering::Relaxed), 1);
    }

    struct Lines(Arc<AtomicU64>);
    impl io::Write for Lines {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.fetch_add(
                bytes.iter().filter(|&&byte| byte == b'\n').count() as u64,
                Ordering::Relaxed,
            );
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn incident_output_is_bounded_without_dropping_histograms_or_worst_examples() {
        let lines = Arc::new(AtomicU64::new(0));
        let output = lines.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || Lines(output.clone()))
            .finish();
        let mut attribution = Attribution::default();
        let started = Instant::now();
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..1_000 {
                let mut sample = Sample::new("incident_regression");
                sample.stages[1] = Duration::from_millis(100);
                sample.execution = Execution::start().map(Execution::finish);
                sample.incident(Some(1));
                attribution.record(sample);
            }
        });
        let elapsed = started.elapsed();
        let emitted = lines.load(Ordering::Relaxed);
        attribution.started = Some(Instant::now() - Duration::from_secs(60));
        let report = attribution.report().expect("completed attribution window");
        assert_eq!(report["operations"][0]["total"]["count"], 1_000);
        assert_eq!(report["operations"][0]["total"]["max_us"], 100_000);
        for stage in STAGES {
            assert_eq!(report["operations"][0]["stages"][stage]["count"], 1_000);
        }
        assert_eq!(report["slowest_jobs"].as_array().unwrap().len(), EXAMPLES);
        println!(
            "1000 slow operations: {emitted} incident lines in {:.3}s; all histograms and worst examples retained",
            elapsed.as_secs_f64()
        );
        assert!(
            emitted <= elapsed.as_secs() + 1,
            "incident output must not exceed one line per process per second"
        );
    }
}
