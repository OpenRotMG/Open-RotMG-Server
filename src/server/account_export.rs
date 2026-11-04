use super::*;
use axum::extract::{FromRequest, Request};
use std::{io::Write, time::Duration};

const DEADLINE: Duration = Duration::from_secs(15);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    email: String,
    password: String,
}
#[derive(Debug)]
struct Failure(StatusCode, &'static str);
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}
impl std::error::Error for Failure {}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let mut response = hiscores::json_error(self.0, self.1);
        if self.0 == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert("retry-after", "60".parse().unwrap());
        }
        response
            .headers_mut()
            .insert("cache-control", "no-store".parse().unwrap());
        response
    }
}
fn internal(stage: &'static str) -> Failure {
    // Store errors may contain persisted payloads; only the failed stage crosses this boundary.
    tracing::error!(stage, "account export failed");
    Failure(StatusCode::INTERNAL_SERVER_ERROR, stage)
}

pub(super) fn routes(a: &App) -> Router<App> {
    Router::new().route(
        "/api/account/export",
        post(download)
            .fallback(|| async {
                Failure(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "Use POST with email and password.",
                )
            })
            .layer(DefaultBodyLimit::max(2048))
            .layer(axum::middleware::from_fn_with_state(
                a.clone(),
                lifecycle::drain::http_entry,
            ))
            .layer(axum::middleware::from_fn(private_response)),
    )
}
async fn private_response(request: Request, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    if response.status().is_server_error() && !response.headers().contains_key("cache-control") {
        response = Failure(
            response.status(),
            "Account export admission/completion unavailable; retry shortly.",
        )
        .into_response();
    }
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}
async fn download(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    match tokio::time::timeout(DEADLINE, run(&a, peer, request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(failure)) => failure.into_response(),
        Err(_) => Failure(
            StatusCode::GATEWAY_TIMEOUT,
            "Account export timed out; retry shortly.",
        )
        .into_response(),
    }
}
async fn run(
    a: &App,
    peer: SocketAddr,
    request: Request,
) -> std::result::Result<Response, Failure> {
    if request.uri().query().is_some() {
        return Err(Failure(
            StatusCode::BAD_REQUEST,
            "Export accepts only a JSON email/password body, not query parameters.",
        ));
    }
    // Match ordinary account requests: absent Origin is allowed for nonbrowser clients.
    if request
        .headers()
        .get("origin")
        .is_some_and(|v| v.to_str().ok() != Some(a.origin.as_str()))
    {
        return Err(Failure(
            StatusCode::FORBIDDEN,
            "Account export origin denied.",
        ));
    }
    let ip = request_ip(a, request.headers(), peer).map_err(|_| {
        Failure(
            StatusCode::BAD_REQUEST,
            "Invalid account export request identity.",
        )
    })?;
    if !rate_allow(
        &mut a.requests.lock().expect("request registry poisoned"),
        ip,
        std::time::Instant::now(),
    ) {
        return Err(Failure(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many account requests; retry in one minute.",
        ));
    }
    let Json(credentials) = Json::<Credentials>::from_request(request, a)
        .await
        .map_err(|rejection| {
            Failure(
                rejection.status(),
                "Expected a bounded JSON object containing only email and password strings.",
            )
        })?;
    if credentials.password.is_empty() {
        return Err(Failure(
            StatusCode::UNAUTHORIZED,
            "Account credentials not valid.",
        ));
    }
    let authentication = a.authentication.clone().read_owned().await;
    let account = a
        .store
        .authenticate(credentials.email, credentials.password)
        .await
        .map_err(|error| {
            if error.is::<crate::store::InvalidCredentials>() {
                Failure(StatusCode::UNAUTHORIZED, "Account credentials not valid.")
            } else if error.is::<crate::store::AuthenticationBusy>() {
                Failure(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Account authentication busy; retry shortly.",
                )
            } else {
                internal("Account export authentication unavailable; retry shortly.")
            }
        })?;
    let claim = a.try_claim(account).map_err(|error| {
        if error.is::<lifecycle::AccountBusy>() {
            Failure(StatusCode::CONFLICT, "Leave the game and wait for your character to finish saving, then retry export.")
        } else {
            internal("Account export ownership unavailable; retry shortly.")
        }
    })?.ok_or(Failure(StatusCode::TOO_MANY_REQUESTS, "Account export capacity reached; retry shortly."))?;
    // The existing completion supervisor retains both guards if the HTTP waiter times out/disconnects.
    lifecycle::account(a, vec![account], "account_export", move |a| async move {
        let _authentication = authentication;
        let _claim = claim;
        let saved =
            a.store.export_account_save(account).await.map_err(|_| {
                internal("Account export snapshot validation failed; contact support.")
            })?;
        let mut body = Bounded(Vec::new());
        serde_json::to_writer(&mut body, &saved).map_err(|_| {
            internal("Account export serialization/32 MiB limit failed; contact support.")
        })?;
        Ok((
            [
                ("content-type", "application/json".to_string()),
                ("cache-control", "no-store".to_string()),
                (
                    "content-disposition",
                    format!(
                        "attachment; filename=\"openrotmg-account-{}.json\"",
                        saved.exported_at
                    ),
                ),
            ],
            body.0,
        )
            .into_response())
    })
    .await
    .map_err(|error| match error.downcast::<Failure>() {
        Ok(failure) => failure,
        Err(_) => internal("Account export completion failed; retry shortly."),
    })
}
struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > crate::store::account_save::MAX_BYTES - self.0.len() as u64 {
            return Err(std::io::Error::other("account save exceeds 32 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests;
