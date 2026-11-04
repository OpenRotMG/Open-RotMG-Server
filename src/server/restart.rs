use super::hiscores::{json_error, private_denial};
use super::*;
use serde::Serialize;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NOTICE: Duration = Duration::from_secs(90);

pub(crate) struct Service {
    instance: String,
    current: Option<Countdown>,
}
impl Default for Service {
    fn default() -> Self {
        use rand::RngCore;
        let mut bytes = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self {
            instance: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            current: None,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub(crate) instance: String,
    pub(crate) owner: String,
    pub(crate) deadline_ms: u64,
}
#[derive(Debug)]
pub(crate) struct ConsumedUpdate {
    pub(crate) operation: Receipt,
}
#[derive(Clone, Debug, Serialize)]
struct Status {
    receipt: Receipt,
    state: &'static str,
}
struct Countdown {
    status: Status,
    deadline: Instant,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Begin { owner: String },
    Status,
    Cancel { receipt: Receipt, actor_id: String },
    Consume { receipt: Receipt },
}
impl Service {
    fn begin(&mut self, owner: String, now: Instant) -> Result<Status> {
        ensure!(
            owner.len() == 64
                && owner
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid restart owner"
        );
        if let Some(current) = &self.current {
            ensure!(
                current.status.receipt.owner != owner,
                "restart owner already used"
            );
            ensure!(
                current.status.state != "consumed",
                "restart already consumed"
            );
            ensure!(
                current.status.state == "cancelled" || now >= current.deadline,
                "restart already pending"
            );
        }
        let status = Status {
            receipt: Receipt {
                instance: self.instance.clone(),
                owner,
                deadline_ms: (SystemTime::now().duration_since(UNIX_EPOCH)? + NOTICE)
                    .as_millis()
                    .try_into()?,
            },
            state: "pending",
        };
        self.current = Some(Countdown {
            status: status.clone(),
            deadline: now + NOTICE,
        });
        Ok(status)
    }
    fn status(&self, now: Instant) -> Result<Status> {
        let current = self.current.as_ref().context("no pending restart")?;
        ensure!(
            current.status.state == "pending",
            "restart already {}",
            current.status.state
        );
        ensure!(
            now < current.deadline,
            "restart cancellation deadline passed"
        );
        Ok(current.status.clone())
    }
    fn consume(
        &mut self,
        receipt: &Receipt,
        now: Instant,
    ) -> Result<(Status, Option<ConsumedUpdate>)> {
        let status = self.finish(receipt, false, now)?;
        let update = (status.state == "consumed").then(|| ConsumedUpdate {
            operation: status.receipt.clone(),
        });
        Ok((status, update))
    }
    fn finish(&mut self, receipt: &Receipt, cancel: bool, now: Instant) -> Result<Status> {
        let current = self.current.as_mut().context("no pending restart")?;
        ensure!(
            current.status.receipt == *receipt,
            "stale or foreign restart receipt"
        );
        if !cancel && current.status.state == "cancelled" {
            return Ok(current.status.clone());
        }
        ensure!(
            current.status.state == "pending",
            "restart already {}",
            current.status.state
        );
        if cancel {
            ensure!(
                now < current.deadline,
                "restart cancellation deadline passed"
            );
        } else {
            ensure!(now >= current.deadline, "restart warning has not elapsed");
        }
        // An acknowledged cancellation must fence the deploy owner's stop: https://github.com/OpenRotMG/Open-RotMG-Server/issues/1374
        current.status.state = if cancel { "cancelled" } else { "consumed" };
        Ok(current.status.clone())
    }
}

pub(super) async fn request(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Response {
    if let Some(response) = private_denial(&a, peer, &headers) {
        return response;
    }
    let result = (|| {
        ensure!(
            !*a.shutdown.borrow() && lifecycle::drain::open(&a),
            "server is already stopping"
        );
        let mut service = a.restart.lock().expect("restart countdown poisoned");
        let now = Instant::now();
        let status = match request {
            Request::Begin { owner } => service.begin(owner, now)?,
            Request::Status => service.status(now)?,
            Request::Cancel { receipt, actor_id } => {
                ensure!(
                    (17..=20).contains(&actor_id.len())
                        && actor_id.bytes().all(|b| b.is_ascii_digit()),
                    "invalid Discord actor"
                );
                let status = service.finish(&receipt, true, now)?;
                tracing::info!(actor_id, owner=%receipt.owner, deadline_ms=receipt.deadline_ms, "restart cancelled");
                status
            }
            Request::Consume { receipt } => return service.consume(&receipt, now),
        };
        tracing::info!(owner=%status.receipt.owner, deadline_ms=status.receipt.deadline_ms, state=status.state, "restart countdown observed");
        Ok((status, None))
    })();
    match result {
        Ok((status, update)) => {
            if let Some(update) = update {
                tracing::info!(instance=%update.operation.instance, owner=%update.operation.owner, deadline_ms=update.operation.deadline_ms, "intentional update stop consumed");
            }
            Json(status).into_response()
        }
        Err(error) => json_error(StatusCode::CONFLICT, error),
    }
}

#[cfg(test)]
mod tests;
