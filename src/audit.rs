use serde_json::{Value, json};
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
    dropped: u64,
}
impl Stream {
    fn admit(&mut self, second: u64) -> bool {
        self.sequence += 1;
        if second != self.second {
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

pub fn record(kind: &str, data: Value) {
    static STREAM: OnceLock<Mutex<Stream>> = OnceLock::new();
    let mut stream = STREAM
        .get_or_init(|| {
            Mutex::new(Stream {
                run: format!("{:032x}", rand::random::<u128>()),
                sequence: 0,
                started: Instant::now(),
                second: 0,
                emitted: 0,
                dropped: 0,
            })
        })
        .lock()
        .expect("audit stream poisoned");
    let second = stream.started.elapsed().as_secs();
    if !stream.admit(second) {
        return;
    }
    let record = json!({"schema":1,"run":stream.run,"sequence":stream.sequence,"producer_dropped":stream.dropped,
        "pid":std::process::id(),"unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before epoch").as_millis(),
        "elapsed_us":stream.started.elapsed().as_micros(),"kind":kind,"data":data});
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
    fn producer_budget_leaves_an_explicit_sequence_gap() {
        let mut stream = Stream {
            run: "test".into(),
            sequence: 0,
            started: Instant::now(),
            second: 0,
            emitted: 0,
            dropped: 0,
        };
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
