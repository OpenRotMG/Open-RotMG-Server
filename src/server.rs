#[cfg(test)]
mod character_slot_tests;
#[cfg(test)]
mod client_contract_tests;
mod input;
#[cfg(test)]
mod lifecycle_finalize_tests;
#[cfg(test)]
mod lifecycle_native_tests;
#[cfg(test)]
mod name_choice_boundaries_tests;
#[cfg(test)]
mod name_choice_tests;
#[cfg(test)]
mod password_tests;
mod pictures;
#[cfg(test)]
mod purchase_tests;
#[cfg(test)]
mod queue_tests;
#[cfg(test)]
mod resurrection_tests;
#[cfg(test)]
mod usage_tests;
#[cfg(test)]
mod xp_booster_tests;
use crate::{
    content::{Content, WIZARD},
    crypto::Keys,
    store::{InventoryWrite, Store, xml_escape},
    wire::{self, Codec, Packet, Reader, Writer},
    world::{Action, Handle, InventoryPlan, Pos, Slot, TradeAction, TradePlan, World},
};
use anyhow::{Context, Result, bail, ensure};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, DefaultBodyLimit, Form, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{FutureExt, SinkExt, StreamExt};
use input::Incoming;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
};
pub(crate) mod lifecycle;
pub use lifecycle::Owner;
use lifecycle::{CloseReason, Stage};
impl App {
    fn server_usage(&self) -> Result<f64> {
        ensure!(
            (1..=85).contains(&self.max_players),
            "invalid server capacity: {}",
            self.max_players
        );
        // Admission includes claims before avatars exist and retains them through failed saves.
        let occupied = self
            .owners
            .lock()
            .map_err(|_| anyhow::anyhow!("ownership registry poisoned"))?
            .len();
        ensure!(
            occupied <= self.max_players,
            "ownership exceeds capacity: {occupied}/{}",
            self.max_players
        );
        let usage = occupied as f64 / self.max_players as f64;
        tracing::debug!(
            occupied,
            capacity = self.max_players,
            usage,
            "advertised admission usage"
        );
        Ok(usage)
    }
}
#[cfg(test)]
pub(crate) use account_actions::tests::fixture as lifecycle_test_fixture;
use tokio::sync::mpsc;
mod account_actions;
pub(crate) mod diagnostics;
mod gold;
mod guilds;
mod hiscores;
pub(crate) mod testing;
mod updates;
use account_actions::{discord_announce, discord_kick, discord_recover, reset_password};
use hiscores::{collection, discord_link, hiscores};
#[derive(Clone)]
pub struct App {
    pub started: std::time::Instant,
    pub testing: Option<testing::Testing>,
    pub diagnostics: Arc<diagnostics::Diagnostics>,
    pub store: Store,
    pub world: Handle,
    pub keys: Arc<Keys>,
    pub content: Arc<Content>,
    pub swf: Arc<Vec<u8>>,
    pub origin: String,
    pub sfx: PathBuf,
    pub max_players: usize,
    pub behind_proxy: bool,
    pub shore_bots: bool,
    pub dialogue: Option<Arc<crate::bots::dialogue::Service>>,
    pub society: Arc<Mutex<crate::bots::adventures::Society>>,
    pub requests: Arc<Mutex<HashMap<IpAddr, (std::time::Instant, u32)>>>,
    pub picture_reads: Arc<Mutex<HashMap<IpAddr, (std::time::Instant, u32)>>>,
    pub connections: Arc<tokio::sync::Semaphore>,
    pub control: Option<String>,
    pub discord_link_token: Option<String>,
    pub owners: Arc<Mutex<BTreeMap<i32, lifecycle::Claim>>>,
    pub authentication: Arc<tokio::sync::RwLock<()>>,
    pub guild_actions: Arc<tokio::sync::Mutex<()>>,
    pub gold_updates: Arc<tokio::sync::Mutex<()>>,
    pub bots: Arc<Mutex<BTreeMap<i32, Owner>>>,
    #[cfg(test)]
    pub quarantined: Arc<Mutex<Vec<Owner>>>,
    pub shutdown: tokio::sync::watch::Sender<bool>,
    pub(crate) admission: Arc<lifecycle::drain::Admission>,
}
fn error(e: anyhow::Error) -> Response {
    tracing::error!(error=%format_args!("{e:#}"),"request failed");
    (
        StatusCode::BAD_REQUEST,
        [("content-type", "application/xml")],
        format!("<Error>{}</Error>", xml_escape(&e.to_string())),
    )
        .into_response()
}
fn request_ip(a: &App, headers: &HeaderMap, peer: SocketAddr) -> Result<IpAddr> {
    if let Some(origin) = headers.get("origin") {
        ensure!(
            origin.to_str()? == a.origin,
            "unapproved request origin: {}; expected {}",
            origin.to_str()?,
            a.origin
        );
    }
    let ip = if a.behind_proxy {
        headers
            .get("x-forwarded-for")
            .context("proxy must supply X-Forwarded-For")?
            .to_str()?
            .parse::<IpAddr>()
            .context("proxy must supply one client IP")?
    } else {
        peer.ip()
    };
    Ok(ip)
}
fn admit(a: &App, headers: &HeaderMap, peer: SocketAddr) -> Result<bool> {
    let ip = request_ip(a, headers, peer)?;
    Ok(rate_allow(
        &mut a.requests.lock().expect("request registry poisoned"),
        ip,
        std::time::Instant::now(),
    ))
}
fn rate_allow(
    requests: &mut HashMap<IpAddr, (std::time::Instant, u32)>,
    ip: IpAddr,
    now: std::time::Instant,
) -> bool {
    requests.retain(|_, (start, _)| now.duration_since(*start).as_secs() < 60);
    if requests.len() >= 4096 && !requests.contains_key(&ip) {
        return false;
    }
    let (_, count) = requests.entry(ip).or_insert((now, 0));
    *count = count.saturating_add(1);
    if *count == 21 {
        tracing::warn!(%ip, "guest entry rate exceeded");
    }
    *count <= 20
}
fn limited() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("content-type", "application/xml"), ("retry-after", "60")],
        "<Error>Too many guest entry requests; retry in one minute.</Error>",
    )
        .into_response()
}
async fn chars(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    match admit(&a, &headers, peer) {
        Ok(true) => {}
        Ok(false) => return limited(),
        Err(e) => return error(e),
    }
    let result: Result<String> = async {
        let guid = form.get("guid").context("guid is required")?.clone();
        ensure!(
            form.get("secret").is_none_or(|s| s.is_empty()),
            "external platform authentication is not implemented"
        );
        let password = form.get("password").context("password required")?.clone();
        let _authentication = a.authentication.read().await;
        let account = a.store.characters(guid, password).await?;
        let feed = a.store.updates().await?;
        Ok(account.xml(&feed.news_xml(&a.origin), a.server_usage()?))
    }
    .await;
    match result {
        Ok(xml) => ([("content-type", "application/xml")], xml).into_response(),
        Err(e) if e.is::<crate::store::InvalidCredentials>() => {
            tracing::info!("legacy character-list credentials rejected");
            (
                [("content-type", "application/xml")],
                "<Error>Account credentials not valid</Error>",
            )
                .into_response()
        }
        Err(e) => error(e),
    }
}
async fn delete_character(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    match admit(&a, &headers, peer) {
        Ok(true) => {}
        Ok(false) => return limited(),
        Err(e) => return error(e),
    }
    let result: Result<()> = async {
        let character: i32 = form
            .get("charId")
            .context("charId required")?
            .parse()
            .context("invalid charId")?;
        ensure!(character > 0, "invalid charId");
        ensure!(
            form.get("secret").is_none_or(|s| s.is_empty()),
            "external platform authentication is not implemented"
        );
        let guid = form.get("guid").context("guid required")?.clone();
        let password = form.get("password").context("password required")?.clone();
        let _authentication = a.authentication.read().await;
        let account = a.store.authenticate(guid, password).await?;
        let _claim = a.claim(account)?;
        lifecycle::account(&a, vec![account], "delete_character", move |a| async move {
            lifecycle::stage(Stage::Store);
            a.store.delete_character(account, character).await
        })
        .await
    }
    .await;
    let xml = match result {
        Ok(()) => "<Success/>".into(),
        Err(e) => {
            tracing::warn!(error=%e, "character deletion rejected");
            let message = if e.is::<crate::store::InvalidCredentials>() {
                "Account credentials not valid".into()
            } else {
                e.to_string()
            };
            format!("<Error>{}</Error>", xml_escape(&message))
        }
    };
    (
        [
            ("content-type", "application/xml"),
            ("cache-control", "no-store"),
        ],
        xml,
    )
        .into_response()
}
async fn account_request(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Path(operation): axum::extract::Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    match admit(&a, &headers, peer) {
        Ok(true) => {}
        Ok(false) => return limited(),
        Err(e) => return error(e),
    }
    let result: Result<i32> = async {
        let guid = form.get("guid").context("guid required")?.clone();
        if operation == "changePassword" {
            ensure!(
                form.get("secret").is_none_or(|s| s.is_empty()),
                "external platform authentication is not implemented"
            );
            let password = form.get("password").context("password required")?.clone();
            ensure!(!password.is_empty(), crate::store::InvalidCredentials);
            let verifier = a
                .store
                .prepare_password(
                    form.get("newPassword")
                        .context("newPassword required")?
                        .clone(),
                )
                .await?;
            // Fence the current verifier through ownership and commit, without upgrading a read lock.
            let authentication = a.authentication.clone().write_owned().await;
            let id = a.store.authenticate(guid, password).await?;
            let _claim = a.claim(id)?;
            lifecycle::account(&a, vec![id], "change_password", move |a| async move {
                let _authentication = authentication;
                lifecycle::stage(Stage::Store);
                a.store.change_password(id, verifier).await
            })
            .await?;
            return Ok(id);
        }
        let _authentication = a.authentication.read().await;
        let id = match operation.as_str() {
            "register" => {
                a.store
                    .register(
                        guid,
                        form.get("newGUID").context("newGUID required")?.clone(),
                        form.get("newPassword")
                            .context("newPassword required")?
                            .clone(),
                    )
                    .await?
            }
            "verify" => {
                let password = form.get("password").context("password required")?.clone();
                ensure!(!password.is_empty(), "email and password required");
                a.store.authenticate(guid, password).await?
            }
            "setName" => {
                let id = a
                    .store
                    .authenticate(
                        guid,
                        form.get("password").context("password required")?.clone(),
                    )
                    .await?;
                let _claim = a.claim(id)?;
                let name = form.get("name").context("name required")?.clone();
                lifecycle::account(&a, vec![id], "choose_name", move |a| async move {
                    lifecycle::stage(Stage::Store);
                    a.store.choose_name(id, name).await
                })
                .await?;
                id
            }
            "purchaseCharSlot" => {
                ensure!(
                    form.get("secret").is_none_or(|s| s.is_empty()),
                    "external platform authentication is not implemented"
                );
                let timer: i32 = form
                    .get("ignore")
                    .context("ignore timer required")?
                    .parse()
                    .context("invalid signed-32-bit ignore timer")?;
                let password = form.get("password").context("password required")?.clone();
                ensure!(
                    !password.is_empty(),
                    "Register an account before buying character slots."
                );
                let id = a.store.authenticate(guid, password).await?;
                let _claim = a.claim(id)?;
                lifecycle::account(
                    &a,
                    vec![id],
                    "purchase_character_slot",
                    move |a| async move {
                        lifecycle::stage(Stage::Store);
                        a.store.purchase_character_slot(id, timer).await
                    },
                )
                .await?;
                id
            }
            _ => bail!("account operation not implemented"),
        };
        Ok(id)
    }
    .await;
    let xml = match result {
        Ok(id) => {
            tracing::info!(account = id, operation, "account request completed");
            "<Success/>".into()
        }
        Err(e) => {
            tracing::warn!(error=%e, operation, "account request rejected");
            let tag = if e.is::<crate::store::SlotReplay>() {
                "FatalError"
            } else {
                "Error"
            };
            format!("<{tag}>{}</{tag}>", xml_escape(&e.to_string()))
        }
    };
    // The original account service carries semantic failures in XML, not HTTP error pages.
    (
        [
            ("content-type", "application/xml"),
            ("cache-control", "no-store"),
        ],
        xml,
    )
        .into_response()
}
async fn legends(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    match admit(&a, &headers, peer) {
        Ok(true) => {}
        Ok(false) => return limited(),
        Err(e) => return error(e),
    }
    let result: Result<String> = async {
        let span = form.get("timespan").context("timespan required")?.clone();
        let account = form
            .get("accountId")
            .context("accountId required")?
            .parse()?;
        let character = form.get("charId").context("charId required")?.parse()?;
        a.store.legends(span, account, character).await
    }
    .await;
    match result {
        Ok(xml) => (
            [
                ("content-type", "application/xml"),
                ("cache-control", "no-store"),
            ],
            xml,
        )
            .into_response(),
        Err(e) => error(e),
    }
}
async fn fame(State(a): State<App>, Form(form): Form<HashMap<String, String>>) -> Response {
    let result: Result<String> = async {
        let account = form
            .get("accountId")
            .context("accountId required")?
            .parse()?;
        let character = form.get("charId").context("charId required")?.parse()?;
        a.store.fame(account, character).await
    }
    .await;
    match result {
        Ok(xml) => ([("content-type", "application/xml")], xml).into_response(),
        Err(e) => error(e),
    }
}
async fn swf(State(a): State<App>) -> Response {
    (
        [
            ("content-type", "application/x-shockwave-flash"),
            ("cache-control", "no-store"),
        ],
        a.swf.as_ref().clone(),
    )
        .into_response()
}
async fn health(State(a): State<App>) -> Response {
    let started = a.started;
    match a.world.directory(move |d| Ok(d.health(started))).await {
        Ok(mut v) => {
            v["bot_dialogue"] = match &a.dialogue {
                Some(service) => service.health(),
                None => serde_json::json!({"enabled":false,"status":"disabled"}),
            };
            Json(v).into_response()
        }
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, error(e)).into_response(),
    }
}
async fn websocket(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<DiagnosticQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    if origin != Some(a.origin.as_str()) {
        tracing::warn!(?origin, expected=%a.origin, "unapproved WebSocket origin");
        return (
            StatusCode::FORBIDDEN,
            "unapproved WebSocket origin; inspect the server log for received and expected origins",
        )
            .into_response();
    }
    match admit(&a, &headers, peer) {
        Ok(true) => {}
        Ok(false) => return limited(),
        Err(e) => return error(e),
    }
    let Ok(permit) = a.connections.clone().try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "game connection limit reached",
        )
            .into_response();
    };
    let browser = match query
        .diagnostics
        .as_deref()
        .map(|key| a.diagnostics.browser(key))
        .transpose()
    {
        Ok(browser) => browser,
        Err(e) => return error(e),
    };
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(512).collect());
    let entry = match lifecycle::drain::entry(&a) {
        Ok(entry) => entry,
        Err(error) => return hiscores::json_error(StatusCode::SERVICE_UNAVAILABLE, error),
    };
    ws.max_message_size(1_048_576).on_upgrade(move |socket| {
        entry.scope(async move {
            let _permit = permit;
            connection(socket, a, user_agent, browser).await
        })
    })
}
#[derive(Deserialize)]
struct DiagnosticQuery {
    diagnostics: Option<String>,
}
fn fail(description: &str) -> Packet {
    let mut o = Writer::new();
    o.i32(0).utf(description);
    Packet { id: 0, body: o.0 }
}
struct NativeReader<'a>(Reader<'a>);
impl<'a> NativeReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        self.0.take(n).context(CloseReason::Protocol)
    }
    fn done(&self) -> Result<()> {
        self.0.done().context(CloseReason::Protocol)
    }
}
macro_rules! native_readers {
    ($($method:ident: $ty:ty),*) => { impl NativeReader<'_> { $(fn $method(&mut self) -> Result<$ty> { self.0.$method().context(CloseReason::Protocol) })* } };
}
native_readers!(u8: u8, i16: i16, i32: i32, f32: f32, utf: String);
impl<'a> std::ops::Deref for NativeReader<'a> {
    type Target = Reader<'a>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for NativeReader<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn slot(r: &mut Reader<'_>) -> Result<Slot> {
    Ok(Slot {
        object: r.i32()?,
        slot: r.u8()? as usize,
        kind: r.i16()? as i32,
    })
}
async fn persist(a: &App, plan: InventoryPlan) -> Result<()> {
    lifecycle::stage(Stage::Store);
    if let Some(purchase) = &plan.vault_purchase {
        let _gold = a.gold_updates.lock().await;
        let result = a
            .store
            .purchase_vault(
                InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                },
                purchase.chest,
            )
            .await;
        lifecycle::stage(Stage::Publication);
        a.world
            .for_player(plan.player)
            .call(move |w| w.finish_vault_purchase(plan, result))
            .await?;
        lifecycle::stage(Stage::Settled);
        return Ok(());
    }
    if let Some((_, purchase)) = &plan.purchase {
        let _gold = a.gold_updates.lock().await;
        let result = a
            .store
            .purchase(
                InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                },
                purchase.clone(),
            )
            .await;
        lifecycle::stage(Stage::Publication);
        a.world
            .for_player(plan.player)
            .call(move |w| w.finish_purchase(plan, result))
            .await?;
        lifecycle::stage(Stage::Settled);
        return Ok(());
    }
    lifecycle::stage(Stage::Reservation);
    let plan = a.world.directory(move |d| d.stage_unlock(plan)).await?;
    lifecycle::stage(Stage::Store);
    let collector = plan.collected.map(|_| plan.account);
    let result = if let Some(pet) = plan.companion.as_ref().filter(|pet| pet.permanent) {
        a.store
            .select_companion(
                InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                },
                pet.source.slot,
                u16::try_from(pet.source.kind)?,
                pet.selected,
                a.content.clone(),
            )
            .await
    } else if let Some(slot) = plan.xp_boost {
        a.store
            .consume_xp_booster(
                InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                },
                (slot.object == plan.player).then_some(slot.slot),
                plan.vaults.clone(),
            )
            .await
    } else if let Some((slot, tex1, tex2)) = plan.appearance {
        a.store
            .consume_dye(
                InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                },
                slot.slot,
                u16::try_from(slot.kind)?,
                [tex1, tex2],
            )
            .await
    } else if let Some((snapshot, slot, index, amount)) = &plan.stat_growth {
        a.store
            .consume_stat(
                snapshot.clone(),
                (slot.object == plan.player).then_some(slot.slot),
                *index,
                *amount,
                plan.vaults.clone(),
            )
            .await
    } else {
        a.store
            .exchange_collected(
                vec![InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                }],
                plan.vaults.clone(),
                plan.collected,
            )
            .await
            .and_then(|mut revisions| {
                revisions
                    .pop()
                    .context("inventory commit returned no revision")
            })
    };
    lifecycle::stage(Stage::Publication);
    a.world
        .directory(move |d| d.finish_inventory(plan, result))
        .await?;
    if let Some(account) = collector {
        gold::sync(a, account).await?;
    }
    lifecycle::stage(Stage::Settled);
    Ok(())
}
pub(crate) async fn inventory(
    a: &App,
    id: i32,
    prepare: impl FnOnce(&mut World) -> Result<Option<InventoryPlan>> + Send + 'static,
) -> Result<Result<()>> {
    lifecycle::player(a, id, "inventory", move |a| async move {
        lifecycle::stage(Stage::Reservation);
        let prepared = a.world.for_player(id).call(move |w| Ok(prepare(w))).await?;
        match prepared {
            Ok(Some(plan)) => {
                persist(&a, plan).await?;
                Ok(Ok(()))
            }
            Ok(None) => Ok(Ok(())),
            Err(error) if lifecycle::rule_rejection(&error) => {
                lifecycle::stage(Stage::Rejected);
                Ok(Err(error))
            }
            Err(error) => Err(error),
        }
    })
    .await
}

async fn persist_progress_inner(a: &App, id: i32, snapshot: crate::store::Character) -> Result<()> {
    lifecycle::stage(Stage::Store);
    let result = a
        .store
        .progress(snapshot.clone())
        .await
        .context("progress persistence failed");
    lifecycle::stage(Stage::Publication);
    a.world
        .for_player(id)
        .call(move |w| {
            w.committed(id, result)?;
            w.finish_progress(id, snapshot)
        })
        .await
}
#[cfg(test)]
async fn persist_death(
    a: &App,
    id: i32,
    ch: crate::store::Character,
    killer: String,
) -> Result<()> {
    lifecycle::player(a, id, "death", move |a| async move {
        persist_death_inner(&a, id, ch, killer).await
    })
    .await
}
async fn persist_death_inner(
    a: &App,
    id: i32,
    ch: crate::store::Character,
    killer: String,
) -> Result<()> {
    lifecycle::stage(Stage::Store);
    let slot = ch
        .items
        .iter()
        .take(4)
        .position(|kind| *kind >= 0 && a.content.objects[&(*kind as u16)].resurrects);
    if let Some(slot) = slot {
        let result = a
            .store
            .resurrect(ch, slot, a.content.clone())
            .await
            .context("resurrection persistence failed");
        lifecycle::stage(Stage::Publication);
        a.world
            .directory(move |d| d.finish_resurrection(id, result))
            .await
    } else {
        let result = a
            .store
            .death(ch, killer)
            .await
            .context("death persistence failed");
        lifecycle::stage(Stage::Publication);
        a.world
            .for_player(id)
            .call(move |w| {
                w.committed(id, result)?;
                w.finish_death(id)
            })
            .await
    }
}
async fn persist_trade(a: &App, plan: TradePlan) -> Result<()> {
    let report_failure = lifecycle::failure_reporter(
        a,
        &plan.plans.iter().map(|p| p.account).collect::<Vec<_>>(),
        "trade",
    )?;
    let result = std::panic::AssertUnwindSafe(async {
        lifecycle::stage(Stage::Store);
        let id = plan.plans[0].player;
        let writes = plan
            .plans
            .iter()
            .map(|p| InventoryWrite {
                id: p.character,
                account: p.account,
                revision: p.revision,
                items: p.items.clone(),
            })
            .collect();
        let result = a.store.exchange(writes, Vec::new()).await;
        lifecycle::stage(Stage::Publication);
        a.world
            .for_player(id)
            .call(move |w| w.finish_trade(plan, result))
            .await
    })
    .catch_unwind()
    .await;
    match result {
        Ok(Ok(())) => {
            lifecycle::stage(Stage::Settled);
            Ok(())
        }
        Ok(Err(error)) => {
            report_failure(format!("{error:#}"));
            Err(error)
        }
        Err(panic) => {
            report_failure("trade completion task panicked".into());
            std::panic::resume_unwind(panic)
        }
    }
}

fn trade_offer(r: &mut Reader<'_>) -> Result<[bool; 12]> {
    ensure!(r.i16()? == 12, "trade offer must contain twelve slots");
    let mut values = [false; 12];
    for value in &mut values {
        let raw = r.u8()?;
        ensure!(raw <= 1, "invalid trade offer boolean");
        *value = raw == 1;
    }
    Ok(values)
}
fn player_text(r: &mut Reader<'_>) -> Result<String> {
    let text = r.utf()?;
    r.done()?;
    Ok(text)
}
#[derive(Default)]
struct Session {
    account: Option<Owner>,
    player: Option<i32>,
    expected: Option<i32>,
    reconnecting: bool,
    transfer: Option<Packet>,
    runtime: Option<tokio::runtime::Handle>,
    diagnostics: diagnostics::ConnectionDiagnostics,
}
fn collision_report(other: bool, r: &mut Reader<'_>) -> Result<()> {
    // Reports cannot grant hits: the actor advances and resolves projectiles itself.
    r.i32()?;
    r.u8()?;
    r.i32()?;
    if other {
        r.i32()?;
    }
    r.done()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplyQueueError {
    Full,
    Closed,
}
impl std::fmt::Display for ReplyQueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Full => "connection reply queue is full",
            Self::Closed => "connection reply queue is closed",
        })
    }
}
impl std::error::Error for ReplyQueueError {}

fn enqueue_reply(tx: &mpsc::Sender<Packet>, packet: Packet) -> Result<()> {
    // This handler's caller is also the only consumer, so waiting for capacity deadlocks.
    tx.try_send(packet).map_err(|error| {
        match error {
            mpsc::error::TrySendError::Full(_) => ReplyQueueError::Full,
            mpsc::error::TrySendError::Closed(_) => ReplyQueueError::Closed,
        }
        .into()
    })
}

async fn handle(
    a: &mut App,
    packet: Packet,
    session: &mut Session,
    tx: &mpsc::Sender<Packet>,
    lost: &tokio::sync::watch::Sender<bool>,
    received: std::time::Instant,
) -> Result<()> {
    let entry = lifecycle::drain::entry(a)?;
    if let (Some(owner), Some(player)) = (&session.account, session.player) {
        owner.bind(player)?;
    }
    let mut owned_session = std::mem::take(session);
    let reply = tx.clone();
    let tx = tx.clone();
    let lost = lost.clone();
    let accounts = owned_session
        .account
        .as_ref()
        .map(|owner| vec![owner.id])
        .unwrap_or_default();
    let (app, completed_session, result) =
        lifecycle::account(a, accounts, "native_packet", move |mut app| async move {
            let result = entry
                .scope(handle_inner(
                    &mut app,
                    packet,
                    &mut owned_session,
                    &tx,
                    &lost,
                    received,
                ))
                .await;
            if let (Err(error), Some(owner)) = (&result, &owned_session.account) {
                lifecycle::observe(&app, owner.id, "native_packet", error);
            }
            Ok((app, owned_session, result))
        })
        .await?;
    *a = app;
    *session = completed_session;
    result?;
    if let Some(packet) = session.transfer.take() {
        let id = session
            .player
            .context("prepared transfer lost its avatar")?;
        depart(a, id).await?;
        session.player = None;
        // Admission must be released before RECONNECT can reach an immediate replacement HELLO.
        session
            .account
            .take()
            .context("transfer ownership missing")?
            .release();
        enqueue_reply(&reply, packet)?;
        session.reconnecting = true;
    }
    Ok(())
}

async fn handle_inner(
    a: &mut App,
    packet: Packet,
    session: &mut Session,
    tx: &mpsc::Sender<Packet>,
    lost: &tokio::sync::watch::Sender<bool>,
    received: std::time::Instant,
) -> Result<()> {
    let Session {
        account,
        player,
        expected,
        reconnecting,
        transfer: _,
        runtime,
        diagnostics,
    } = session;
    if *reconnecting {
        tracing::debug!(
            packet = packet.id,
            "discarded in-flight input after durable departure"
        );
        return Ok(());
    }
    // Resurrection is alive again, but its old scene must stay inert until the owned RECONNECT.
    if let Some(id) = *player
        && a.world
            .call(move |w| {
                let p = w.players.get(&id).context("player missing")?;
                Ok(p.native_transferring()
                    || matches!(packet.id, 7 | 38 | 30 | 65 | 33 | 42 | 3)
                        && (p.death_pending || !p.character.alive))
            })
            .await?
    {
        tracing::debug!(
            player = id,
            packet = packet.id,
            "ignored queued command from dying or transferring character"
        );
        return Ok(());
    }
    macro_rules! ensure_protocol {
        ($condition:expr, $($detail:tt)*) => { ensure!($condition, anyhow::anyhow!($($detail)*).context(CloseReason::Protocol)); };
    }
    let mut r = NativeReader(Reader::new(&packet.body));
    if packet.id == wire::BUY {
        let mut shape = NativeReader(Reader::new(&packet.body));
        shape.i32()?;
        shape.done()?;
    }
    match packet.id {
        wire::HELLO => {
            ensure_protocol!(account.is_none(), "duplicate HELLO");
            let version = r.utf()?;
            ensure_protocol!(version == "123.5.1", "unsupported client build {version}");
            let game = r.i32()?;
            let guid = a.keys.decrypt(&r.utf()?).context(CloseReason::Protocol)?;
            let password = a.keys.decrypt(&r.utf()?).context(CloseReason::Protocol)?;
            let secret = a.keys.decrypt(&r.utf()?).context(CloseReason::Protocol)?;
            ensure_protocol!(
                secret.is_empty(),
                "external platform authentication is not implemented"
            );
            let keytime = r.i32()?;
            let n = usize::try_from(r.i16()?).context(CloseReason::Protocol)?;
            ensure_protocol!(n == 0 || n == 16, "invalid transfer key size");
            let key = r.take(n)?.to_vec();
            ensure_protocol!(r.i32()? == 0, "map editor JSON not supported");
            for _ in 0..4 {
                r.utf()?;
            }
            r.done()?;
            let authentication = a.authentication.read().await;
            let registered = !password.is_empty();
            let id = a.store.authenticate(guid, password).await?;
            let mut owner = a.claim_session(id)?;
            if let Some(runtime) = runtime {
                owner.arm_cleanup(a, runtime.clone());
            }
            owner.registered = registered;
            *account = Some(owner);
            drop(authentication);
            lifecycle::account(a, vec![id], "login", move |a| async move {
                lifecycle::stage(Stage::Store);
                a.store.compete(id).await
            })
            .await?;
            let prepared = a
                .testing
                .as_ref()
                .map(|t| t.native_entry(id))
                .transpose()?
                .flatten();
            let (destination, character) = a
                .world
                .directory(move |d| {
                    if key.is_empty() {
                        if let Some((world, _)) = prepared {
                            ensure!(
                                d.worlds.contains_key(&world),
                                "prepared native world no longer exists"
                            );
                            Ok((world, None))
                        } else {
                            Ok((d.initial(game)?, None))
                        }
                    } else {
                        Ok((game, Some(d.consume(id, game, keytime, key)?)))
                    }
                })
                .await?;
            *expected = character;
            diagnostics.world = Some(destination);
            a.world = a.world.in_world(destination);
            let body = a.world.call(|w| Ok(w.map_info())).await?;
            enqueue_reply(
                tx,
                Packet {
                    id: wire::MAP_INFO,
                    body,
                },
            )?;
            tracing::info!(
                account = id,
                requested_world = game,
                actual_world = destination,
                "guest authenticated"
            );
        }
        wire::CREATE | wire::LOAD => {
            let _guild = a.guild_actions.lock().await;
            ensure_protocol!(player.is_none(), "character already loaded");
            let account = account
                .as_ref()
                .ok_or_else(|| lifecycle::protocol("HELLO required before character selection"))?
                .id;
            let ch = if packet.id == wire::CREATE {
                ensure_protocol!(
                    expected.is_none(),
                    "transfer requires LOAD of the existing character"
                );
                let kind = r.i16()? as u16;
                r.done()?;
                let class = a.content.get(kind).context(CloseReason::Protocol)?;
                ensure_protocol!(class.class == "Player", "unknown playable class");
                lifecycle::stage(Stage::Store);
                a.store.create(account, class).await?
            } else {
                let id = r.i32()?;
                r.done()?;
                ensure_protocol!(
                    expected.is_none_or(|bound| bound == id),
                    "transfer character mismatch"
                );
                a.store.load(account, id).await.map_err(|error| {
                    if matches!(
                        error.downcast_ref::<rusqlite::Error>(),
                        Some(rusqlite::Error::QueryReturnedNoRows)
                    ) {
                        error.context(CloseReason::Protocol)
                    } else {
                        error
                    }
                })?
            };
            let output = crate::world::Output {
                packets: tx.clone(),
                lost: lost.clone(),
            };
            let vault = if a.world.call(|w| Ok(w.name == "Vault")).await? {
                Some(a.store.vaults(account).await?)
            } else {
                None
            };
            let lists = a.store.account_lists(account).await?;
            let presentation = a.store.presentation(account).await?;
            let prepared = a
                .testing
                .as_ref()
                .map(|t| t.native_entry(account))
                .transpose()?
                .flatten();
            let bind = lifecycle::binding(a, account)?;
            lifecycle::stage(Stage::Publication);
            *player = Some(
                a.world
                    .call(move |w| {
                        if let Some(vault) = vault {
                            w.load_vaults(vault)?;
                        }
                        let id = w.enter_at(
                            ch,
                            Some(output),
                            prepared.map(|(_, pos)| pos),
                            presentation,
                        )?;
                        bind(id)?;
                        w.sync_account_lists(id, Some(lists))?;
                        Ok(id)
                    })
                    .await?,
            );
            if expected.is_none() {
                let id = player.context("guild sign-on player missing")?;
                a.world.directory(move |d| d.guild_sign_on(id)).await?;
            }
            gold::sync(a, account).await?;
            if let Some(testing) = &a.testing {
                testing.native_entered(account)?;
            }
        }
        wire::UPDATE_ACK => {
            r.done()?;
            if a.testing.is_some()
                && let Some(id) = player
            {
                let id = *id;
                a.world
                    .for_player(id)
                    .call(move |w| {
                        w.players
                            .get_mut(&id)
                            .context("native UPDATE_ACK player missing")?
                            .native_update_acked = true;
                        Ok(())
                    })
                    .await?;
            }
        }
        wire::MOVE => {
            let tick = r.i32()?;
            let time = r.i32()?;
            let pos = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            let count = r.i16()?;
            ensure_protocol!((0..=128).contains(&count), "movement history too long");
            for _ in 0..count {
                r.i32()?;
                r.f32()?;
                r.f32()?;
            }
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("MOVE before character"))?;
            a.world
                .call(move |w| {
                    w.acknowledge_tick(id, tick)?;
                    match w.native_move_received(id, pos, (time, received)) {
                        Ok(_) => Ok(()),
                        Err(e) if lifecycle::rule_rejection(&e) => {
                            w.correct_position(id, time, &e.to_string())
                        }
                        Err(error) => Err(error),
                    }
                })
                .await?;
        }
        wire::PLAYER_SHOOT => {
            let time = r.i32()?;
            let bullet = r.u8()?;
            let kind = r.i16()? as u16;
            let pos = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            let angle = r.f32()?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("shoot before character"))?;
            a.world
                .call(move |w| {
                    match w.native_shoot_received(id, (time, received), bullet, kind, pos, angle) {
                        Ok(_) => Ok(()),
                        Err(error)
                            if error
                                .downcast_ref::<crate::world::RejectedNativeShot>()
                                .is_some() =>
                        {
                            w.correct_native_shot(id, time, bullet, kind, &format!("{error:#}"))
                        }
                        Err(error) => Err(error),
                    }
                })
                .await?;
        }
        wire::CREATE_GUILD
        | wire::GUILD_INVITE
        | wire::JOIN_GUILD
        | wire::CHANGE_GUILD_RANK
        | wire::GUILD_REMOVE => {
            let mut shape = NativeReader(Reader::new(&packet.body));
            shape.utf()?;
            if packet.id == wire::CHANGE_GUILD_RANK {
                shape.i32()?;
            }
            shape.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("guild action before character"))?;
            guilds::native(a, id, packet.id, &mut r, tx, received).await?;
        }
        50 if guilds::merchant(
            a,
            player.ok_or_else(|| lifecycle::protocol("purchase before character"))?,
            &packet.body,
        )
        .await? =>
        {
            let id =
                player.ok_or_else(|| lifecycle::protocol("guild purchase before character"))?;
            guilds::native(a, id, packet.id, &mut r, tx, received).await?;
        }
        wire::PLAYER_TEXT => {
            let text = player_text(&mut r).context(CloseReason::Protocol)?;
            if text.trim().eq_ignore_ascii_case("/tutorial") {
                crate::world::validate_chat(&text)?;
                return reconnect(a, session, Reconnect::Tutorial, received).await;
            }
            if let Some((kind, name)) = guilds::chat_command(&text) {
                let id =
                    player.ok_or_else(|| lifecycle::protocol("guild command before character"))?;
                let mut body = Writer::new();
                body.utf(name);
                guilds::native(a, id, kind, &mut Reader::new(&body.0), tx, received).await?;
                return Ok(());
            }
            let guild_chat = text
                .split_whitespace()
                .next()
                .is_some_and(|s| s.eq_ignore_ascii_case("/g") || s.eq_ignore_ascii_case("/guild"));
            // A revoked member must not receive chat between the database commit and cache publication.
            let _guild = if guild_chat {
                Some(a.guild_actions.lock().await)
            } else {
                None
            };
            let id = player.ok_or_else(|| lifecycle::protocol("chat before character"))?;

            if let Some(message) = text
                .trim()
                .strip_prefix("/bug")
                .filter(|s| s.is_empty() || s.starts_with(char::is_whitespace))
            {
                let owner = account
                    .as_ref()
                    .context("authenticated session required")?
                    .id;
                if let Err(error) =
                    diagnostics::report(a, owner, id, message.trim(), diagnostics).await
                {
                    tracing::error!(account=owner, player=id, error=%error, "bug report failed");
                    enqueue_reply(
                        tx,
                        crate::world::chat_info(&format!("Could not submit bug report: {error}")),
                    )?;
                }
            } else if matches!(
                text.trim(),
                "/discord" | "/discord recovery" | "/discord norecovery"
            ) {
                let result = async {
                    ensure!(a.discord_link_token.is_some(), lifecycle::controller("Discord linking is not configured"));
                    let owner = account.as_ref().context("authenticated session required")?;
                    ensure!(owner.registered, lifecycle::controller("sign into your registered account again before Discord account actions"));
                    let owner = owner.id;
                    lifecycle::stage(Stage::Store);
                    let reply = match text.trim() {
                        "/discord recovery" => {
                            let code = a.store.issue_recovery_code(owner).await?;
                            format!("Use /recover code:{code} with your linked Discord account within five minutes. This ENABLES that Discord account to reset your game password. Keep this code private. Use /discord norecovery to revoke permission.")
                        }
                        "/discord norecovery" => {
                            a.store.disable_recovery(owner).await?;
                            "Discord password recovery disabled. All recovery codes and reset links revoked.".into()
                        }
                        _ => {
                            let code = a.store.issue_discord_code(owner).await?;
                            format!("Use /link code:{code} with the official Discord bot within five minutes. This makes your Discord identity visible on hiscores. Keep this code private.")
                        }
                    };
                    lifecycle::stage(Stage::Settled);
                    Ok::<_, anyhow::Error>(reply)
                }.await;
                let reply = match result {
                    Ok(text) => text,
                    Err(error) if lifecycle::rule_rejection(&error) => {
                        lifecycle::stage(Stage::Rejected);
                        error.to_string()
                    }
                    Err(error) => return Err(error),
                };
                enqueue_reply(tx, crate::world::chat_private(&reply))?;
            } else if matches!(
                text.split_whitespace()
                    .next()
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("/ignore" | "/unignore")
            ) {
                let result = async {
                    crate::world::validate_chat(&text)?;
                    a.world
                        .directory(move |d| d.chat_permit(id, received))
                        .await?;
                    let args: Vec<_> = text.split_whitespace().collect();
                    ensure!(
                        args.len() == 2,
                        lifecycle::controller("Usage: /ignore <player> or /unignore <player>")
                    );
                    let owner = account
                        .as_ref()
                        .context("authenticated session required")?
                        .id;
                    let target = a.store.account_named(args[1].into()).await?;
                    let add = args[0].eq_ignore_ascii_case("/ignore");
                    lifecycle::stage(Stage::Store);
                    let lists = a.store.edit_account_list(owner, 1, add, target).await?;
                    lifecycle::stage(Stage::Publication);
                    a.world
                        .call(move |w| w.sync_account_lists(id, Some(lists)))
                        .await?;
                    lifecycle::stage(Stage::Settled);
                    Ok::<_, anyhow::Error>(if add {
                        "Player ignored."
                    } else {
                        "Player unignored."
                    })
                }
                .await;
                let reply = match result {
                    Ok(text) => crate::world::chat_info(text),
                    Err(error) if lifecycle::rule_rejection(&error) => {
                        lifecycle::stage(Stage::Rejected);
                        crate::world::chat_error(&error.to_string())
                    }
                    Err(error) => return Err(error),
                };
                enqueue_reply(tx, reply)?;
            } else if let Err(error) = a.world.directory(move |d| d.chat(id, text, received)).await
            {
                if !lifecycle::rule_rejection(&error) {
                    return Err(error);
                }
                tracing::warn!(player=id, error=%error, "local chat rejected");
                enqueue_reply(tx, crate::world::chat_error(&error.to_string()))?;
            }
        }
        wire::EDIT_ACCOUNT_LIST => {
            let list = r.i32()?;
            let add = r.u8()?;
            ensure_protocol!(add <= 1, "invalid account list operation");
            let target = r.i32()?;
            r.done()?;
            let id =
                player.ok_or_else(|| lifecycle::protocol("account list edit before character"))?;
            let owner = account
                .as_ref()
                .context("authenticated session required")?
                .id;
            let result = async {
                ensure_protocol!((0..=1).contains(&list), "invalid account list");
                let target = a
                    .world
                    .call(move |w| w.account_list_target(id, target))
                    .await?;
                lifecycle::stage(Stage::Store);
                let lists = a
                    .store
                    .edit_account_list(owner, list, add != 0, target)
                    .await?;
                lifecycle::stage(Stage::Publication);
                a.world
                    .call(move |w| w.sync_account_lists(id, Some(lists)))
                    .await?;
                lifecycle::stage(Stage::Settled);
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(error) = result {
                if !lifecycle::rule_rejection(&error) {
                    return Err(error);
                }
                tracing::warn!(player=id, list, target, error=%error, "account list edit rejected");
                // The selected client only adds to ACCOUNTLIST 0; a fresh session clears an optimistic lock.
                if list == 0 && add != 0 {
                    return Err(
                        error.context("Lock change failed; reconnect to refresh your player list")
                    );
                }
                lifecycle::stage(Stage::Publication);
                a.world
                    .call(move |w| w.sync_account_lists(id, None))
                    .await?;
                lifecycle::stage(Stage::Settled);
                enqueue_reply(tx, crate::world::chat_error(&error.to_string()))?;
            }
        }
        wire::BUY => {
            let merchant = r.i32()?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("purchase before character"))?;
            let registered = account
                .as_ref()
                .ok_or_else(|| lifecycle::protocol("purchase before HELLO"))?
                .registered;
            lifecycle::stage(Stage::Reservation);
            let plan = if registered {
                a.world.call(move |w| w.buy(id, merchant)).await
            } else {
                Err(lifecycle::controller(
                    "Register and sign in before buying items.",
                ))
            };
            match plan {
                Ok(plan) => persist(a, plan).await?,
                Err(error) if lifecycle::rule_rejection(&error) => {
                    lifecycle::stage(Stage::Rejected);
                    tracing::warn!(player=id, merchant, error=%error, "purchase rejected");
                    enqueue_reply(tx, crate::world::shop::reply(false, &error.to_string()))?;
                }
                Err(error) => return Err(error),
            }
        }
        wire::TELEPORT => {
            let target = r.i32()?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("teleport before character"))?;
            if let Err(error) = a
                .world
                .directory(move |d| d.teleport(id, target, received))
                .await
            {
                if !lifecycle::rule_rejection(&error) {
                    return Err(error);
                }
                tracing::warn!(player=id, target, error=%error, "teleport rejected");
                enqueue_reply(tx, crate::world::chat_error(&error.to_string()))?;
            }
        }
        wire::PONG => {
            let nonce = r.i32()?;
            r.i32()?;
            r.done()?;
            diagnostics.pong(nonce);
            if let (Some(testing), Some(player)) = (&a.testing, player) {
                testing.acknowledge_native(nonce, *player);
            }
        }
        64 => {
            let time = r.i32()?;
            let pos = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("ground damage before character"))?;
            a.world
                .call(move |w| w.ground_damage(id, time, pos))
                .await?;
        }
        22 => {
            let time = r.i32()?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("SHOOT ACK before character"))?;
            a.world.call(move |w| w.acknowledge_shoot(id, time)).await?;
        }
        76 => {
            r.i32()?;
            r.u8()?;
            r.i32()?;
            r.u8()?;
            r.done()?;
        }
        24 => {
            let bullet = r.u8()?;
            let owner = r.i32()?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("hit report before character"))?;
            a.world
                .call(move |w| w.native_player_hit(id, owner, bullet))
                .await?;
        }
        59 => {
            player.ok_or_else(|| lifecycle::protocol("area acknowledgement before character"))?;
            r.i32()?;
            r.f32()?;
            r.f32()?;
            r.done()?;
        }
        66 | 51 => collision_report(packet.id == 66, &mut r).context(CloseReason::Protocol)?,
        30 => {
            let time = r.i32()?;
            let s = slot(&mut r).context(CloseReason::Protocol)?;
            let pos = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("item use before character"))?;
            lifecycle::stage(Stage::Reservation);
            match a
                .world
                .call(move |w| w.native_use_item(id, time, s, pos))
                .await
            {
                Ok(Some(plan)) => persist(a, plan).await?,
                Ok(None) => {}
                Err(error) if lifecycle::rule_rejection(&error) => {
                    lifecycle::stage(Stage::Rejected);
                    tracing::warn!(player=id, item=s.kind, slot=s.slot, error=%error, "item use rejected");
                    enqueue_reply(tx, crate::world::chat_error(&error.to_string()))?;
                    let mut out = Writer::new();
                    out.i32(1);
                    enqueue_reply(tx, Packet { id: 4, body: out.0 })?;
                }
                Err(error) => return Err(error),
            }
        }
        21 => {
            let name = r.utf()?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("trade request before character"))?;
            a.world.call(move |w| w.request_trade(id, name)).await?;
        }
        1 => {
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("trade cancel before character"))?;
            a.world
                .call(move |w| {
                    w.cancel_trade(id, "Trade cancelled.");
                    Ok(())
                })
                .await?;
        }
        37 => {
            let values = trade_offer(&mut r).context(CloseReason::Protocol)?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("trade change before character"))?;
            a.world.call(move |w| w.change_trade(id, values)).await?;
        }
        57 => {
            let mine = trade_offer(&mut r).context(CloseReason::Protocol)?;
            let theirs = trade_offer(&mut r).context(CloseReason::Protocol)?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("trade accept before character"))?;
            lifecycle::stage(Stage::Reservation);
            if let Some(plan) = a
                .world
                .call(move |w| w.accept_trade(id, mine, theirs))
                .await?
            {
                persist_trade(a, plan).await?;
            }
        }
        35 => {
            let slot = slot(&mut r).context(CloseReason::Protocol)?;
            r.done()?;
            let id =
                player.ok_or_else(|| lifecycle::protocol("inventory drop before character"))?;
            lifecycle::stage(Stage::Reservation);
            let plan = a.world.call(move |w| w.drop_item(id, slot)).await?;
            persist(a, plan).await?;
        }
        65 => {
            r.i32()?;
            r.f32()?;
            r.f32()?;
            let from = slot(&mut r).context(CloseReason::Protocol)?;
            let to = slot(&mut r).context(CloseReason::Protocol)?;
            r.done()?;
            let id =
                player.ok_or_else(|| lifecycle::protocol("inventory swap before character"))?;
            lifecycle::stage(Stage::Reservation);
            let plan = a.world.call(move |w| w.swap(id, from, to)).await?;
            persist(a, plan).await?;
        }
        14 => {
            let time = r.i32()?;
            r.done()?;
            let id = player.ok_or_else(|| lifecycle::protocol("GOTO ACK before character"))?;
            a.world.call(move |w| w.acknowledge_goto(id, time)).await?;
        }
        48 => {
            r.done()?;
        }
        33 => {
            let name = r.utf()?;
            r.done()?;
            let account = account
                .as_ref()
                .ok_or_else(|| lifecycle::protocol("name before HELLO"))?
                .id;
            let _gold = a.gold_updates.lock().await;
            lifecycle::stage(Stage::Reservation);
            let reservation = if let Some(id) = *player {
                match a
                    .world
                    .for_player(id)
                    .call(move |w| w.begin_name_choice(id, account))
                    .await?
                {
                    Ok(plan) => Some((id, plan)),
                    Err(reason) => {
                        lifecycle::stage(Stage::Rejected);
                        tracing::info!(account, reason, "name choice rejected before reservation");
                        let mut o = Writer::new();
                        o.u8(0).utf(reason);
                        enqueue_reply(tx, Packet { id: 20, body: o.0 })?;
                        return Ok(());
                    }
                }
            } else {
                None
            };
            lifecycle::stage(Stage::Store);
            let result = a
                .store
                .name_choice(account, name.clone(), reservation.is_some())
                .await;
            lifecycle::stage(Stage::Publication);
            let rejection = if let Some((id, plan)) = reservation {
                a.world
                    .for_player(id)
                    .call(move |w| w.finish_name_choice(plan, name, result))
                    .await?
            } else {
                match result? {
                    crate::store::NameChoice::Rejected(reason) => Some(reason),
                    crate::store::NameChoice::Committed { .. } => None,
                }
            };
            lifecycle::stage(Stage::Settled);
            if let Some(reason) = rejection {
                tracing::info!(account, reason, "name choice rejected without charge");
            }
            let mut o = Writer::new();
            o.u8(u8::from(rejection.is_none()))
                .utf(rejection.unwrap_or(""));
            enqueue_reply(tx, Packet { id: 20, body: o.0 })?;
        }
        42 | 3 => {
            let _guild = a.guild_actions.lock().await;
            let portal = if packet.id == 3 { Some(r.i32()?) } else { None };
            r.done()?;
            player.ok_or_else(|| lifecycle::protocol("transfer before character"))?;
            let guild_portal = a
                .world
                .call(move |w| {
                    Ok(portal
                        .and_then(|p| w.entities.get(&p))
                        .is_some_and(|e| e.kind == 0x72f))
                })
                .await?;
            if guild_portal {
                let guild = a
                    .store
                    .guild(
                        account
                            .as_ref()
                            .ok_or_else(|| lifecycle::protocol("guild portal before HELLO"))?
                            .id,
                    )
                    .await?;
                a.world.directory(move |d| d.guild_instance(guild)).await?;
            }
            reconnect(a, session, Reconnect::Portal(portal), received).await?;
        }
        _ => {
            return Err(anyhow::anyhow!(CloseReason::Protocol).context(format!(
                "packet {} is not implemented in this development build",
                packet.id
            )));
        }
    }
    Ok(())
}

enum Reconnect {
    Portal(Option<i32>),
    Tutorial,
}

async fn reconnect(
    a: &App,
    session: &mut Session,
    request: Reconnect,
    received: std::time::Instant,
) -> Result<()> {
    let id = session.player.context("transfer before character")?;
    let transfer = a
        .world
        .directory(move |d| {
            let (account, character, destination) = match request {
                Reconnect::Tutorial => {
                    d.chat_permit(id, received)?;
                    let p = &d.worlds[&d.location(id)?].players[&id];
                    (p.character.account, p.character.id, d.initial(-1)?)
                }
                Reconnect::Portal(portal) => {
                    if portal.is_none() && d.location(id)? == -2 {
                        tracing::debug!(
                            player = id,
                            "redundant Nexus escape acknowledged without transfer"
                        );
                        return Ok(None);
                    }
                    d.destination(id, portal)?
                }
            };
            let world = &d.worlds[&destination];
            if world.players.len() >= world.max_players {
                let message = crate::world::chat_info("Destination is full.");
                crate::world::send(
                    &d.worlds[&d.location(id)?].players[&id],
                    message.id,
                    message.body,
                );
                return Ok(None);
            }
            let (keytime, key, name) = d.issue(account, character, destination)?;
            Ok(Some((destination, keytime, key, name)))
        })
        .await?;
    let Some((destination, keytime, key, name)) = transfer else {
        return Ok(());
    };
    let mut o = Writer::new();
    o.utf(&name)
        .utf("")
        .i32(2050)
        .i32(destination)
        .i32(keytime)
        .i16(key.len() as i16);
    o.0.extend(key);
    session.transfer = Some(Packet { id: 39, body: o.0 });
    tracing::debug!(
        player = id,
        destination,
        "transfer prepared; retirement required before reconnect"
    );
    Ok(())
}

async fn connection(
    socket: WebSocket,
    a: App,
    user_agent: Option<String>,
    browser: Option<Arc<Mutex<diagnostics::Browser>>>,
) {
    let (wsout, wsin) = socket.split();
    connection_io(wsout, wsin, a, user_agent, browser).await;
}
async fn send_frame<S: futures_util::Sink<Message> + Unpin>(
    output: &mut S,
    message: Message,
    lost: &mut tokio::sync::watch::Receiver<bool>,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
    cancel: Option<tokio::sync::watch::Receiver<bool>>,
    drain: &mut tokio::sync::watch::Receiver<u64>,
) -> std::result::Result<(), CloseReason> {
    tokio::select! {
        biased;
        _ = drain.changed() => Err(CloseReason::Shutdown),
        _ = async { if !*lost.borrow() { let _ = lost.changed().await; } } => Err(CloseReason::Output),
        _ = async { if !*shutdown.borrow() { let _ = shutdown.changed().await; } } => Err(CloseReason::Shutdown),
        _ = async { match cancel { Some(mut cancel) => { if !*cancel.borrow() { let _ = cancel.changed().await; } }, None => std::future::pending::<()>().await } } => Err(CloseReason::Controller),
        sent = tokio::time::timeout(std::time::Duration::from_secs(5), output.send(message)) => match sent {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(CloseReason::Transport),
            Err(_) => Err(CloseReason::Deadline),
        }
    }
}

async fn connection_io<S, I>(
    mut wsout: S,
    wsin: I,
    mut a: App,
    user_agent: Option<String>,
    browser: Option<Arc<Mutex<diagnostics::Browser>>>,
) where
    S: futures_util::Sink<Message> + Unpin,
    I: futures_util::Stream<Item = std::result::Result<Message, axum::Error>>
        + Send
        + Unpin
        + 'static,
{
    let mut drain = lifecycle::drain::stopping(&a);
    let Ok(_connection) = lifecycle::drain::entry(&a) else {
        return;
    };
    let (tx, mut rx) = mpsc::channel::<Packet>(64);
    let (lost, mut lost_rx) = tokio::sync::watch::channel(false);
    // Actor work and output backpressure must not manufacture late, clustered input receipts.
    let (mut incoming, input_task) = input::receive(wsin, lost.clone(), a.world.clock.clone());
    let mut shutdown = a.shutdown.subscribe();
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(10));
    tokio::pin!(deadline);
    let mut codec = Codec::server();
    let mut session = Session {
        runtime: Some(tokio::runtime::Handle::current()),
        ..Session::default()
    };
    session.diagnostics.user_agent = user_agent;
    session.diagnostics.browser = browser;
    let mut reconnect_sent = false;
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(5));
    let mut serial = 0;
    let mut transfers = tokio::time::interval(std::time::Duration::from_millis(250));
    let mut testing_stop = a.testing.as_ref().map(|t| t.stopped.subscribe());
    let mut terminal_drain = false;
    let mut close_reason = CloseReason::Transport;
    macro_rules! send {
        ($message:expr) => {
            send_frame(
                &mut wsout,
                $message,
                &mut lost_rx,
                &mut shutdown,
                session.account.as_ref().and_then(|o| o.cancel.clone()),
                &mut drain,
            )
            .await
            .map_err(|reason| {
                close_reason = reason;
            })
        };
    }
    'connection: loop {
        if *lost_rx.borrow() {
            close_reason = CloseReason::Output;
            break;
        }
        if *shutdown.borrow()
            || drain.has_changed().expect("live admission sender lost")
            || !lifecycle::drain::open(&a)
        {
            close_reason = CloseReason::Shutdown;
            break;
        }
        if session
            .account
            .as_ref()
            .and_then(|o| o.cancel.as_ref())
            .is_some_and(|c| *c.borrow())
        {
            close_reason = CloseReason::Controller;
            break;
        }
        tokio::select! {
            _=async { match testing_stop.as_mut() { Some(stop) => { if stop.borrow().is_none() { let _ = stop.changed().await; } }, None => std::future::pending::<()>().await } } => {
                let player=session.player;
                let terminal=a.world.directory(move |d|Ok(d.worlds.values().flat_map(|w|w.players.iter()).any(|(id,p)|Some(*id)==player && (!p.character.alive || p.death_pending)))).await;
                match terminal {
                    // Ruffle can discard its queued DEATH frame if this socket closes first.
                    Ok(true) => {
                        terminal_drain=true;testing_stop=None;
                        deadline.as_mut().reset(tokio::time::Instant::now()+std::time::Duration::from_secs(5));
                    }
                    Ok(false) => {tracing::warn!("native test control stopped; closing living game connection");break;}
                    Err(error) => {tracing::error!(%error,"terminal native drain inspection failed");break;}
                }
            },
            event=incoming.recv()=>match event {
                Some(Incoming::Command(packet, received, _bytes))=>{
                    if incoming.is_closed() { break; }
                    if a.testing.as_ref().is_some_and(|t| t.stopped.borrow().is_some()) { if terminal_drain {continue;} break; }
                    if !session.reconnecting {
                        let id=packet.id;
                        session.diagnostics.packets_in += 1;
                        session.diagnostics.processed(id);
                        let handling = std::time::Instant::now();
                        let queued=a.world.clock.now().saturating_duration_since(received);
                        let queue_before=incoming.len();
                        let requests=Arc::new(Mutex::new(crate::performance::Requests::default()));
                        let result=crate::performance::REQUEST.scope(requests.clone(),handle(&mut a,packet,&mut session,&tx,&lost,received)).await;
                        session.diagnostics.handled(diagnostics::CommandSample {packet:id,queued,handling:handling.elapsed(),queue_before,queue_after:incoming.len(),failed:result.is_err(),requests:requests.lock().expect("request timings poisoned").clone()});
                        if let Err(e)=result {
                            let rejected = lifecycle::rule_rejection(&e);
                            close_reason=if e.is::<ReplyQueueError>() { CloseReason::Output } else if rejected { CloseReason::Controller } else { e.downcast_ref::<CloseReason>().copied().unwrap_or(CloseReason::Internal) };
                            if close_reason == CloseReason::Internal && let Some(owner) = &session.account {
                                owner.failed(format!("unclassified native command failure: {e:#}"));
                            }
                            tracing::error!(account=session.account.as_ref().map(|o|o.id),player=session.player,packet=id,error=%format_args!("{e:#}"),"game command rejected");
                            if e.is::<ReplyQueueError>() { close_reason=CloseReason::Output;break 'connection; }
                            if e.downcast_ref::<CloseReason>() == Some(&CloseReason::Protocol) {
                                close_reason=CloseReason::Protocol;
                                let f=fail(&e.to_string());
                                let _=send!(Message::Binary(codec.frame(f.id,f.body).into()));
                                break 'connection;
                            }
                            if rejected && matches!(id,1|21|37|57) && let Some(player) = session.player {
                                let reason = e.to_string();
                                let closed = a.world.for_player(player).call(move |w| Ok(w.cancel_trade(player, &reason))).await;
                                match closed { Ok(true) => {}, Ok(false) => { let mut out = Writer::new(); out.i32(1).utf(&e.to_string()); if send!(Message::Binary(codec.frame(12,out.0).into())).is_err() {break 'connection;} }, Err(error) => { tracing::error!(%error,"trade rejection notification failed"); break 'connection; } }
                                continue;
                            }
                            if rejected && matches!(id,30|35|65) {
                                let mut o=Writer::new();o.i32(1);
                                if send!(Message::Binary(codec.frame(4,o.0).into())).is_err(){break 'connection;}
                                continue;
                            }
                            let f=fail(&e.to_string());
                            let _=send!(Message::Binary(codec.frame(f.id,f.body).into()));
                            break 'connection;
                        }
                    }
                },
                Some(Incoming::Ping(bytes))=>{if send!(Message::Pong(bytes)).is_err(){break}},
                None=>break,
            },
            _=transfers.tick(),if session.player.is_some() && !session.reconnecting=>{
                let player = session.player.expect("guarded loaded player");
                match automatic_transfer(&a, player).await {
                    Ok(Some(transfer)) => {
                        if transfer.player!=player || !transfer.output.packets.same_channel(&tx) {
                            tracing::error!(player,"automatic transfer recipient differs from owned session");
                            break;
                        }
                        session.player = None;
                        session.reconnecting = true;
                        session.account.take();
                        let packet = transfer.packet();
                        // Retired-world frames must not follow RECONNECT into a replacement scene.
                        while rx.try_recv().is_ok() {}
                        if send!(Message::Binary(codec.frame(packet.id,packet.body).into())).is_err(){break;}
                        reconnect_sent=true;
                        deadline.as_mut().reset(tokio::time::Instant::now()+std::time::Duration::from_secs(10));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::error!(player,%error,"automatic durable transfer failed");
                        let f=fail(&error.to_string());
                        let _=send!(Message::Binary(codec.frame(f.id,f.body).into()));
                        break;
                    }
                }
            },
            _=async {
                match session.account.as_ref().and_then(|o| o.cancel.clone()) {
                    Some(mut cancel) => { if !*cancel.borrow() { let _ = cancel.changed().await; } }
                    None => std::future::pending::<()>().await,
                }
            }=>{tracing::info!(account=session.account.as_ref().map(|o|o.id),"closing requested game session");break;},
            _=shutdown.changed()=>{close_reason=CloseReason::Shutdown;break;},
            _=drain.changed()=>{close_reason=CloseReason::Shutdown;break;},
            _=lost_rx.changed()=>{close_reason=CloseReason::Output;tracing::warn!(player=session.player,"closing slow game connection before its state diverges");break;},
            _=&mut deadline,if terminal_drain || session.player.is_none() && (!session.reconnecting || reconnect_sent)=>{close_reason=CloseReason::Deadline;tracing::warn!(reconnecting=session.reconnecting,reconnect_sent,terminal_drain,"game selection, reconnect-close or terminal-drain deadline exceeded");break;},
            outgoing=rx.recv()=>{
                let Some(packet)=outgoing else {break};
                let reconnect=packet.id==39;
                session.diagnostics.queue_high_water = session.diagnostics.queue_high_water.max(rx.len()+1);
                let nonce = (packet.id==wire::PING && packet.body.len()==4).then(|| i32::from_be_bytes(packet.body[..4].try_into().unwrap()));
                let frame = codec.frame(packet.id,packet.body);
                if let (Some(testing),Some(player),Some(nonce)) = (&a.testing,session.player,nonce) { testing.note_native_frame(nonce,player,&frame); }
                let sending = std::time::Instant::now();
                if send!(Message::Binary(frame.into())).is_err(){break;}
                session.diagnostics.packets_out += 1;
                session.diagnostics.last_socket_send=Some(std::time::Instant::now());
                session.diagnostics.max_send_ms = session.diagnostics.max_send_ms.max(sending.elapsed().as_secs_f64()*1000.0);
                // Ruffle can discard unread TCP bytes when the proxy closes before its next frame.
                if reconnect {reconnect_sent=true;deadline.as_mut().reset(tokio::time::Instant::now()+std::time::Duration::from_secs(10));}
            },
            _=ping.tick()=>{if session.account.is_some() && !a.world.clock.paused(){serial+=1;let mut o=Writer::new();o.i32(serial);
                session.diagnostics.sent(serial);
                if send!(Message::Binary(codec.frame(wire::PING,o.0).into())).is_err(){break;}}}
        }
    }
    input_task.abort();
    // Publish custody before diagnostics; neither a cancelled socket task nor its sink owns cleanup.
    let finalization = session
        .account
        .take()
        .map(|owner| owner.close(&a, close_reason));
    if let Some(id) = session.player {
        let diagnostic = diagnostics::disconnect(
            &a,
            id,
            &session.diagnostics,
            serde_json::json!({"reason":close_reason.to_string(),
            "input_queue_packets":incoming.len(),"output_queue_packets":rx.len(),"last_frame_age_ms":input_task.last_frame_age_ms()}),
        );
        if std::panic::AssertUnwindSafe(diagnostic)
            .catch_unwind()
            .await
            .is_err()
        {
            eprintln!(
                "native disconnect diagnostics panicked; finalization remains owned for avatar={id}"
            );
        }
    }
    if let Some(result) = finalization {
        let result = match result {
            Ok(Some(finalization)) => finalization.wait().await,
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            tracing::error!(player=?session.player,error=%error,"disconnect finalization failed; account fenced");
        }
    }
}
pub(crate) async fn automatic_transfer(
    a: &App,
    id: i32,
) -> Result<Option<crate::world::ForcedTransfer>> {
    let transfer = a.world.directory(move |d| d.forced_transfer(id)).await?;
    if transfer.is_some() {
        depart(a, id).await?;
    }
    Ok(transfer)
}
pub(crate) async fn depart(a: &App, id: i32) -> Result<()> {
    // Legacy owned adapter continuations retain custody until their Phase2 conversion.
    if lifecycle::nested_operation() {
        return lifecycle::player(a, id, "departure", move |a| async move {
            depart_inner(&a, id).await
        })
        .await;
    }
    if !a
        .owners
        .lock()
        .expect("ownership registry poisoned")
        .values()
        .any(|c| c.matches_avatar(id))
    {
        lifecycle::player(a, id, "departure_binding", |_| async { Ok(()) }).await?;
    }
    lifecycle::finalize(a, id, CloseReason::Controller)?
        .wait()
        .await
}
enum DepartureStep {
    Save(crate::store::Character),
    Progress(crate::store::Character),
    Death(crate::store::Character, String),
    Waiting,
}
async fn depart_inner(a: &App, id: i32) -> Result<()> {
    loop {
        lifecycle::stage(Stage::Reservation);
        let step = a
            .world
            .for_player(id)
            .call(move |w| {
                if let Some(ch) = w.begin_departure(id)? {
                    return Ok(DepartureStep::Save(ch));
                }
                // Departure already owns this generation, so the collector cannot admit its prerequisites.
                if let Some((_, snapshot)) =
                    w.pending_progress_where(|player, _| player == id).pop()
                {
                    return Ok(DepartureStep::Progress(snapshot));
                }
                if let Some((_, ch, killer)) =
                    w.pending_deaths_where(|player, _| player == id).pop()
                {
                    return Ok(DepartureStep::Death(ch, killer));
                }
                Ok(DepartureStep::Waiting)
            })
            .await?;
        let ch = match step {
            DepartureStep::Progress(snapshot) => {
                persist_progress_inner(a, id, snapshot).await?;
                continue;
            }
            DepartureStep::Death(ch, killer) => {
                persist_death_inner(a, id, ch, killer).await?;
                continue;
            }
            DepartureStep::Waiting => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                continue;
            }
            DepartureStep::Save(ch) => ch,
        };
        let retirement = lifecycle::retirement(a, ch.account, id)?;
        lifecycle::stage(Stage::Store);
        let result = if ch.alive {
            a.store
                .progress(ch)
                .await
                .context("departure persistence failed")
        } else {
            Ok(())
        };
        lifecycle::stage(Stage::Publication);
        a.world
            .for_player(id)
            .call(move |w| {
                w.committed(id, result)?;
                w.finish_departure(id)
            })
            .await?;
        lifecycle::stage(Stage::Retirement);
        retirement()?;
        lifecycle::stage(Stage::Settled);
        return Ok(());
    }
}
#[derive(Deserialize)]
struct BotCreate {
    guid: String,
    world: i32,
}
async fn bot_create(
    State(a): State<App>,
    headers: HeaderMap,
    Json(input): Json<BotCreate>,
) -> Response {
    let result: Result<serde_json::Value> = async {
        control(&a, &headers)?;
        let acc = a.store.list(input.guid).await?;
        let mut owner = a.claim(acc.id)?;
        let account = acc.id;
        lifecycle::account(&a, vec![account], "script_entry", move |a| async move {
            owner.arm_cleanup(&a, tokio::runtime::Handle::current());
            lifecycle::stage(Stage::Store);
            let ch = if acc.characters.is_empty() {
                a.store.create(acc.id, a.content.get(WIZARD)?).await?
            } else {
                ensure!(acc.characters.len() == 1, "unexpected character count");
                acc.characters[0].clone()
            };
            let bind = lifecycle::binding(&a, acc.id)?;
            lifecycle::stage(Stage::Reservation);
            let destination = a.world.directory(move |d| d.initial(input.world)).await?;
            lifecycle::stage(Stage::Store);
            let presentation = a.store.presentation(acc.id).await?;
            lifecycle::stage(Stage::Publication);
            let id = a
                .world
                .in_world(destination)
                .call(move |w| {
                    let id = w.enter_at(ch, None, None, presentation)?;
                    bind(id)?;
                    Ok(id)
                })
                .await?;
            let mut bots = a.bots.lock().expect("bot ownership registry poisoned");
            ensure!(
                !bots.contains_key(&id),
                "script avatar already has a controller owner"
            );
            owner.publish_script(id)?;
            bots.insert(id, owner);
            lifecycle::stage(Stage::Settled);
            Ok(id)
        })
        .await
        .map(|id| serde_json::json!({"player":id}))
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
fn control(a: &App, headers: &HeaderMap) -> Result<()> {
    ensure!(
        !*a.shutdown.borrow() && lifecycle::drain::open(a),
        "server admission is frozen"
    );
    let token = a.control.as_ref().context("script control is disabled")?;
    ensure!(
        headers.get("authorization").and_then(|v| v.to_str().ok())
            == Some(format!("Bearer {token}").as_str()),
        "invalid control token"
    );
    Ok(())
}
#[derive(Deserialize)]
struct BotCommand {
    player: i32,
    action: Action,
}
async fn bot_action(
    State(a): State<App>,
    headers: HeaderMap,
    Json(input): Json<BotCommand>,
) -> Response {
    let result: Result<serde_json::Value> = async {
        control(&a, &headers)?;
        let id = input.player;
        inventory(&a, id, move |w| w.action(id, input.action)).await??;
        a.world.for_player(id).call(move |w| w.observe(id)).await
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
struct BotTrade {
    player: i32,
    action: TradeAction,
}
async fn bot_trade(
    State(a): State<App>,
    headers: HeaderMap,
    Json(input): Json<BotTrade>,
) -> Response {
    let result: Result<serde_json::Value> = async {
        control(&a, &headers)?;
        let id = input.player;
        lifecycle::player(&a, id, "trade", move |a| async move {
            lifecycle::stage(Stage::Reservation);
            let prepared = a
                .world
                .for_player(id)
                .call(move |w| Ok(w.script_trade(id, input.action)))
                .await?;
            match prepared {
                Ok(Some(plan)) => persist_trade(&a, plan).await,
                Ok(None) => Ok(()),
                Err(error) => {
                    if lifecycle::rule_rejection(&error) {
                        lifecycle::stage(Stage::Rejected);
                    }
                    Err(error)
                }
            }
        })
        .await?;
        a.world.for_player(id).call(move |w| w.observe(id)).await
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
struct ScriptStep {
    at_ms: u64,
    action: Option<Action>,
}
#[derive(Deserialize)]
struct Script {
    player: i32,
    steps: Vec<ScriptStep>,
}
async fn script(State(a): State<App>, headers: HeaderMap, Json(input): Json<Script>) -> Response {
    let result: Result<Vec<serde_json::Value>> = async {
        control(&a, &headers)?;
        ensure!(
            !input.steps.is_empty() && input.steps.len() <= 512,
            "script needs 1–512 steps"
        );
        let mut previous = 0;
        for s in &input.steps {
            ensure!(
                s.at_ms >= previous && s.at_ms <= 120000,
                "script times must be ordered and at most 120 seconds"
            );
            previous = s.at_ms;
        }
        let start = tokio::time::Instant::now();
        let mut shutdown=a.shutdown.subscribe();
        let mut drain=lifecycle::drain::stopping(&a);
        let mut frames = Vec::new();
        for step in input.steps {
            tokio::select!{_=tokio::time::sleep_until(start+std::time::Duration::from_millis(step.at_ms))=>{},_=shutdown.changed()=>bail!("server is shutting down"),_=drain.changed()=>bail!("server admission is frozen")};
            let id = input.player;
            if let Some(action) = step.action {
                inventory(&a, id, move |w| w.action(id, action)).await??;
            }
            frames.push(a.world.for_player(id).call(move |w| w.observe(id)).await?);
        }
        Ok(frames)
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
struct MapView {
    player: Option<i32>,
}
async fn map(State(a): State<App>, headers: HeaderMap, Query(view): Query<MapView>) -> Response {
    let world = match view.player {
        Some(id) => a.world.for_player(id),
        None => a.world.in_world(-1),
    };
    let result: Result<serde_json::Value> = async {
        control(&a, &headers)?;
        world.call(move |w| w.inspect_map(view.player)).await
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
struct BotPlayer {
    player: i32,
}
async fn observe(
    State(a): State<App>,
    headers: HeaderMap,
    Json(input): Json<BotPlayer>,
) -> Response {
    let result: Result<serde_json::Value> = async {
        control(&a, &headers)?;
        a.world
            .directory(move |d| {
                let scope = d.location(input.player)?;
                let mut frame = d.worlds[&scope].observe(input.player)?;
                frame["player_view"] = serde_json::to_value(d.player_view(input.player)?)?;
                Ok(frame)
            })
            .await
    }
    .await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
async fn bot_leave(
    State(a): State<App>,
    headers: HeaderMap,
    Json(input): Json<BotPlayer>,
) -> Response {
    let result: Result<()> = async {
        control(&a, &headers)?;
        if let Some(finalization) = close_script(&a, input.player)? {
            finalization.wait().await?;
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => Json(serde_json::json!({"saved":true})).into_response(),
        Err(e) => error(e),
    }
}
fn close_script(a: &App, player: i32) -> Result<Option<lifecycle::Finalized>> {
    // Serialize map custody with registry publication, so a duplicate cannot see neither.
    let mut bots = a.bots.lock().expect("bot ownership registry poisoned");
    match bots.remove(&player) {
        Some(owner) => owner.close(a, CloseReason::Controller),
        None => lifecycle::join_script(a, player).map(Some),
    }
}
fn close_scripts(a: &App) -> Result<Vec<lifecycle::Finalized>> {
    let mut bots = a.bots.lock().expect("bot ownership registry poisoned");
    let mut completions = lifecycle::script_finalizations(a);
    for id in bots.keys().copied().collect::<Vec<_>>() {
        if let Some(completion) = bots
            .remove(&id)
            .expect("script owner")
            .close(a, CloseReason::Shutdown)?
        {
            completions.push(completion);
        }
    }
    Ok(completions)
}

pub(crate) async fn persistence_tick(a: &App) -> Result<()> {
    let app = a.clone();
    let progress = a
        .world
        .directory(move |d| {
            let mut owned = BTreeMap::new();
            let mut failure = None;
            let plans: Vec<_> = d
                .worlds
                .values_mut()
                .flat_map(|w| {
                    w.pending_progress_where(|id, account| {
                        match lifecycle::reserve_player(&app, account, id, "progress") {
                            Ok(Some(guard)) => {
                                owned.insert(id, guard);
                                true
                            }
                            Ok(None) => false,
                            Err(error) => {
                                failure = Some(error);
                                false
                            }
                        }
                    })
                })
                .collect();
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(plans
                .into_iter()
                .map(|(id, snapshot)| {
                    (
                        owned.remove(&id).expect("reserved progress owner"),
                        id,
                        snapshot,
                    )
                })
                .collect::<Vec<_>>())
        })
        .await?;
    for (guard, id, snapshot) in progress {
        if let Err(error) = lifecycle::complete(guard, move |a| async move {
            persist_progress_inner(&a, id, snapshot).await
        })
        .await
        {
            tracing::error!(player=id,error=%format_args!("{error:#}"),"progress completion failed");
        }
    }
    let app = a.clone();
    let pending = a
        .world
        .directory(move |d| {
            let mut owned = BTreeMap::new();
            let mut failure = None;
            let plans: Vec<_> = d
                .worlds
                .values_mut()
                .flat_map(|w| {
                    w.pending_deaths_where(|id, account| {
                        match lifecycle::reserve_player(&app, account, id, "death") {
                            Ok(Some(guard)) => {
                                owned.insert(id, guard);
                                true
                            }
                            Ok(None) => false,
                            Err(error) => {
                                failure = Some(error);
                                false
                            }
                        }
                    })
                })
                .collect();
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(plans
                .into_iter()
                .map(|(id, ch, killer)| {
                    (
                        owned.remove(&id).expect("reserved death owner"),
                        id,
                        ch,
                        killer,
                    )
                })
                .collect::<Vec<_>>())
        })
        .await?;
    for (guard, id, ch, killer) in pending {
        if let Err(error) = lifecycle::complete(guard, move |a| async move {
            persist_death_inner(&a, id, ch, killer).await
        })
        .await
        {
            tracing::error!(player=id,error=%format_args!("{error:#}"),"death completion failed");
        }
    }
    Ok(())
}
fn client_assets(client: &std::path::Path) -> Router<App> {
    let mut router = Router::new();
    for (route, file) in [
        ("/", "index.html"),
        ("/index.html", "index.html"),
        ("/launcher.js", "launcher.js"),
        ("/draw.html", "draw.html"),
        ("/native-texture-editor.swf", "native-texture-editor.swf"),
    ] {
        router = router.route_service(
            route,
            tower_http::services::ServeFile::new(client.join(file)),
        );
    }
    router.nest_service(
        "/vendor",
        tower_http::services::ServeDir::new(client.join("vendor")),
    )
}

#[cfg(test)]
mod client_assets_tests;

fn routes(a: &App, client: PathBuf) -> Router {
    let app = Router::new()
        .route(
            "/legacy-api/picture/save",
            post(pictures::save).layer(DefaultBodyLimit::max(24 * 1024)),
        )
        .route(
            "/legacy-api/picture/get",
            post(pictures::get_picture).layer(DefaultBodyLimit::max(2048)),
        )
        .route(
            "/legacy-api/picture/{operation}",
            post(pictures::request).layer(DefaultBodyLimit::max(2048)),
        )
        .route("/legacy-api/char/list", post(chars))
        .route("/legacy-api/char/fame", post(fame))
        .route("/legacy-api/char/delete", post(delete_character))
        .route("/legacy-api/account/{operation}", post(account_request))
        .route("/legacy-api/fame/list", post(legends))
        .route("/legacy-api/guild/{operation}", post(guilds::request))
        .nest_service("/sfx", tower_http::services::ServeDir::new(a.sfx.clone()))
        .route("/client.swf", get(swf))
        .route("/game", get(websocket))
        .route(
            "/api/diagnostics/{key}",
            post(diagnostics::browser_sample).layer(DefaultBodyLimit::max(4096)),
        )
        .route(
            "/api/bug-reports/{id}/diagnostics.json",
            get(diagnostics::attachment),
        )
        .route("/internal/discord/bugs", get(diagnostics::pending))
        .route("/internal/discord/bugs/{id}", post(diagnostics::receipt))
        .route("/api/updates", get(updates::feed))
        .route(
            "/internal/discord/updates",
            post(updates::sync).layer(DefaultBodyLimit::max(4 * 1024 * 1024)),
        )
        .route("/api/hiscores", get(hiscores))
        .route("/api/collection/{account}", get(collection))
        .route("/internal/discord/announce", post(discord_announce))
        .route("/internal/discord/link", post(discord_link))
        .route("/internal/discord/kick", post(discord_kick))
        .route("/internal/discord/recover", post(discord_recover))
        .route("/api/account/recover", post(reset_password));
    app.route("/dev/testing", post(testing::command))
        .route("/dev/join", post(bot_create))
        .route("/dev/action", post(bot_action))
        .route("/dev/trade", post(bot_trade))
        .route("/dev/script", post(script))
        .route("/dev/map", get(map))
        .route("/dev/observe", post(observe))
        .route("/dev/leave", post(bot_leave))
        .route_layer(axum::middleware::from_fn_with_state(
            a.clone(),
            lifecycle::drain::http_entry,
        ))
        .merge(lifecycle::drain::routes())
        .route("/health", get(health))
        .merge(client_assets(&client))
        .layer(DefaultBodyLimit::max(1_048_576))
        .with_state(a.clone())
}

pub async fn serve(a: App, bind: SocketAddr, client: PathBuf) -> Result<()> {
    let deaths = a.clone();
    tokio::spawn(async move {
        let result: Result<()> = async {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                persistence_tick(&deaths).await?;
            }
        }
        .await;
        if let Err(error) = result {
            tracing::error!(error=%format_args!("{error:#}"), "persistence coordinator stopped");
        }
    });
    let listener = tokio::net::TcpListener::bind(bind).await.with_context(|| {
        format!("cannot listen on {bind}; another server may already use this port. Stop it and rerun ./run.sh")
    })?;
    if a.shore_bots {
        crate::bots::start(&a).await?;
    }
    let app = routes(&a, client);
    tracing::info!(address=%bind,origin=%a.origin,max_players=a.max_players,behind_proxy=a.behind_proxy,"game frontdoor listening");
    if !a.behind_proxy {
        eprintln!(
            "\nLocal server ready. Open: http://{bind}/\nLogs: the terminal and .local/server.log\n"
        );
    }
    let shutdown = a.clone();
    let failure = Arc::new(Mutex::new(None));
    let report = failure.clone();
    let (stopped, mut stop) = tokio::sync::watch::channel(false);
    let serving = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let result: Result<()> = async {
            shutdown_signal().await?;
            tracing::info!("saving active characters before shutdown");
            lifecycle::drain::prepare(&shutdown);
            shutdown.shutdown.send_replace(true);
            lifecycle::drain::terminate(&shutdown).await
        }
        .await;
        if let Err(e) = result {
            tracing::error!(error=%e,"shutdown persistence failed");
            *report.lock().expect("shutdown report poisoned") = Some(e.to_string());
        }
        stopped.send_replace(true);
    });
    tokio::select! {
        result = serving => result?,
        _ = async { let _ = stop.changed().await; tokio::time::sleep(std::time::Duration::from_secs(1)).await; } => {
            let failure = failure.lock().expect("shutdown report poisoned").clone();
            bail!("HTTP shutdown deadline exceeded; drain failure={failure:?}");
        }
    }
    if let Some(e) = failure.lock().expect("shutdown report poisoned").take() {
        bail!("{e}");
    }
    Ok(())
}
async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {v=tokio::signal::ctrl_c()=>v?,_=term.recv()=>{}};
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_otherhit_has_target_but_squarehit_does_not() {
        let mut out = Writer::new();
        out.i32(1234);
        out.u8(7);
        out.i32(1001);
        let square = out.0.clone();
        out.i32(5000);
        assert_eq!(square.len(), 9);
        assert_eq!(out.0.len(), 13);
        collision_report(false, &mut Reader::new(&square)).unwrap();
        collision_report(true, &mut Reader::new(&out.0)).unwrap();
        assert!(collision_report(false, &mut Reader::new(&out.0)).is_err());
        for end in 0..out.0.len() {
            assert!(collision_report(true, &mut Reader::new(&out.0[..end])).is_err());
        }
        out.u8(0);
        assert!(collision_report(true, &mut Reader::new(&out.0)).is_err());
    }
    #[test]
    fn native_trade_offer_requires_exact_canonical_twelve_slot_shape() {
        let mut out = Writer::new();
        out.i16(12);
        for slot in 0..12 {
            out.u8(u8::from(slot == 4));
        }
        let mut r = Reader::new(&out.0);
        let offer = trade_offer(&mut r).unwrap();
        r.done().unwrap();
        assert!(offer[4]);
        assert_eq!(offer.iter().filter(|v| **v).count(), 1);
        for count in [-1, 0, 11, 13, i16::MAX] {
            let mut out = Writer::new();
            out.i16(count);
            assert!(trade_offer(&mut Reader::new(&out.0)).is_err());
        }
        out.0[6] = 2;
        assert!(trade_offer(&mut Reader::new(&out.0)).is_err());
        out.0[6] = 1;
        out.0.pop();
        assert!(trade_offer(&mut Reader::new(&out.0)).is_err());
    }
    #[test]
    fn selected_player_text_has_only_one_utf_string() {
        let mut out = Writer::new();
        out.utf("café 😀");
        assert_eq!(player_text(&mut Reader::new(&out.0)).unwrap(), "café 😀");
        for end in 0..out.0.len() {
            assert!(player_text(&mut Reader::new(&out.0[..end])).is_err());
        }
        out.utf("ForgedSender");
        assert!(player_text(&mut Reader::new(&out.0)).is_err());
        assert!(player_text(&mut Reader::new(&[0, 1, 0xff])).is_err());
    }
    #[test]
    fn guest_rate_is_bounded_and_expires() {
        let mut requests = HashMap::new();
        let now = std::time::Instant::now();
        let ip = "192.0.2.1".parse().unwrap();
        for _ in 0..20 {
            assert!(rate_allow(&mut requests, ip, now));
        }
        assert!(!rate_allow(&mut requests, ip, now));
        assert!(rate_allow(&mut requests, "192.0.2.2".parse().unwrap(), now));
        assert!(rate_allow(
            &mut requests,
            ip,
            now + std::time::Duration::from_secs(60)
        ));
        assert_eq!(requests.len(), 1);
        for n in 1..=4096 {
            requests.insert(
                IpAddr::V6(std::net::Ipv6Addr::from(n as u128)),
                (now + std::time::Duration::from_secs(60), 1),
            );
        }
        assert!(!rate_allow(
            &mut requests,
            "192.0.2.3".parse().unwrap(),
            now + std::time::Duration::from_secs(60)
        ));
    }
}

#[cfg(test)]
mod equipment_fame_tests;
