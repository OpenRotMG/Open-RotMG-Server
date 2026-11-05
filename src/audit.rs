use serde_json::{Value, json};

#[derive(Default)]
pub struct MotionWindow {
    pub epoch: u64,
    samples: Vec<Value>,
    omitted: u64,
}
impl MotionWindow {
    pub fn observe(&mut self, sample: impl FnOnce() -> Value) {
        if self.samples.len() == 64 {
            self.omitted += 1;
        } else {
            self.samples.push(sample());
        }
    }
    pub fn boundary(&mut self) {
        self.epoch = self.epoch.checked_add(1).expect("motion epoch exhausted");
    }
    pub fn flush(&mut self, account: i32, character: i32, player: i32, world: &str, closing: bool) {
        if self.samples.is_empty() && self.omitted == 0 {
            return;
        }
        record(
            "motion",
            json!({"account":account,"character":character,"player":player,"world":world,
            "closing":closing,"omitted":self.omitted,"samples":std::mem::take(&mut self.samples)}),
        );
        self.omitted = 0;
    }
}
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Stream {
    run: String,
    sequence: u64,
    started: Instant,
    second: u64,
    emitted: u32,
    bytes_left: usize,
    dropped: u64,
}
impl Stream {
    fn new() -> Self {
        Self {
            run: format!("{:032x}", rand::random::<u128>()),
            sequence: 0,
            started: Instant::now(),
            second: 0,
            emitted: 0,
            bytes_left: 1024 * 1024,
            dropped: 0,
        }
    }
    fn admit_payload(&mut self, bytes: usize) -> bool {
        if bytes > 32 * 1024 || bytes > self.bytes_left {
            self.dropped += 1;
            return false;
        }
        self.bytes_left -= bytes;
        true
    }
    fn admit(&mut self, second: u64) -> bool {
        self.sequence += 1;
        if second != self.second {
            // A world's movement windows flush together, so reserve a bounded burst.
            let refill = (second - self.second).min(4) as usize * 256 * 1024;
            self.bytes_left = (self.bytes_left + refill).min(1024 * 1024);
            self.second = second;
            self.emitted = 0;
        }
        if self.emitted == 200 {
            self.dropped += 1;
            return false;
        }
        self.emitted += 1;
        true
    }
}

#[derive(Default)]
struct CombatBuffer {
    started: Option<Instant>,
    bytes: usize,
    facts: Vec<Value>,
}
impl CombatBuffer {
    fn flush(&mut self) {
        if self.facts.is_empty() {
            return;
        }
        emit(
            "combat_batch",
            json!({"facts":std::mem::take(&mut self.facts)}),
        );
        self.started = None;
        self.bytes = 0;
    }
    fn push(&mut self, data: Value) {
        let bytes = data.to_string().len();
        if self.bytes + bytes > 16 * 1024 {
            self.flush();
        }
        if self.facts.is_empty() {
            self.started = Some(Instant::now());
        }
        self.facts.push(data);
        self.bytes += bytes;
        if self.facts.len() == 32 || self.bytes >= 16 * 1024 {
            self.flush();
        }
    }
}
static COMBAT: OnceLock<Mutex<CombatBuffer>> = OnceLock::new();
pub fn combat(data: Value) {
    COMBAT
        .get_or_init(|| Mutex::new(CombatBuffer::default()))
        .lock()
        .expect("combat audit poisoned")
        .push(data);
}
pub fn flush_combat(force: bool) {
    if let Some(buffer) = COMBAT.get() {
        let mut buffer = buffer.lock().expect("combat audit poisoned");
        if force
            || buffer
                .started
                .is_some_and(|at| at.elapsed().as_millis() >= 100)
        {
            buffer.flush();
        }
    }
}
pub fn record(kind: &str, data: Value) {
    flush_combat(false);
    emit(kind, data);
}
fn emit(kind: &str, data: Value) {
    static STREAM: OnceLock<Mutex<Stream>> = OnceLock::new();
    let mut stream = STREAM
        .get_or_init(|| Mutex::new(Stream::new()))
        .lock()
        .expect("audit stream poisoned");
    let second = stream.started.elapsed().as_secs();
    if !stream.admit(second) {
        return;
    }
    let record = json!({"schema":2,"run":stream.run,"sequence":stream.sequence,"producer_dropped":stream.dropped,
        "pid":std::process::id(),"unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before epoch").as_millis(),
        "elapsed_us":stream.started.elapsed().as_micros(),"kind":kind,"data":data}).to_string();
    if !stream.admit_payload(record.len()) {
        return;
    }
    // Sequence assignment and emission share a lock so journal loss is distinguishable from producer reordering.
    tracing::info!(target: "openrotmg::audit", record=%record, "game audit fact");
}

#[derive(Default)]
struct Packets {
    count: u64,
    bytes: usize,
    min_bytes: Option<usize>,
    max_bytes: usize,
    min_gap_us: Option<u128>,
    max_gap_us: Option<u128>,
    gap_count: u64,
    gap_sum_us: u128,
    gap_squared_sum_us: u128,
    zero_gaps: u64,
    regressed_receipts: u64,
    last: Option<Instant>,
}
impl Packets {
    fn observe(&mut self, bytes: usize, received: Instant) {
        self.count += 1;
        self.bytes += bytes;
        self.min_bytes = Some(self.min_bytes.map_or(bytes, |n| n.min(bytes)));
        self.max_bytes = self.max_bytes.max(bytes);
        if let Some(previous) = self.last {
            if let Some(gap) = received.checked_duration_since(previous) {
                let gap = gap.as_micros();
                self.gap_count += 1;
                self.gap_sum_us += gap;
                self.gap_squared_sum_us += gap * gap;
                self.zero_gaps += u64::from(gap == 0);
                self.min_gap_us = Some(self.min_gap_us.map_or(gap, |n| n.min(gap)));
                self.max_gap_us = Some(self.max_gap_us.map_or(gap, |n| n.max(gap)));
            } else {
                self.regressed_receipts += 1;
            }
        }
        self.last = Some(received);
    }
    fn snapshot(&self, packet: u8) -> Value {
        json!({"packet":packet,"count":self.count,"body_bytes":self.bytes,
            "min_body_bytes":self.min_bytes,"max_body_bytes":self.max_bytes,
            "min_receipt_gap_us":self.min_gap_us,"max_receipt_gap_us":self.max_gap_us,
            "receipt_gap_count":self.gap_count,"receipt_gap_sum_us":self.gap_sum_us,
            "receipt_gap_squared_sum_us":self.gap_squared_sum_us,"zero_receipt_gaps":self.zero_gaps,
            "regressed_receipts":self.regressed_receipts})
    }
}

pub struct Window {
    id: u64,
    since: Instant,
    packets: BTreeMap<u8, Packets>,
}
impl Window {
    pub fn new() -> Self {
        Self {
            id: rand::random(),
            since: Instant::now(),
            packets: BTreeMap::new(),
        }
    }
    pub fn observe(&mut self, packet: &crate::wire::Packet, received: Instant) {
        // Counts and lengths include authentication packets, never their bodies or strings.
        self.packets
            .entry(packet.id)
            .or_default()
            .observe(packet.body.len(), received);
    }
    pub fn flush(
        &mut self,
        account: Option<i32>,
        player: Option<i32>,
        world: Option<i32>,
        closing: bool,
    ) {
        record(
            "input_window",
            json!({"session":self.id,"account":account,"player":player,"world":world,
            "covered_us":self.since.elapsed().as_micros(),"closing":closing,
            "packets":self.packets.iter().map(|(id,p)|p.snapshot(*id)).collect::<Vec<_>>()}),
        );
        self.packets.clear();
        self.since = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_windows_bound_memory_and_preserve_correction_epochs() {
        let mut window = MotionWindow::default();
        let built = std::cell::Cell::new(0);
        for _ in 0..80 {
            window.observe(|| {
                built.set(built.get() + 1);
                json!({"epoch":0})
            });
        }
        assert_eq!(built.get(), 64);
        assert_eq!((window.samples.len(), window.omitted), (64, 16));
        window.boundary();
        assert_eq!(window.samples[0]["epoch"], 0);
        window.flush(1, 2, 3, "Nexus", false);
        assert!(window.samples.is_empty());
        assert_eq!((window.omitted, window.epoch), (0, 1));
        window.observe(|| json!({"epoch":1}));
        assert_eq!(window.samples[0]["epoch"], 1);
    }

    #[test]
    fn producer_budget_leaves_an_explicit_sequence_gap() {
        let mut stream = Stream::new();
        for _ in 0..200 {
            assert!(stream.admit(0));
        }
        assert!(!stream.admit(0));
        assert!(stream.admit(1));
        assert_eq!(
            (stream.sequence, stream.dropped, stream.emitted),
            (202, 1, 1)
        );
    }

    #[test]
    fn byte_budget_and_record_limit_drop_explicitly_and_reset_each_second() {
        let mut stream = Stream::new();
        assert!(stream.admit(0));
        assert!(!stream.admit_payload(32 * 1024 + 1));
        for _ in 0..32 {
            assert!(stream.admit(0));
            assert!(stream.admit_payload(32 * 1024));
        }
        assert!(stream.admit(0));
        assert!(!stream.admit_payload(1));
        assert_eq!(stream.dropped, 2);
        assert!(stream.admit(1));
        assert!(stream.admit_payload(1));
        assert_eq!(stream.bytes_left, 256 * 1024 - 1);
        assert!(stream.admit(10));
        assert_eq!(stream.bytes_left, 1024 * 1024);
    }

    #[test]
    fn combat_buffers_bound_count_and_bytes_without_discarding_fact_values() {
        let mut buffer = CombatBuffer::default();
        for _ in 0..31 {
            buffer.push(json!({"world_ms":0}));
        }
        assert_eq!(buffer.facts.len(), 31);
        buffer.push(json!({"world_ms":1}));
        assert!(buffer.facts.is_empty());
        buffer.push(json!({"values":[1,2,3]}));
        assert_eq!(buffer.facts[0]["values"], json!([1, 2, 3]));
        for _ in 0..10 {
            buffer.push(json!({"values":vec![1;1200]}));
            assert!(buffer.bytes < 16 * 1024);
        }
        buffer.flush();
        assert_eq!(buffer.bytes, 0);
        assert!(buffer.started.is_none());
    }

    #[test]
    fn packet_windows_measure_receipts_without_retaining_payloads() {
        let at = Instant::now();
        let mut window = Window::new();
        let packet = crate::wire::Packet {
            id: 17,
            body: b"PASSWORD-NEVER-RETAIN".to_vec(),
        };
        window.observe(&packet, at);
        window.observe(&packet, at + std::time::Duration::from_millis(7));
        let sample = window.packets[&17].snapshot(17);
        assert_eq!(sample["count"], 2);
        assert_eq!(sample["min_receipt_gap_us"], 7000);
        assert_eq!(sample["max_receipt_gap_us"], 7000);
        assert_eq!(sample["receipt_gap_sum_us"], 7000);
        assert_eq!(sample["receipt_gap_squared_sum_us"], 49_000_000);
        assert!(!sample.to_string().contains("PASSWORD"));
        window.observe(&packet, at);
        assert_eq!(window.packets[&17].regressed_receipts, 1);
    }
}
