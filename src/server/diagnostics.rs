use super::*;
use axum::extract::Path;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, VecDeque},
    time::Instant,
};

pub struct Diagnostics {
    root: PathBuf,
    pub binary_sha256: String,
    source_revision: Option<String>,
    browsers: Mutex<HashMap<String, std::sync::Weak<Mutex<Browser>>>>,
    files: Mutex<BTreeSet<String>>,
}
#[derive(Default)]
pub struct Browser {
    samples: VecDeque<(Instant, BrowserSample)>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSample {
    build: String,
    renderer: Option<String>,
    renderer_status: String,
    width: u32,
    height: u32,
    pixel_ratio: f64,
    hidden: bool,
    hidden_during_interval: bool,
    frames: u32,
    max_frame_gap_ms: f64,
    long_tasks: Option<u32>,
    long_task_ms: Option<f64>,
    elapsed_ms: f64,
}
impl BrowserSample {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.build.len() <= 256
                && self.renderer.as_ref().is_none_or(|s| s.len() <= 512)
                && self.renderer_status.len() <= 80,
            "diagnostic string too long"
        );
        ensure!(
            self.width <= 32768 && self.height <= 32768 && self.frames <= 100_000,
            "invalid display/frame count"
        );
        for n in [self.pixel_ratio, self.max_frame_gap_ms, self.elapsed_ms] {
            ensure!(
                n.is_finite() && (0.0..=86_400_000.0).contains(&n),
                "invalid browser timing"
            );
        }
        ensure!(
            self.long_task_ms
                .is_none_or(|n| n.is_finite() && (0.0..=86_400_000.0).contains(&n)),
            "invalid long task timing"
        );
        Ok(())
    }
}
impl Diagnostics {
    pub fn open(root: PathBuf) -> Result<Self> {
        use sha2::{Digest, Sha256};
        std::fs::create_dir_all(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut pending = BTreeSet::new();
        for entry in std::fs::read_dir(&root)? {
            let path = entry?.path();
            if path.extension().is_some_and(|v| v == "json") {
                let report: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                let id = report["id"].as_str().context("missing stored bug ID")?;
                ensure!(
                    valid_id(id) && path.file_stem().is_some_and(|s| s == id),
                    "invalid stored bug identity"
                );
                match report["delivery"]["status"].as_str() {
                    Some("pending") => {
                        pending.insert(id.into());
                    }
                    Some("delivered") => {}
                    _ => bail!("invalid stored bug delivery state"),
                }
            }
        }
        let executable = std::env::current_exe()?;
        let source_revision = match std::fs::read_to_string(executable.with_file_name("REVISION")) {
            Ok(value) => {
                let value = value.trim().to_string();
                ensure!(
                    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid release REVISION"
                );
                Some(value)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            root,
            source_revision,
            binary_sha256: format!("{:x}", Sha256::digest(std::fs::read(executable)?)),
            browsers: Mutex::default(),
            files: Mutex::new(pending),
        })
    }
    pub fn browser(&self, key: &str) -> Result<Arc<Mutex<Browser>>> {
        ensure!(
            key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid diagnostic session"
        );
        let mut browsers = self.browsers.lock().expect("diagnostics poisoned");
        browsers.retain(|_, v| v.strong_count() > 0);
        if let Some(browser) = browsers.get(key).and_then(|v| v.upgrade()) {
            return Ok(browser);
        }
        ensure!(browsers.len() < 256, "diagnostic session limit reached");
        let browser = Arc::new(Mutex::new(Browser::default()));
        browsers.insert(key.into(), Arc::downgrade(&browser));
        Ok(browser)
    }
    fn save(&self, report: &Value) -> Result<()> {
        let mut pending = self.files.lock().expect("diagnostics poisoned");
        ensure!(
            std::fs::read_dir(&self.root)?.count() < 10_000,
            "Bug report storage full; notify an administrator."
        );
        self.write(report)?;
        pending.insert(report["id"].as_str().context("missing report ID")?.into());
        Ok(())
    }
    fn write(&self, report: &Value) -> Result<()> {
        use std::io::Write;
        let id = report["id"].as_str().context("missing report ID")?;
        ensure!(valid_id(id), "invalid report ID");
        let path = self.root.join(format!("{id}.json"));
        let temp = self
            .root
            .join(format!("{id}-{:016x}.tmp", rand::random::<u64>()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(report)?)?;
        file.sync_all()?;
        std::fs::rename(temp, path)?;
        std::fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}
fn valid_id(id: &str) -> bool {
    id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
#[derive(Default)]
pub struct ConnectionDiagnostics {
    pub user_agent: Option<String>,
    pub browser: Option<Arc<Mutex<Browser>>>,
    pending: BTreeMap<i32, Instant>,
    replies: VecDeque<(Instant, f64)>,
    pub packets_in: u64,
    pub packets_out: u64,
    pub max_send_ms: f64,
    pub max_handler_ms: f64,
    pub max_input_queue_ms: f64,
    pub queue_high_water: usize,
    pub expired_pings: u64,
    pub unmatched_pongs: u64,
    started: Option<Instant>,
}
impl ConnectionDiagnostics {
    pub fn sent(&mut self, serial: i32) {
        let now = Instant::now();
        self.started.get_or_insert(now);
        let count = self.pending.len();
        self.pending.retain(|_, t| t.elapsed().as_secs() < 120);
        self.expired_pings += (count - self.pending.len()) as u64;
        self.pending.insert(serial, now);
    }
    pub fn pong(&mut self, serial: i32) {
        if let Some(sent) = self.pending.remove(&serial) {
            self.replies
                .push_back((Instant::now(), sent.elapsed().as_secs_f64() * 1000.0));
            while self
                .replies
                .front()
                .is_some_and(|(t, _)| t.elapsed().as_secs() > 120)
            {
                self.replies.pop_front();
            }
        } else {
            self.unmatched_pongs += 1;
        }
    }
    fn snapshot(&self) -> Value {
        let samples: Vec<_> = self
            .replies
            .iter()
            .filter(|(t, _)| t.elapsed().as_secs() <= 120)
            .map(|(t, ms)| json!({"age_ms": t.elapsed().as_millis(), "rtt_ms": ms}))
            .collect();
        let browser = self.browser.as_ref().map(|b| {
            b.lock()
                .expect("browser poisoned")
                .samples
                .iter()
                .filter(|(t, _)| t.elapsed().as_secs() <= 120)
                .map(|(t, sample)| json!({"age_ms": t.elapsed().as_millis(), "sample": sample}))
                .collect::<Vec<_>>()
        });
        json!({"user_agent": self.user_agent, "browser_samples": browser, "rtt_samples": samples,
            "pending_ping_ages_ms": self.pending.values().map(|t| t.elapsed().as_millis()).collect::<Vec<_>>(),
            "expired_pings_session": self.expired_pings, "unmatched_pongs_session": self.unmatched_pongs,
            "game_commands_in_session": self.packets_in, "queued_packets_out_session": self.packets_out,
            "max_socket_send_ms_session": self.max_send_ms, "max_handler_ms_session": self.max_handler_ms, "max_input_queue_ms_session": self.max_input_queue_ms,
            "observed_outgoing_queue_high_water_session": self.queue_high_water,
            "ping_observation_age_ms": self.started.map(|t| t.elapsed().as_millis())})
    }
}
fn summary(values: impl Iterator<Item = f64>) -> Value {
    let mut values: Vec<_> = values.collect();
    values.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        values
            .get(((values.len() as f64 * p).ceil() as usize).saturating_sub(1))
            .copied()
    };
    json!({"samples": values.len(), "median": percentile(0.5), "p95": percentile(0.95), "max": values.last()})
}
fn public_diagnostics(report: &Value) -> Value {
    let connection = &report["connection"];
    let rtt = connection["rtt_samples"].as_array().expect("RTT samples");
    let ticks = report["context"]["recent_ticks"]
        .as_array()
        .expect("tick samples");
    let browser: Vec<_> = connection["browser_samples"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let visible: Vec<_> = browser
        .iter()
        .filter(|v| v["sample"]["hidden_during_interval"] == false)
        .collect();
    let versions = browser
        .last()
        .and_then(|v| v["sample"]["build"].as_str())
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    let revision = |key: &str, length: usize| {
        versions
            .as_ref()
            .and_then(|v| v[key].as_str())
            .filter(|s| s.len() == length && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_owned)
    };
    let mut history = Vec::new();
    for bucket in 0..24 {
        let within = |v: &&Value| v["age_ms"].as_u64().is_some_and(|age| age / 5000 == bucket);
        let tick_values: Vec<_> = ticks.iter().filter(within).collect();
        let ping_values: Vec<_> = rtt.iter().filter(within).collect();
        if !tick_values.is_empty() || !ping_values.is_empty() {
            history.push(json!({"seconds_before_report": [bucket * 5, (bucket + 1) * 5],
                "game_rtt_ms": summary(ping_values.iter().filter_map(|v| v["rtt_ms"].as_f64())),
                "tick_ms": summary(tick_values.iter().filter_map(|v| v["execution_us"].as_f64().map(|n| n / 1000.0))),
                "scheduling_delay_ms": summary(tick_values.iter().filter_map(|v| v["scheduling_lateness_us"].as_f64().map(|n| n / 1000.0)))}));
        }
    }
    let browser_history: Vec<_> = browser.iter().map(|v| {
        let s = &v["sample"];
        json!({"age_ms": v["age_ms"], "interval_ms": s["elapsed_ms"], "hidden_during_interval": s["hidden_during_interval"],
            "animation_frames": s["frames"], "max_frame_gap_ms": s["max_frame_gap_ms"], "long_tasks": s["long_tasks"], "long_task_ms": s["long_task_ms"],
            "canvas_width": s["width"], "canvas_height": s["height"], "pixel_ratio": s["pixel_ratio"]})
    }).collect();
    let ua = connection["user_agent"].as_str().unwrap_or("");
    let browser_family = if ua.contains("Firefox/") {
        Some("Firefox")
    } else if ua.contains("Edg/") {
        Some("Edge")
    } else if ua.contains("Chrome/") {
        Some("Chrome")
    } else if ua.contains("Safari/") {
        Some("Safari")
    } else {
        None
    };
    let os_family = if ua.contains("Android") {
        Some("Android")
    } else if ua.contains("iPhone") || ua.contains("iPad") {
        Some("iOS")
    } else if ua.contains("Windows") {
        Some("Windows")
    } else if ua.contains("Macintosh") {
        Some("macOS")
    } else if ua.contains("Linux") {
        Some("Linux")
    } else {
        None
    };
    json!({"schema": 2, "reported_at_unix_ms": report["created_at_unix_ms"],
        "versions": {"server": report["server_source_revision"], "server_binary_sha256": report["server_binary_sha256"], "protocol": report["protocol_build"],
            "website": revision("website", 40), "client": revision("client", 40), "runtime": revision("runtime", 16)},
        "environment": {"browser": browser_family, "os": os_family},
        "game_rtt_ms": summary(rtt.iter().filter_map(|v| v["rtt_ms"].as_f64())),
        "tick_ms": summary(ticks.iter().filter_map(|v| v["execution_us"].as_f64().map(|n| n / 1000.0))),
        "scheduling_delay_ms": summary(ticks.iter().filter_map(|v| v["scheduling_lateness_us"].as_f64().map(|n| n / 1000.0))),
        "visible_frame_gap_ms": summary(visible.iter().filter_map(|v| v["sample"]["max_frame_gap_ms"].as_f64())),
        "connection": {"max_socket_send_ms": connection["max_socket_send_ms_session"], "max_handler_ms": connection["max_handler_ms_session"],
            "max_input_queue_ms": connection["max_input_queue_ms_session"], "outgoing_queue_high_water": connection["observed_outgoing_queue_high_water_session"],
            "expired_pings": connection["expired_pings_session"], "pending_ping_ages_ms": connection["pending_ping_ages_ms"]},
        "capture_window_seconds": 120,
        "population": {"world_players": report["context"]["world_players"], "world_entities": report["context"]["world_entities"], "world_projectiles": report["context"]["world_projectiles"],
            "total_players": report["context"]["total_players"], "total_worlds": report["context"]["total_worlds"]},
        "history": history, "browser_history": browser_history,
        "units": "Timings in milliseconds; history uses five-second buckets. RTT includes game/client/server processing. Animation-frame gaps are not game FPS. Null means unavailable."})
}
pub async fn attachment(State(a): State<App>, Path(id): Path<String>) -> Response {
    if !valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let result = tokio::task::spawn_blocking(move || -> Result<Option<Value>> {
        let path = a.diagnostics.root.join(format!("{id}.json"));
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let report: Value = serde_json::from_slice(&bytes)?;
        Ok(report.get("diagnostics").cloned())
    })
    .await;
    match result {
        Ok(Ok(Some(value))) => (
            [
                (
                    "content-disposition",
                    "attachment; filename=\"diagnostics.json\"",
                ),
                ("x-content-type-options", "nosniff"),
            ],
            Json(value),
        )
            .into_response(),
        Ok(Ok(None)) => StatusCode::NOT_FOUND.into_response(),
        Ok(Err(e)) => error(e),
        Err(e) => error(e.into()),
    }
}

pub async fn report(
    a: &App,
    account: i32,
    player: i32,
    message: &str,
    connection: &ConnectionDiagnostics,
) -> Result<()> {
    ensure!(
        a.discord_link_token.is_some(),
        "Discord bug reporting is not configured."
    );
    ensure!(
        !message.trim().is_empty(),
        "Usage: /bug <description>. Reports are public; do not include personal information."
    );
    ensure!(
        message.chars().count() <= 500,
        "Bug message is too long (maximum 500 characters)."
    );
    let requested = Instant::now();
    let context = a.world.directory(move |d| d.bug_context(player)).await?;
    let id = {
        use rand::RngCore;
        let mut bytes = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let reporter_name = context["player_name"]
        .as_str()
        .context("bug reporter name missing")?;
    let mut report = json!({"schema": 3, "reporter_name": reporter_name, "id": id, "created_at_unix_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis(),
        "delivery": {"status": "pending"}, "message_untrusted": message, "account_id": account,
        "server_binary_sha256": a.diagnostics.binary_sha256, "server_source_revision": a.diagnostics.source_revision, "protocol_build": "123.5.1", "context": context,
        "server_uptime_ms": a.started.elapsed().as_millis(), "configured_player_cap": a.max_players,
        "context_request_ms": requested.elapsed().as_secs_f64() * 1000.0, "connection": connection.snapshot(),
        "capture_window_seconds": 120});
    report["diagnostics"] = public_diagnostics(&report);
    report["diagnostics_url"] = json!(format!(
        "{}/api/bug-reports/{id}/diagnostics.json",
        a.origin
    ));
    let object = report.as_object_mut().expect("report object");
    object.remove("connection");
    object.insert("context".into(), json!({"player_id": player}));
    let diagnostics = a.diagnostics.clone();
    tokio::task::spawn_blocking(move || diagnostics.save(&report)).await??;
    tracing::info!(report_id=%id, account, player, "bug report saved; awaiting Discord delivery");
    Ok(())
}
pub async fn browser_sample(
    State(a): State<App>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Json(sample): Json<BrowserSample>,
) -> Response {
    let result: Result<bool> = (|| {
        ensure!(
            headers.get("origin").and_then(|v| v.to_str().ok()) == Some(a.origin.as_str()),
            "unapproved diagnostic origin"
        );
        sample.validate()?;
        let browser = a
            .diagnostics
            .browsers
            .lock()
            .expect("diagnostics poisoned")
            .get(&key)
            .and_then(|v| v.upgrade());
        let Some(browser) = browser else {
            return Ok(false);
        };
        let mut browser = browser.lock().expect("browser poisoned");
        ensure!(
            browser
                .samples
                .back()
                .is_none_or(|(t, _)| t.elapsed().as_secs() >= 3),
            "diagnostic sample rate exceeded"
        );
        while browser.samples.len() >= 24 {
            browser.samples.pop_front();
        }
        browser.samples.push_back((Instant::now(), sample));
        Ok(true)
    })();
    match result {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            "No active game socket for this diagnostic session",
        )
            .into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    target: String,
    id: String,
}
pub async fn pending(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(e) = super::hiscores::authorize(&a, peer, &headers) {
        return error(e);
    }
    let result = tokio::task::spawn_blocking(move || -> Result<Value> {
        let mut pending = a.diagnostics.files.lock().expect("diagnostics poisoned");
        while let Some(id) = pending.first().cloned() {
            let value: Value = serde_json::from_slice(&std::fs::read(
                a.diagnostics.root.join(format!("{id}.json")),
            )?)?;
            if value["delivery"]["status"] == "pending" {
                return Ok(json!({"report": value}));
            }
            ensure!(
                value["delivery"]["status"] == "delivered",
                "invalid stored bug delivery state"
            );
            pending.remove(&id);
        }
        Ok(json!({"report": null}))
    })
    .await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => error(e),
        Err(e) => error(e.into()),
    }
}
pub async fn receipt(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(receipt): Json<Receipt>,
) -> Response {
    let result: Result<()> = async {
        super::hiscores::authorize(&a, peer, &headers)?;
        ensure!(valid_id(&id) && receipt.id.bytes().all(|b| b.is_ascii_digit()) && !receipt.id.is_empty(), "invalid bug delivery receipt");
        ensure!(match receipt.target.as_str() {
            "private_message_id" | "public_message_id" => (17..=20).contains(&receipt.id.len()),
            "issue_number" => receipt.id.parse::<u32>().is_ok_and(|n| n > 0),
            _ => false,
        }, "invalid bug delivery target");
        if receipt.target == "issue_number" {
            let diagnostics = a.diagnostics.clone();
            let (id, issue) = (id.clone(), receipt.id.clone());
            let account = tokio::task::spawn_blocking(move || -> Result<i32> {
                let report: Value = serde_json::from_slice(&std::fs::read(diagnostics.root.join(format!("{id}.json")))?)?;
                ensure!(report["id"] == id && matches!(report["delivery"]["status"].as_str(), Some("pending" | "delivered")), "invalid stored bug report");
                ensure!(report["delivery"]["issue_number"].is_null() || report["delivery"]["issue_number"] == issue, "conflicting delivery receipt");
                Ok(serde_json::from_value(report["account_id"].clone())?)
            }).await??;
            // Keep the report retryable until its once-only reward is durable.
            a.store.reward_first_bug(account).await?;
            super::gold::sync(&a, account).await?;
        }
        let world = a.world.clone();
        let notification = tokio::task::spawn_blocking(move || -> Result<Option<(i32, i32, String)>> {
            let mut pending = a.diagnostics.files.lock().expect("diagnostics poisoned");
            let mut report: Value = serde_json::from_slice(&std::fs::read(a.diagnostics.root.join(format!("{id}.json")))?)?;
            let notify = receipt.target == "issue_number" && report["delivery"]["issue_number"].is_null();
            ensure!(report["delivery"][&receipt.target].is_null() || report["delivery"][&receipt.target] == receipt.id, "conflicting delivery receipt");
            report["delivery"][&receipt.target] = json!(receipt.id);
            if ["public_message_id", "issue_number"].iter().all(|key| report["delivery"][*key].is_string()) {
                report["delivery"]["status"] = json!("delivered");
            }
            a.diagnostics.write(&report)?;
            if report["delivery"]["status"] == "delivered" { pending.remove(&id); }
            tracing::info!(report_id=%id, target=%receipt.target, external_id=%receipt.id, "bug report delivery acknowledged");
            Ok(if notify { Some((report["context"]["player_id"].as_i64().context("missing report player")? as i32, report["account_id"].as_i64().context("missing report account")? as i32, receipt.id)) } else { None })
        }).await??;
        if let Some((player, account, issue)) = notification
            && let Err(error) = world.for_player(player).call(move |w| {
                let p = w.players.get(&player).context("reporting player disconnected")?;
                ensure!(p.character.account == account, "reporting session changed");
                let packet = crate::world::chat_info(&format!("Reported as #{issue}: https://github.com/OpenRotMG/Open-RotMG-Server/issues/{issue}"));
                crate::world::send(p, packet.id, packet.body);
                Ok(())
            }).await { tracing::info!(%error, "bug issue created after reporting player disconnected"); }
        Ok(())
    }.await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_attachment_is_bounded_and_strictly_allowlisted() {
        let report = json!({"created_at_unix_ms": 123, "server_source_revision": "a".repeat(40), "server_binary_sha256": "b".repeat(64), "protocol_build": "123.5.1",
            "context": {"player_id": 12345, "world_name": "PRIVATE_WORLD", "position": {"x": 1}, "world_players": 2, "world_entities": 20, "world_projectiles": 4,
                "recent_ticks": (0..2400).map(|i| json!({"age_ms": i*50, "execution_us": 1000, "scheduling_lateness_us": 2000})).collect::<Vec<_>>()},
            "connection": {"user_agent": "PRIVATE_UA Chrome/123 Linux", "rtt_samples": [{"age_ms": 1000, "rtt_ms": 75}],
                "browser_samples": [{"age_ms": 1000, "sample": {"build": r#"{"website":"PRIVATE_BUILD"}"#, "renderer": "PRIVATE_GPU", "hidden_during_interval": false, "frames": 300, "elapsed_ms": 5000, "max_frame_gap_ms": 40}}]}});
        let attachment = public_diagnostics(&report);
        assert_eq!(attachment["tick_ms"]["samples"], 2400);
        assert_eq!(attachment["tick_ms"]["max"], 1.0);
        assert_eq!(attachment["game_rtt_ms"]["median"], 75.0);
        assert_eq!(attachment["history"].as_array().unwrap().len(), 24);
        assert_eq!(
            attachment["environment"],
            json!({"browser": "Chrome", "os": "Linux"})
        );
        let encoded = serde_json::to_string(&attachment).unwrap();
        for excluded in [
            "PRIVATE_",
            "player_id",
            "world_name",
            "position",
            "user_agent",
            "renderer",
            "12345",
        ] {
            assert!(!encoded.contains(excluded), "{excluded}");
        }
        assert!(encoded.len() < 15000);
    }
    #[test]
    fn ping_serials_are_matched_and_only_recent_samples_are_reported() {
        let mut d = ConnectionDiagnostics::default();
        d.sent(1);
        d.pong(2);
        d.pong(1);
        d.pong(1);
        assert_eq!(d.snapshot()["rtt_samples"].as_array().unwrap().len(), 1);
        assert_eq!(d.unmatched_pongs, 2);
        d.replies
            .push_back((Instant::now() - std::time::Duration::from_secs(121), 99.0));
        assert_eq!(d.snapshot()["rtt_samples"].as_array().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn consecutive_native_commands_spool_privately_and_receipts_survive_restart() {
        let (root, mut a) = super::super::account_actions::tests::fixture();
        let account = a.store.list("bug-test".into()).await.unwrap().id;
        a.store.rename(account, "BugWizard".into()).await.unwrap();
        let character = a
            .store
            .create(account, a.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let (messages, mut notification_rx) = mpsc::channel(64);
        let (notify_lost, _) = tokio::sync::watch::channel(false);
        let player = a
            .world
            .call(move |w| {
                w.enter(
                    character,
                    Some(crate::world::Output {
                        packets: messages,
                        lost: notify_lost,
                    }),
                )
            })
            .await
            .unwrap();
        let mut session = Session {
            account: Some(a.claim_session(account).unwrap()),
            player: Some(player),
            ..Session::default()
        };
        session.diagnostics.user_agent = Some("PRIVATE_UA".into());
        session.diagnostics.sent(1);
        session.diagnostics.pong(1);
        let (tx, mut rx) = mpsc::channel(64);
        let (lost, _) = tokio::sync::watch::channel(false);
        let mut text = Writer::new();
        text.utf("/bug enemies froze");
        for _ in 0..2 {
            handle(
                &mut a,
                Packet {
                    id: wire::PLAYER_TEXT,
                    body: text.0.clone(),
                },
                &mut session,
                &tx,
                &lost,
                Instant::now(),
            )
            .await
            .unwrap();
        }
        assert!(
            rx.try_recv().is_err(),
            "only issue creation should produce a success reply"
        );
        assert_eq!(a.store.gold(account).await.unwrap().balance, 100);
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
        );
        let peer: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let response = pending(State(a.clone()), ConnectInfo(peer), headers.clone()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        let report = &value["report"];
        assert_eq!(report["schema"], 3);
        assert_eq!(report["reporter_name"], "BugWizard");
        assert_eq!(report["message_untrusted"], "enemies froze");
        assert!(report.get("connection").is_none());
        assert_eq!(report["account_id"], account);
        let id = report["id"].as_str().unwrap().to_string();
        let public = attachment(State(a.clone()), Path(id.clone())).await;
        assert_eq!(public.status(), StatusCode::OK);
        let public_body = axum::body::to_bytes(public.into_body(), 100_000)
            .await
            .unwrap();
        let encoded = String::from_utf8(public_body.to_vec()).unwrap();
        for excluded in [
            "PRIVATE_UA",
            "account_id",
            "player_id",
            "position",
            "message_untrusted",
            "reporter_name",
            "BugWizard",
            "investigation",
            "enemies froze",
        ] {
            assert!(!encoded.contains(excluded), "{excluded}");
        }
        assert!(encoded.len() < 40_000);
        assert_eq!(
            attachment(State(a.clone()), Path("../game.db".into()))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            attachment(State(a.clone()), Path("f".repeat(24)))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert!(
            super::report(&a, account, player, "", &session.diagnostics)
                .await
                .unwrap_err()
                .to_string()
                .contains("Usage: /bug")
        );
        assert_eq!(
            pending(State(a.clone()), ConnectInfo(peer), HeaderMap::new())
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        let mut forwarded = headers.clone();
        forwarded.insert("x-forwarded-for", "127.0.0.1".parse().unwrap());
        assert_eq!(
            pending(State(a.clone()), ConnectInfo(peer), forwarded)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        let unauthorized = receipt(
            State(a.clone()),
            ConnectInfo(peer),
            Path(id.clone()),
            HeaderMap::new(),
            Json(Receipt {
                target: "issue_number".into(),
                id: "42".into(),
            }),
        )
        .await;
        assert_eq!(unauthorized.status(), StatusCode::BAD_REQUEST);
        rusqlite::Connection::open(root.join("game.db"))
            .unwrap()
            .execute(
                "UPDATE accounts SET credits=2147483647 WHERE id=?1",
                [account],
            )
            .unwrap();
        let failed = receipt(
            State(a.clone()),
            ConnectInfo(peer),
            Path(id.clone()),
            headers.clone(),
            Json(Receipt {
                target: "issue_number".into(),
                id: "42".into(),
            }),
        )
        .await;
        assert_eq!(failed.status(), StatusCode::BAD_REQUEST);
        let unacknowledged: Value = serde_json::from_slice(
            &std::fs::read(root.join("bug-reports").join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        assert!(unacknowledged["delivery"]["issue_number"].is_null());
        assert_eq!(unacknowledged["delivery"]["status"], "pending");
        assert!(a.store.gold(account).await.unwrap().rewards.is_empty());
        rusqlite::Connection::open(root.join("game.db"))
            .unwrap()
            .execute("UPDATE accounts SET credits=100 WHERE id=?1", [account])
            .unwrap();
        for (target, external_id) in [
            ("public_message_id", "1559759855621898313"),
            ("issue_number", "42"),
            ("issue_number", "42"),
        ] {
            let response = receipt(
                State(a.clone()),
                ConnectInfo(peer),
                Path(id.clone()),
                headers.clone(),
                Json(Receipt {
                    target: target.into(),
                    id: external_id.into(),
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
        let mut confirmed = false;
        let mut rewarded = 0;
        while let Ok(packet) = notification_rx.try_recv() {
            if packet.id == wire::TEXT {
                let mut text = Reader::new(&packet.body);
                text.utf().unwrap();
                text.i32().unwrap();
                text.i32().unwrap();
                text.u8().unwrap();
                text.utf().unwrap();
                let message = text.utf().unwrap();
                confirmed |= message
                    == "Reported as #42: https://github.com/OpenRotMG/Open-RotMG-Server/issues/42";
                rewarded += usize::from(
                    message == "You received 50 Gold for submitting your first bug report!",
                );
            }
        }
        assert!(
            confirmed,
            "issue receipt must notify the in-game reporting player"
        );
        assert_eq!(a.store.gold(account).await.unwrap().balance, 150);
        assert_eq!(
            rewarded, 1,
            "retries must not repeat the reward notification"
        );
        let bad = receipt(
            State(a.clone()),
            ConnectInfo(peer),
            Path(id.clone()),
            headers.clone(),
            Json(Receipt {
                target: "issue_number".into(),
                id: "43".into(),
            }),
        )
        .await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        a.diagnostics = Arc::new(Diagnostics::open(root.join("bug-reports")).unwrap());
        let response = pending(State(a.clone()), ConnectInfo(peer), headers.clone()).await;
        let body = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap();
        let remaining: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(remaining["report"]["reporter_name"], "BugWizard");
        assert_eq!(remaining["report"]["message_untrusted"], "enemies froze");
        assert_eq!(remaining["report"]["account_id"], account);
        assert_ne!(remaining["report"]["id"], id);
        let second = receipt(
            State(a.clone()),
            ConnectInfo(peer),
            Path(remaining["report"]["id"].as_str().unwrap().into()),
            headers.clone(),
            Json(Receipt {
                target: "issue_number".into(),
                id: "43".into(),
            }),
        )
        .await;
        assert_eq!(second.status(), StatusCode::NO_CONTENT);
        assert_eq!(a.store.gold(account).await.unwrap().balance, 150);
        assert!(a.store.gold(account).await.unwrap().rewards.is_empty());
        let saved: Value = serde_json::from_slice(
            &std::fs::read(root.join("bug-reports").join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(saved["delivery"]["status"], "delivered");
        session.account.take().unwrap().release();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn browser_samples_require_origin_active_session_and_bounded_schema() {
        let (root, a) = super::super::account_actions::tests::fixture();
        let key = "b".repeat(32);
        let browser = a.diagnostics.browser(&key).unwrap();
        let sample: BrowserSample = serde_json::from_value(json!({"build": "test", "renderer": null, "renderer_status": "unavailable", "width": 800, "height": 600, "pixel_ratio": 1.5, "hidden": false, "hidden_during_interval": false, "frames": 300, "max_frame_gap_ms": 20, "long_tasks": null, "long_task_ms": null, "elapsed_ms": 5000})).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("origin", a.origin.parse().unwrap());
        assert_eq!(
            browser_sample(
                State(a.clone()),
                Path(key.clone()),
                HeaderMap::new(),
                Json(sample.clone())
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            browser_sample(
                State(a.clone()),
                Path(key.clone()),
                headers.clone(),
                Json(sample.clone())
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            browser_sample(
                State(a.clone()),
                Path(key.clone()),
                headers.clone(),
                Json(sample.clone())
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(browser.lock().unwrap().samples.len(), 1);
        let mut bad = sample.clone();
        bad.max_frame_gap_ms = -1.0;
        assert!(bad.validate().is_err());
        let mut value = serde_json::to_value(&sample).unwrap();
        value["cookie"] = json!("secret");
        assert!(serde_json::from_value::<BrowserSample>(value).is_err());
        drop(browser);
        assert_eq!(
            browser_sample(State(a), Path(key), headers, Json(sample))
                .await
                .status(),
            StatusCode::CONFLICT
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn spool_is_durable_and_private() {
        let root = std::env::temp_dir().join(format!("bug-spool-{}", rand::random::<u64>()));
        let d = Diagnostics::open(root.clone()).unwrap();
        let id = "a".repeat(24);
        let value =
            json!({"id": id, "delivery": {"status": "pending"}, "message_untrusted": "@everyone"});
        d.save(&value).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(
                &std::fs::read(root.join(format!("{id}.json"))).unwrap()
            )
            .unwrap(),
            value
        );
        assert!(d.save(&json!({"id": "../escape"})).is_err());
        assert!(d.browser("bad").is_err());
        let browser = d.browser(&"b".repeat(32)).unwrap();
        assert!(Arc::ptr_eq(&browser, &d.browser(&"b".repeat(32)).unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
