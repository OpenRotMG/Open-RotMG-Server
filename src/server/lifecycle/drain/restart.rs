use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    instance: String,
    owner: String,
    deadline_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Status {
    receipt: Receipt,
    state: &'static str,
    actor_id: Option<String>,
    #[serde(skip)]
    deadline: tokio::time::Instant,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    Begin { owner: String, seconds: u64 },
    Status { receipt: Option<Receipt> },
    Cancel { receipt: Receipt, actor_id: String },
    Consume { receipt: Receipt },
}
fn apply(a: &App, request: Request) -> Result<Status> {
    let mut state = a.admission.state.lock().expect("admission poisoned");
    ensure!(
        !*a.shutdown.borrow() && state.current.is_none(),
        "server is already stopping"
    );
    let now = tokio::time::Instant::now();
    if let Request::Begin { owner, seconds } = request {
        ensure!(
            owner.len() == 64
                && owner
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid restart owner"
        );
        ensure!(
            (1..=90).contains(&seconds),
            "restart countdown must be 1–90 seconds"
        );
        ensure!(
            state
                .restart
                .as_ref()
                .is_none_or(|s| s.state != "pending" || now >= s.deadline),
            "another restart is pending"
        );
        let deadline_ms = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?
            + seconds * 1000;
        let status = Status {
            receipt: Receipt {
                instance: state.instance.clone(),
                owner,
                deadline_ms,
            },
            state: "pending",
            actor_id: None,
            deadline: now + Duration::from_secs(seconds),
        };
        tracing::info!(owner=%status.receipt.owner, deadline_ms, "update restart pending");
        state.restart = Some(status.clone());
        return Ok(status);
    }
    let current = state
        .restart
        .as_mut()
        .context("no pending update restart")?;
    let receipt = match &request {
        Request::Status { receipt } => receipt.as_ref(),
        Request::Cancel { receipt, .. } | Request::Consume { receipt } => Some(receipt),
        Request::Begin { .. } => unreachable!(),
    };
    ensure!(
        receipt.is_none_or(|r| *r == current.receipt),
        "stale restart receipt"
    );
    match request {
        Request::Cancel { actor_id, .. } => {
            ensure!(
                (17..=20).contains(&actor_id.len()) && actor_id.bytes().all(|b| b.is_ascii_digit()),
                "invalid Discord actor"
            );
            ensure!(
                current.state == "pending" && now < current.deadline,
                "restart is no longer cancellable"
            );
            current.state = "cancelled";
            current.actor_id = Some(actor_id.clone());
            tracing::info!(%actor_id, owner=%current.receipt.owner, "update restart cancelled");
        }
        Request::Consume { .. } => {
            if current.state != "cancelled" {
                ensure!(
                    current.state == "pending" && now >= current.deadline,
                    "restart is not ready for cutover"
                );
                // The same lock as cancellation makes the cutover decision irreversible and race-free.
                current.state = "consumed";
                tracing::info!(owner=%current.receipt.owner, "update restart consumed");
            }
        }
        Request::Status { .. } => {}
        Request::Begin { .. } => unreachable!(),
    }
    Ok(current.clone())
}

pub(crate) async fn request(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Response {
    let mut response = if super::super::super::hiscores::authorize(&a, peer, &headers).is_err() {
        super::super::super::hiscores::json_error(
            StatusCode::FORBIDDEN,
            "lifecycle authorization denied",
        )
    } else {
        match apply(&a, request) {
            Ok(status) => Json(status).into_response(),
            Err(error) => super::super::super::hiscores::json_error(StatusCode::CONFLICT, error),
        }
    };
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

#[cfg(test)]
mod tests;
