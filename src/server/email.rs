use super::{
    hiscores::{json_error, json_result},
    *,
};
use std::time::Duration;

pub(crate) struct Service {
    client: reqwest::Client,
    endpoint: String,
    from: String,
    slots: tokio::sync::Semaphore,
}
impl Service {
    pub fn configured() -> Result<Option<Arc<Self>>> {
        let file = match std::env::var("EMAIL_API_KEY_FILE") {
            Ok(file) => file,
            Err(std::env::VarError::NotPresent) => {
                tracing::info!("email delivery disabled; EMAIL_API_KEY_FILE is not configured");
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        let key = std::fs::read_to_string(file).context("read email API credential")?;
        let key = key.trim();
        ensure!(!key.is_empty(), "email API credential is empty");
        let from = std::env::var("EMAIL_FROM")
            .context("EMAIL_FROM required when email delivery is enabled")?;
        ensure!(
            !from.is_empty() && !from.chars().any(char::is_control),
            "invalid EMAIL_FROM"
        );
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {key}"))
            .context("invalid email API credential")?;
        authorization.set_sensitive(true);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::AUTHORIZATION, authorization);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(12))
            .build()?;
        Ok(Some(Arc::new(Self {
            client,
            endpoint: "https://api.resend.com/emails".into(),
            from,
            slots: tokio::sync::Semaphore::new(4),
        })))
    }
    async fn send(&self, app: &App, account: i32, ip: IpAddr) -> Result<()> {
        let _permit = self
            .slots
            .try_acquire()
            .context("Email service busy; retry shortly.")?;
        let (email, token) = app.store.issue_email_verification(account, ip).await?;
        let attempt = format!("{:032x}", rand::random::<u128>());
        let link = format!("{}/verify-email/#{token}", app.origin);
        let text = format!(
            "Confirm this email address for your Open RotMG account:\n\n{link}\n\nThis link expires in 24 hours and works once. Open the page and choose Confirm email. A new verification request replaces the previous link.\n\nIf you did not request this, ignore this message. Your account and password have not been changed. We will never ask you to send us your password.\n\nOpen RotMG\nhttps://openrotmg.org/"
        );
        let result: Result<String> = async {
            let mut response = self.client.post(&self.endpoint)
                .header("Idempotency-Key", format!("email-verification-{attempt}"))
                .json(&serde_json::json!({"from":self.from,"to":[email],"subject":"Verify your Open RotMG email","text":text}))
                .send().await.map_err(|e| anyhow::anyhow!("verification mail transport failed (timeout: {}); delivery is unconfirmed", e.is_timeout()))?;
            ensure!(response.status().is_success(), "verification mail provider rejected the request (HTTP {}); delivery is unconfirmed", response.status().as_u16());
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.context("read verification mail receipt")? {
                ensure!(body.len()+chunk.len() <= 16384, "verification mail receipt exceeds 16 KiB");
                body.extend_from_slice(&chunk);
            }
            #[derive(Deserialize)]
            struct Receipt { id: String }
            let receipt: Receipt = serde_json::from_slice(&body).context("invalid verification mail receipt; delivery is unconfirmed")?;
            ensure!(!receipt.id.is_empty() && receipt.id.len() <= 128 && receipt.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'), "invalid verification mail message ID");
            Ok(receipt.id)
        }.await;
        match result {
            Ok(message_id) => {
                tracing::info!(
                    account,
                    attempt,
                    message_id,
                    "verification mail accepted by provider; inbox delivery not yet confirmed"
                );
                Ok(())
            }
            Err(error) => {
                tracing::warn!(account, attempt, error=%error, "verification mail delivery unconfirmed; reserved quota retained");
                Err(anyhow::anyhow!("Send unconfirmed. Check email first."))
            }
        }
    }
}
pub(super) async fn send(app: &App, account: i32, ip: IpAddr) -> Result<()> {
    app.email
        .as_ref()
        .context("Email delivery is not configured.")?
        .send(app, account, ip)
        .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Confirmation {
    token: String,
}
pub(super) async fn confirm(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<Confirmation>,
) -> Response {
    if headers.get("origin").and_then(|h| h.to_str().ok()) != Some(app.origin.as_str()) {
        return json_error(StatusCode::FORBIDDEN, "verification request origin denied");
    }
    match admit(&app, &headers, peer) {
        Ok(true) => {}
        Ok(false) => {
            return json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many verification attempts; wait one minute",
            );
        }
        Err(e) => return json_result(Err(e)),
    }
    json_result(
        app.store
            .confirm_email(request.token)
            .await
            .map(|_| serde_json::json!({"verified":true})),
    )
}

#[cfg(test)]
mod tests;
