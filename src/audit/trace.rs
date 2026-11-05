use super::*;
use crate::wire::{Packet, Reader};
use std::sync::Arc;

const FACTS: usize = 32;
const BYTES: usize = 12 * 1024;
const SECOND_BYTES: usize = 32 * 1024;
const SECOND_FACTS: u32 = 128;

#[derive(Clone)]
pub struct Trace(Arc<Mutex<Buffer>>);
struct Buffer {
    session: u64,
    started: Instant,
    unix_ms: u128,
    identity: Option<[i32; 3]>,
    world: String,
    sequence: u64,
    covered: u64,
    from_sequence: u64,
    second: u64,
    admitted: u32,
    charged: usize,
    facts: Vec<Value>,
    bytes: usize,
    omitted: u64,
}
impl Default for Trace {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Buffer {
            session: rand::random(),
            started: Instant::now(),
            unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before epoch")
                .as_millis(),
            identity: None,
            world: String::new(),
            sequence: 0,
            covered: 0,
            from_sequence: 1,
            second: 0,
            admitted: 0,
            charged: 0,
            facts: Vec::new(),
            bytes: 0,
            omitted: 0,
        })))
    }
}
impl Trace {
    pub fn session(&self) -> u64 {
        self.0.lock().expect("packet trace poisoned").session
    }
    pub fn bind(&self, identity: [i32; 3], world: &str) {
        let mut b = self.0.lock().expect("packet trace poisoned");
        b.flush(false);
        b.identity = Some(identity);
        b.world = world.to_owned();
        b.from_sequence = b.sequence + 1;
        b.covered = b.sequence;
    }
    pub fn flush(&self, closing: bool) {
        self.0.lock().expect("packet trace poisoned").flush(closing);
    }
    pub fn stamp(&self) -> u128 {
        self.0
            .lock()
            .expect("packet trace poisoned")
            .started
            .elapsed()
            .as_micros()
    }
    pub fn event(&self, stage: &str, packet: Option<u8>, frame: Option<u64>, values: Value) -> u64 {
        self.0
            .lock()
            .expect("packet trace poisoned")
            .event(stage, packet, frame, || values)
    }
    pub fn received(&self, packet: &Packet, frame: u64, received_us: u128) -> u64 {
        self.0.lock().expect("packet trace poisoned").event("receipt",Some(packet.id),Some(frame),|| {
            let (payload,policy)=payload(packet,false);
            json!({"body_bytes":packet.body.len(),"received_us":received_us,"payload":payload,"payload_policy":policy})
        })
    }
    pub fn writing(&self, packet: &Packet, queued: usize) -> u64 {
        self.0.lock().expect("packet trace poisoned").event("encoding",Some(packet.id),None,|| {
            let (payload,policy)=payload(packet,true);
            json!({"body_bytes":packet.body.len(),"queued":queued,"payload":payload,"payload_policy":policy})
        })
    }
}
impl Buffer {
    fn event(
        &mut self,
        stage: &str,
        packet: Option<u8>,
        frame: Option<u64>,
        values: impl FnOnce() -> Value,
    ) -> u64 {
        self.sequence += 1;
        if self.identity.is_none() {
            return self.sequence;
        }
        let second = self.started.elapsed().as_secs();
        if second != self.second {
            self.second = second;
            self.admitted = 0;
            self.charged = 0;
        }
        if self.admitted == SECOND_FACTS || self.charged >= SECOND_BYTES || !trace_budget(1) {
            self.omitted += 1;
            self.covered = self.sequence;
            return self.sequence;
        }
        let fact = json!({"sequence":self.sequence,"at_us":self.started.elapsed().as_micros(),"stage":stage,"packet":packet,"frame":frame,"values":values()});
        let bytes = fact.to_string().len();
        if self.charged + bytes > SECOND_BYTES || !trace_budget(bytes - 1) {
            self.omitted += 1;
            self.covered = self.sequence;
            return self.sequence;
        }
        if self.bytes + bytes > BYTES {
            self.flush(false);
        }
        self.facts.push(fact);
        self.covered = self.sequence;
        self.bytes += bytes;
        self.charged += bytes;
        self.admitted += 1;
        if self.facts.len() == FACTS {
            self.flush(false);
        }
        self.sequence
    }
    fn flush(&mut self, closing: bool) {
        if self.identity.is_none() || self.facts.is_empty() && self.omitted == 0 {
            return;
        }
        record(
            "packet_trace",
            json!({"session":self.session,"started_unix_ms":self.unix_ms,"identity":self.identity,"world":self.world,"closing":closing,"from_sequence":self.from_sequence,"through_sequence":self.covered,"omitted":self.omitted,"facts":std::mem::take(&mut self.facts)}),
        );
        self.bytes = 0;
        self.omitted = 0;
        self.from_sequence = self.covered + 1;
    }
}
fn payload(packet: &Packet, outbound: bool) -> (Value, &'static str) {
    match numeric_payload(packet.id, &packet.body, outbound) {
        Ok(Some(v)) => (v, "numeric"),
        Ok(None) => (Value::Null, "not_selected"),
        Err(_) => (Value::Null, "invalid_layout"),
    }
}
pub fn policy() -> Value {
    json!({"connection_bytes_per_second":SECOND_BYTES,"connection_facts_per_second":SECOND_FACTS,"global_bytes_per_second":65536,"global_burst_bytes":131072,"batch_facts":FACTS,"batch_bytes":BYTES,"flush_ms":5000,"diagnostics_ms":30000,"inbound_payloads":[1,7,11,14,16,22,24,30,35,38,48,49,50,51,59,64,65,66,76],"outbound_payloads":[6,19,52],"encoding":"numeric_fields","enqueue_scope":"world_send","receipt_clock":"connection_monotonic","frame_boundary":"packet_completion"})
}
fn trace_budget(bytes: usize) -> bool {
    static BUDGET: OnceLock<Mutex<(Instant, usize)>> = OnceLock::new();
    let mut b = BUDGET
        .get_or_init(|| Mutex::new((Instant::now(), 128 * 1024)))
        .lock()
        .expect("trace budget poisoned");
    let seconds = b.0.elapsed().as_secs();
    if seconds > 0 {
        b.1 = (b.1 + seconds.min(2) as usize * 64 * 1024).min(128 * 1024);
        b.0 += std::time::Duration::from_secs(seconds);
    }
    if bytes > b.1 {
        return false;
    }
    b.1 -= bytes;
    true
}

// Mixed/string-bearing packets are never decoded, even when their body resembles a gameplay packet.
fn numeric_payload(id: u8, body: &[u8], outbound: bool) -> anyhow::Result<Option<Value>> {
    let mut r = Reader::new(body);
    let f = |r: &mut Reader<'_>| -> anyhow::Result<f32> {
        let v = r.f32()?;
        anyhow::ensure!(v.is_finite(), "non-finite trace coordinate");
        Ok(v)
    };
    let value = if outbound {
        match id {
            6 => json!([r.i32()?]),
            52 => json!([r.i32()?, f(&mut r)?, f(&mut r)?]),
            19 => {
                let mut v = vec![
                    json!(r.u8()?),
                    json!(r.i32()?),
                    json!(r.u8()?),
                    json!(f(&mut r)?),
                    json!(f(&mut r)?),
                    json!(f(&mut r)?),
                    json!(r.i16()?),
                ];
                if body.len() == 25 {
                    v.push(json!(r.u8()?));
                    v.push(json!(f(&mut r)?));
                }
                json!(v)
            }
            _ => return Ok(None),
        }
    } else {
        match id {
            7 => {
                let tick = r.i32()?;
                let time = r.i32()?;
                let x = f(&mut r)?;
                let y = f(&mut r)?;
                let n = r.i16()?;
                anyhow::ensure!((0..=128).contains(&n), "trace movement history bound");
                let mut history = Vec::new();
                for _ in 0..n {
                    history.push(json!([r.i32()?, f(&mut r)?, f(&mut r)?]));
                }
                json!([tick, time, x, y, history])
            }
            38 => json!([
                r.i32()?,
                r.u8()?,
                r.i16()?,
                f(&mut r)?,
                f(&mut r)?,
                f(&mut r)?
            ]),
            14 | 16 | 22 | 49 | 50 => json!([r.i32()?]),
            24 => json!([r.u8()?, r.i32()?]),
            59 | 64 => json!([r.i32()?, f(&mut r)?, f(&mut r)?]),
            51 => json!([r.i32()?, r.u8()?, r.i32()?]),
            66 => json!([r.i32()?, r.u8()?, r.i32()?, r.i32()?]),
            76 => json!([r.i32()?, r.u8()?, r.i32()?, r.u8()?]),
            1 | 11 | 48 => json!([]),
            35 => json!([r.i32()?, r.u8()?, r.i16()?]),
            30 => json!([
                r.i32()?,
                r.i32()?,
                r.u8()?,
                r.i16()?,
                f(&mut r)?,
                f(&mut r)?
            ]),
            65 => json!([
                r.i32()?,
                f(&mut r)?,
                f(&mut r)?,
                r.i32()?,
                r.u8()?,
                r.i16()?,
                r.i32()?,
                r.u8()?,
                r.i16()?
            ]),
            _ => return Ok(None),
        }
    };
    r.done()?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_preserves_move_histories_and_never_decodes_strings() {
        let mut w = crate::wire::Writer::new();
        w.i32(10)
            .i32(20)
            .f32(1.)
            .f32(2.)
            .i16(1)
            .i32(19)
            .f32(0.5)
            .f32(1.5);
        assert_eq!(
            numeric_payload(7, &w.0, false).unwrap().unwrap(),
            json!([10, 20, 1., 2., [[19, 0.5, 1.5]]])
        );
        for id in [0, 5, 8, 15, 17, 21, 25, 33, 39, 40, 45, 60, 69, 78] {
            for outgoing in [true, false] {
                assert!(
                    numeric_payload(id, b"PASSWORD CHAT RECOVERY SECRET", outgoing)
                        .unwrap()
                        .is_none()
                );
            }
        }
        assert!(numeric_payload(7, &[], false).is_err());
        let mut p = crate::wire::Writer::new();
        p.i32(1).f32(0.).f32(1.);
        p.0[4..8].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(numeric_payload(52, &p.0, true).is_err());
    }
    #[test]
    fn trace_is_authenticated_bounded_and_omissions_are_not_hidden() {
        let t = Trace::default();
        t.event("receipt", Some(17), Some(1), json!({}));
        assert!(t.0.lock().unwrap().facts.is_empty());
        t.bind([1, 2, 3], "Realm");
        for _ in 0..1000 {
            t.event("frame", None, Some(1), json!({"bytes":1,"packets":1}));
        }
        let mut b = t.0.lock().unwrap();
        assert!(b.facts.len() < FACTS);
        assert!(b.omitted > 0);
        assert!(b.charged <= SECOND_BYTES);
        assert!(b.admitted <= SECOND_FACTS);
        b.charged = SECOND_BYTES;
        b.event("receipt", Some(7), None, || {
            panic!("omitted payload builder must stay lazy")
        });
        b.flush(true);
        assert_eq!(b.from_sequence, b.covered + 1);
        assert!(b.facts.is_empty());
        assert_eq!(b.omitted, 0);
    }
}
