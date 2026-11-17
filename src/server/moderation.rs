use super::hiscores::{json_result, private_denial};
use super::*;

pub struct Moderation {
    moderators: Vec<i32>,
    reviews: std::path::PathBuf,
    webhook: String,
    http: reqwest::Client,
}

impl Moderation {
    pub fn configured() -> Result<Option<Arc<Self>>> {
        let moderators = match std::env::var("OPENROTMG_MODERATORS") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let moderators = moderators
            .split(',')
            .map(|id| {
                id.trim()
                    .parse()
                    .ok()
                    .filter(|id: &i32| *id > 0)
                    .with_context(|| format!("invalid moderator account {id:?}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let reviews = std::env::var_os("OPENROTMG_REVIEWED_ACCOUNTS")
            .context("OPENROTMG_MODERATORS requires OPENROTMG_REVIEWED_ACCOUNTS")?
            .into();
        let path = std::env::var("REPORT_WEBHOOK_FILE")
            .context("OPENROTMG_MODERATORS requires REPORT_WEBHOOK_FILE")?;
        let webhook = std::fs::read_to_string(&path)
            .with_context(|| format!("read report webhook {path}"))?
            .trim()
            .to_string();
        ensure!(
            webhook.starts_with("https://discord.com/api/webhooks/")
                || webhook.starts_with("http://127.0.0.1:"),
            "report webhook must be a Discord webhook URL"
        );
        tracing::info!(?moderators, "moderator reports enabled");
        Ok(Some(Arc::new(Self {
            moderators,
            reviews,
            webhook,
            http: reqwest::Client::new(),
        })))
    }
}

pub(super) async fn report(
    a: &App,
    reporter_account: i32,
    reporter: i32,
    name: &str,
) -> Result<String> {
    ensure!(
        a.moderation
            .as_ref()
            .is_some_and(|m| m.moderators.contains(&reporter_account)),
        lifecycle::controller("Only moderators can use /report.")
    );
    ensure!(
        !name.is_empty() && !name.contains(char::is_whitespace),
        lifecycle::controller("Usage: /report <player name>")
    );
    let wanted = name.to_string();
    let (reporter_name, account, name) = a
        .world
        .directory(move |d| {
            let reporter_name = d.worlds[&d.location(reporter)?].players[&reporter]
                .character
                .name
                .clone();
            let target = d
                .worlds
                .values()
                .flat_map(|w| w.players.values())
                .find(|p| {
                    p.controller == crate::world::Controller::Human
                        && p.character.name.eq_ignore_ascii_case(&wanted)
                })
                .ok_or_else(|| lifecycle::controller("Player not found."))?;
            Ok((
                reporter_name,
                target.character.account,
                target.character.name.clone(),
            ))
        })
        .await?;
    ensure!(
        account != reporter_account,
        lifecycle::controller("You cannot report yourself.")
    );
    let m = a.moderation.clone().expect("checked moderator");
    let reviews = a.content.reviewed_accounts.clone();
    let path = m.reviews.clone();
    let added = tokio::task::spawn_blocking(move || {
        reviews.mark(&path, account, crate::content::reviews::today())
    })
    .await??;
    ensure!(
        added,
        lifecycle::controller("That player is already marked as a cheater.")
    );
    tracing::warn!(reporter = reporter_account, account, %name, "moderator marked account as cheater");
    let text =
        format!("Player {reporter_name} reported player {name}; they are now marked as a cheater.");
    let posted = m
        .http
        .post(&m.webhook)
        .timeout(std::time::Duration::from_secs(10))
        .json(&serde_json::json!({"content":text,"allowed_mentions":{"parse":[]}}))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    Ok(match posted {
        Ok(_) => format!(
            "Player {name} was marked as a cheater and will receive fewer white and potion bags."
        ),
        Err(error) => {
            tracing::error!(account, %error, "report Discord notification failed");
            format!(
                "Player {name} was marked as a cheater, but the Discord notification failed: {error}"
            )
        }
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CheatKick {
    account: i32,
    realm_lockout_seconds: u64,
}
/// The private detector decides when to call this; the server only lets it act on already-marked accounts.
pub(super) async fn cheat_kick(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<CheatKick>,
) -> Response {
    if let Some(response) = private_denial(&a, peer, &headers) {
        return response;
    }
    json_result(
        async {
            let CheatKick {
                account,
                realm_lockout_seconds,
            } = request;
            ensure!(
                a.content.reviewed(account),
                "account {account} is not marked"
            );
            ensure!(
                (1..=3600).contains(&realm_lockout_seconds),
                "realm lockout must be 1-3600 seconds"
            );
            let until =
                std::time::Instant::now() + std::time::Duration::from_secs(realm_lockout_seconds);
            a.world
                .directory(move |d| {
                    d.realm_lockouts.insert(account, until);
                    Ok(())
                })
                .await?;
            let disconnected = a.kick(account).await?;
            tracing::warn!(
                account,
                disconnected,
                realm_lockout_seconds,
                "marked account kicked for detected cheating"
            );
            Ok(serde_json::json!({"disconnected":disconnected}))
        }
        .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Overlaps {
    account: i32,
    player: i32,
    hits: Vec<(i32, u8, u64)>,
}
pub(super) async fn overlaps(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<Overlaps>,
) -> Response {
    if let Some(response) = private_denial(&a, peer, &headers) {
        return response;
    }
    json_result(
        async {
            let Overlaps {
                account,
                player,
                hits,
            } = request;
            ensure!(
                a.content.reviewed(account),
                "account {account} is not marked"
            );
            ensure!((1..=64).contains(&hits.len()), "1-64 hits required");
            let requested = hits.len();
            let applied = a
                .world
                .directory(move |d| {
                    let w = d.worlds.get_mut(&d.location(player)?).unwrap();
                    ensure!(
                        w.players[&player].character.account == account,
                        "player {player} is not account {account}"
                    );
                    w.apply_overlaps(player, &hits)
                })
                .await?;
            tracing::warn!(
                account,
                player,
                requested,
                applied,
                "overlaps applied to marked account"
            );
            Ok(serde_json::json!({"applied":applied}))
        }
        .await,
    )
}

#[cfg(test)]
mod tests;
