mod content;
#[cfg(test)]
mod historical_contracts;
mod input;
mod lifecycle;
mod protocol;
mod realm_map;
mod rules;
#[allow(dead_code)]
#[path = "../wire.rs"]
mod wire;
mod world;
use anyhow::{Context, Result, ensure};
use axum::{
    Form, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use content::Content;
use futures_util::{Sink, SinkExt, StreamExt};
use rusqlite::Connection;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tower_http::services::ServeDir;
use wire::{Codec, Packet, Writer};
use world::Character;

#[cfg(test)]
#[path = "cadence_transport_tests.rs"]
mod cadence_transport_tests;

struct App {
    db: lifecycle::Database,
    commands: mpsc::Sender<lifecycle::Command>,
    origin: String,
}
struct Lease {
    commands: mpsc::Sender<lifecycle::Command>,
    session: u64,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Err(error) = self.commands.try_send(lifecycle::Command::Detach {
            session: self.session,
        }) {
            // Closed output receivers are also reaped by the owner on its next tick.
            tracing::warn!(session=self.session,error=%error,"build100 detach queue rejected");
        }
    }
}
impl App {
    fn character(&self, guid: &str) -> Result<Option<Character>> {
        lifecycle::character(&self.db, guid)
    }
    #[cfg(test)]
    fn save(&self, guid: &str, p: &Character) -> Result<()> {
        let state = serde_json::to_string(p)?;
        self.db.lock().expect("database mutex poisoned").execute("INSERT INTO build100_characters(guid,state) VALUES(?,?) ON CONFLICT(guid) DO UPDATE SET state=excluded.state",[guid,&state])?;
        Ok(())
    }
}
fn database(path: &std::path::Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "synchronous", "FULL")?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS build100_characters(guid TEXT PRIMARY KEY,state TEXT NOT NULL)",
    )?;
    Ok(db)
}
fn guid(value: &str) -> Result<()> {
    ensure!(
        value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()),
        "expected build100 anonymous GUID"
    );
    Ok(())
}
async fn chars(State(a): State<Arc<App>>, Form(f): Form<HashMap<String, String>>) -> Response {
    let result = (|| -> Result<String> {
        let id = f.get("guid").context("missing GUID")?;
        guid(id)?;
        Ok(match a.character(id)?.filter(|p| p.alive) {
            Some(p) => format!(
                "<Chars><Char><CharId>0</CharId><Name>{}</Name><ObjectType>{}</ObjectType><Level>{}</Level></Char></Chars>",
                p.name, p.kind, p.stats[&7]
            ),
            None => "<Chars/>".into(),
        })
    })();
    match result {
        Ok(xml) => ([("Content-Type", "text/xml")], xml).into_response(),
        Err(e) => {
            tracing::warn!(error=%e,"build100 character list rejected");
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}
fn authorized_origin(headers: &HeaderMap, expected: &str) -> bool {
    let mut origins = headers.get_all("origin").iter();
    origins.next().and_then(|value| value.to_str().ok()) == Some(expected)
        && origins.next().is_none()
}
async fn upgrade(State(a): State<Arc<App>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !authorized_origin(&headers, &a.origin) {
        return (StatusCode::FORBIDDEN, "unapproved build100 origin").into_response();
    }
    ws.max_message_size(input::RAW_BYTE_CAPACITY)
        .max_frame_size(input::RAW_BYTE_CAPACITY)
        .on_upgrade(move |socket| async move {
            if let Err(e) = connection(socket, a).await {
                tracing::warn!(error=%format!("{e:#}"),"build100 session ended");
            }
        })
}
// Diagnostic metadata only: malformed shots still reach the original authoritative parser.
fn shot_trace(packet: &Packet) -> Option<(u8, i32, i16)> {
    if packet.id != 8 || packet.body.len() != 22 {
        return None;
    }
    let mut r = wire::Reader::new(&packet.body);
    let bullet = r.u8().ok()?;
    let owner = r.i32().ok()?;
    r.u8().ok()?;
    Some((bullet, owner, r.i16().ok()?))
}
async fn send<S>(
    socket: &mut S,
    codec: &mut Codec,
    packets: Vec<Packet>,
    session: Option<u64>,
) -> Result<()>
where
    S: Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let count = packets.len();
    let batch_started = std::time::Instant::now();
    tracing::debug!(session, count, "build100 send batch begin");
    for (index, p) in packets.into_iter().enumerate() {
        let id = p.id;
        let shot = shot_trace(&p);
        let started = std::time::Instant::now();
        tracing::debug!(
            session,
            id,
            index,
            count,
            bullet = shot.map(|s| s.0),
            owner = shot.map(|s| s.1),
            item = shot.map(|s| s.2),
            "build100 send packet begin"
        );
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            socket.send(Message::Binary(codec.frame(p.id, p.body).into())),
        )
        .await;
        tracing::debug!(
            session,
            id,
            index,
            count,
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
            batch_elapsed_ms = batch_started.elapsed().as_secs_f64() * 1000.0,
            success = result.as_ref().is_ok_and(|r| r.is_ok()),
            "build100 send packet end"
        );
        result.context("build100 socket output stalled")??;
    }
    tracing::debug!(
        session,
        count,
        elapsed_ms = batch_started.elapsed().as_secs_f64() * 1000.0,
        "build100 send batch end"
    );
    Ok(())
}
async fn input_reply<S>(
    socket: &mut S,
    codec: &mut Codec,
    packets: &mut mpsc::Receiver<Vec<Packet>>,
    mut reply: oneshot::Receiver<Result<(), String>>,
    session: u64,
) -> Result<Result<(), String>>
where
    S: Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    // The raw frame permit stays with dispatch; only outgoing traffic runs during owner backpressure.
    loop {
        tokio::select! {
            result=&mut reply=>return result.context("build100 owner stopped during input"),
            outgoing=packets.recv()=>send(socket,codec,outgoing.context("build100 output channel closed during input")?,Some(session)).await?,
        }
    }
}
async fn connection(socket: WebSocket, a: Arc<App>) -> Result<()> {
    let (mut socket, stream) = socket.split();
    let mut inputs = input::receive(stream);
    let mut codec = protocol::server_codec();
    let mut lease: Option<Lease> = None;
    let (output, mut packets) = mpsc::channel(lifecycle::OUTPUT_CAPACITY);
    let mut pending = Some(output);
    let handshake = tokio::time::sleep(Duration::from_secs(15));
    tokio::pin!(handshake);
    loop {
        tracing::debug!(
            session = lease.as_ref().map(|l| l.session),
            "build100 dispatch poll begin"
        );
        tokio::select! {
            _=&mut handshake, if lease.is_none()=>anyhow::bail!("build100 HELLO timed out"),
            outgoing=packets.recv(), if lease.is_some()=>{
                let Some(outgoing)=outgoing else { break; };
                send(&mut socket,&mut codec,outgoing,lease.as_ref().map(|l|l.session)).await?;
            }
            incoming=inputs.recv()=>{
                let Some(frame)=incoming? else {break;};
                let received=frame.received;
                let frame_seq=frame.sequence;
                // The frame's remaining permit keeps active dispatch inside the raw backlog budget.
                let bytes=match frame.message {
                    Message::Binary(bytes)=>bytes,
                    Message::Close(_)=>break,
                    Message::Ping(bytes)=>{
                        let started=std::time::Instant::now();
                        tracing::debug!(session=lease.as_ref().map(|l|l.session),"build100 pong send begin");
                        let result=tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Pong(bytes))).await;
                        tracing::debug!(session=lease.as_ref().map(|l|l.session),elapsed_ms=started.elapsed().as_secs_f64()*1000.0,success=result.as_ref().is_ok_and(|r|r.is_ok()),"build100 pong send end");
                        result??;continue;
                    }
                    _=>anyhow::bail!("expected binary build100 stream"),
                };
                tracing::debug!(session=lease.as_ref().map(|l|l.session),frame_seq,byte_count=bytes.len(),received_age_ms=received.elapsed().as_secs_f64()*1000.0,"build100 frame dispatch");
                let decoded=codec.feed(&bytes);
                tracing::debug!(session=lease.as_ref().map(|l|l.session),frame_seq,decoded_count=decoded.as_ref().ok().map(Vec::len),success=decoded.is_ok(),received_age_ms=received.elapsed().as_secs_f64()*1000.0,"build100 frame decoded");
                for (packet_index,packet) in decoded?.into_iter().enumerate() {
                    let id=packet.id;
                    let shot=shot_trace(&packet);
                    tracing::debug!(session=lease.as_ref().map(|l|l.session),frame_seq,packet_index,id,bullet=shot.map(|s|s.0),owner=shot.map(|s|s.1),item=shot.map(|s|s.2),received_age_ms=received.elapsed().as_secs_f64()*1000.0,"build100 packet submit");
                    if packet.id==protocol::HELLO {
                        ensure!(lease.is_none(),"duplicate HELLO");
                        let (reply,receive)=oneshot::channel();
                        let command=lifecycle::Command::Join{hello:protocol::Hello::parse(&packet.body)?,output:pending.take().context("HELLO output already assigned")?,reply};
                        let started=std::time::Instant::now();
                        tracing::debug!(frame_seq,packet_index,id,"build100 command enqueue begin");
                        let enqueue=tokio::time::timeout(Duration::from_secs(2),a.commands.send(command)).await;
                        tracing::debug!(frame_seq,packet_index,id,elapsed_ms=started.elapsed().as_secs_f64()*1000.0,success=enqueue.as_ref().is_ok_and(|r|r.is_ok()),"build100 command enqueue end");
                        enqueue.context("build100 command queue stalled")?.map_err(|_|anyhow::anyhow!("build100 owner stopped"))?;
                        let started=std::time::Instant::now();
                        tracing::debug!(frame_seq,packet_index,id,"build100 owner reply begin");
                        let result=receive.await;
                        tracing::debug!(frame_seq,packet_index,id,elapsed_ms=started.elapsed().as_secs_f64()*1000.0,accepted=result.as_ref().ok().map(Result::is_ok),"build100 owner reply end");
                        match result.context("build100 owner stopped during HELLO")? {
                            Ok(session)=>lease=Some(Lease{commands:a.commands.clone(),session}),
                            Err(error)=>{let mut w=Writer::new();w.i32(0).utf(&error);send(&mut socket,&mut codec,vec![Packet{id:0,body:w.0}],None).await?;anyhow::bail!(error);}
                        }
                    } else {
                        let session=lease.as_ref().context("HELLO required")?.session;
                        let (reply,receive)=oneshot::channel();
                        let started=std::time::Instant::now();
                        tracing::debug!(session,frame_seq,packet_index,id,"build100 command enqueue begin");
                        let enqueue=tokio::time::timeout(Duration::from_secs(2),a.commands.send(lifecycle::Command::Input{session,packet,received,reply})).await;
                        tracing::debug!(session,frame_seq,packet_index,id,elapsed_ms=started.elapsed().as_secs_f64()*1000.0,success=enqueue.as_ref().is_ok_and(|r|r.is_ok()),"build100 command enqueue end");
                        enqueue.context("build100 command queue stalled")?.map_err(|_|anyhow::anyhow!("build100 owner stopped"))?;
                        let started=std::time::Instant::now();
                        tracing::debug!(session,frame_seq,packet_index,id,"build100 owner reply begin");
                        let result=input_reply(&mut socket,&mut codec,&mut packets,receive,session).await?;
                        tracing::debug!(session,frame_seq,packet_index,id,elapsed_ms=started.elapsed().as_secs_f64()*1000.0,accepted=result.is_ok(),"build100 owner reply end");
                        if let Err(error)=result {
                            tracing::warn!(session,frame_seq,packet_index,id,bullet=shot.map(|s|s.0),owner=shot.map(|s|s.1),item=shot.map(|s|s.2),error=%error,"build100 request rejected");
                            send(&mut socket,&mut codec,vec![Packet{id:20,body:vec![]},world::message(&error)],Some(session)).await?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
const REALM_SEED: u64 = 100;
const ORIGINAL_CLIENT_SHA256: &str =
    "ec031064b61d22e860de802ae1967ef1bb193b2c35d297ca1d25b1a96f76684e";

#[derive(Deserialize)]
struct ClientManifest {
    client_sha256: String,
    #[serde(default, deserialize_with = "present_manifest_string")]
    data_repairs: Option<String>,
    #[serde(default, deserialize_with = "present_manifest_string")]
    client_file: Option<String>,
    #[serde(default, deserialize_with = "present_manifest_string")]
    original_client_sha256: Option<String>,
}

// Explicit null must not masquerade as the original format's absent fields.
fn present_manifest_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[derive(Debug)]
struct PreparedClient {
    file: &'static str,
    artifact: &'static str,
    sha256: String,
}
impl PreparedClient {
    fn parse(bytes: &[u8]) -> Result<Self> {
        // Serde structs accept positional arrays; keep typed parsing for duplicate-field rejection.
        ensure!(
            bytes
                .iter()
                .find(|&&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                == Some(&b'{'),
            "prepared build100.json root must be a JSON object"
        );
        let manifest: ClientManifest = serde_json::from_slice(bytes)
            .context("invalid prepared build100.json client metadata")?;
        ensure!(
            manifest.client_sha256.len() == 64
                && manifest
                    .client_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid prepared build100 client_sha256; expected lowercase SHA256"
        );
        let (file, artifact) = match manifest.data_repairs.as_deref() {
            None => {
                ensure!(
                    manifest.client_file.is_none() && manifest.original_client_sha256.is_none(),
                    "original build100 manifest must omit client_file and original_client_sha256"
                );
                ensure!(
                    manifest.client_sha256 == ORIGINAL_CLIENT_SHA256,
                    "original build100 client_sha256 must match pinned original identity"
                );
                ("client.swf", "original")
            }
            Some("july100-play-v1") => {
                ensure!(
                    manifest.client_file.as_deref() == Some("client-data-repaired.swf"),
                    "july100-play-v1 requires client_file client-data-repaired.swf"
                );
                ensure!(
                    manifest.original_client_sha256.as_deref() == Some(ORIGINAL_CLIENT_SHA256),
                    "july100-play-v1 requires pinned original_client_sha256"
                );
                ensure!(
                    manifest.client_sha256 != ORIGINAL_CLIENT_SHA256,
                    "repaired build100 client_sha256 must differ from original identity"
                );
                ("client-data-repaired.swf", "july100-play-v1")
            }
            other => anyhow::bail!("unsupported prepared build100 data_repairs: {other:?}"),
        };
        Ok(Self {
            file,
            artifact,
            sha256: manifest.client_sha256,
        })
    }

    fn load(directory: &Path) -> Result<Self> {
        let manifest = std::fs::read(directory.join("build100.json"))
            .context("missing or unreadable prepared build100 manifest build100.json")?;
        let selected = Self::parse(&manifest)?;
        let bytes = std::fs::read(directory.join(selected.file)).with_context(|| {
            format!(
                "missing or unreadable prepared build100 asset {}",
                selected.file
            )
        })?;
        let actual = format!("{:x}", Sha256::digest(bytes));
        ensure!(
            actual == selected.sha256,
            "prepared build100 asset {} SHA256 mismatch: expected {}, actual {actual}",
            selected.file,
            selected.sha256
        );
        Ok(selected)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Realm,
    Arena,
}
impl Mode {
    fn database_file(&self) -> &'static str {
        match self {
            Self::Realm => "build100-realm.sqlite",
            Self::Arena => "build100.sqlite",
        }
    }
    fn server_name(&self) -> &'static str {
        match self {
            Self::Realm => "July100 Reconstructed Realm",
            Self::Arena => "Shared Fixture (not Tutorial)",
        }
    }
}
struct Config {
    mode: Mode,
    listen: SocketAddr,
    client: PathBuf,
    state: PathBuf,
    public_origin: Option<String>,
}
impl Config {
    fn parse(args: Vec<String>) -> Result<Self> {
        let mut args = args.into_iter().peekable();
        let mut mode = Mode::Realm;
        let mut public_origin = None;
        while args.peek().is_some_and(|arg| arg.starts_with("--")) {
            match args.next().as_deref() {
                Some("--arena") => {
                    ensure!(mode != Mode::Arena, "duplicate --arena");
                    mode = Mode::Arena;
                }
                Some("--public-origin") => {
                    ensure!(public_origin.is_none(), "duplicate --public-origin");
                    let value = args
                        .next()
                        .context("--public-origin requires an HTTPS origin")?;
                    ensure!(
                        !value.starts_with("--"),
                        "--public-origin requires an HTTPS origin"
                    );
                    let url = url::Url::parse(&value).context("invalid --public-origin URL")?;
                    ensure!(
                        url.scheme() == "https"
                            && url.host_str().is_some()
                            && url.username().is_empty()
                            && url.password().is_none()
                            && url.origin().ascii_serialization() == value,
                        "--public-origin must be an exact canonical HTTPS origin without credentials, path, query or fragment"
                    );
                    public_origin = Some(value);
                }
                Some(_) => anyhow::bail!("unknown build100 option"),
                None => unreachable!(),
            }
        }
        ensure!(
            mode != Mode::Arena || public_origin.is_none(),
            "--arena is a local fixture and cannot be combined with --public-origin"
        );
        let args: Vec<_> = args.collect();
        ensure!(
            !args.iter().any(|arg| arg.starts_with("--")),
            "build100 options must precede LISTEN CLIENT_DIRECTORY STATE_DIRECTORY"
        );
        ensure!(
            args.len() == 3,
            "usage: openrotmg-build100 [--arena | --public-origin HTTPS_ORIGIN] LISTEN CLIENT_DIRECTORY STATE_DIRECTORY (default: reconstructed Tutorial/realm; --arena: local shared fixture; loopback only)"
        );
        let listen: SocketAddr = args[0]
            .parse()
            .context("invalid build100 LISTEN socket address")?;
        ensure!(
            listen.ip().is_loopback(),
            "build100 experimental anonymous protocol is restricted to loopback"
        );
        Ok(Self {
            mode,
            listen,
            client: PathBuf::from(&args[1]),
            state: PathBuf::from(&args[2]),
            public_origin,
        })
    }

    fn origin(&self, bound: SocketAddr) -> String {
        match &self.public_origin {
            Some(origin) => origin.clone(),
            None => format!("http://{bound}"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = match std::env::var("RUST_LOG") {
        Ok(value) => tracing_subscriber::EnvFilter::try_new(value)?,
        Err(std::env::VarError::NotPresent) => tracing_subscriber::EnvFilter::new("info"),
        Err(error) => return Err(error.into()),
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let config = Config::parse(std::env::args().skip(1).collect())?;
    let client = config.client.canonicalize()?;
    let prepared = PreparedClient::load(&client)?;
    std::fs::create_dir_all(&config.state)?;
    for f in ["index.html", "vendor/ruffle.js"] {
        ensure!(
            client.join(f).is_file(),
            "missing prepared build100 asset {f}; run scripts/prepare-build100.py"
        );
    }
    let content = Content::load(&client)?;
    let database_path = config.state.join(config.mode.database_file());
    let db = Arc::new(Mutex::new(database(&database_path)?));
    let simulation = match config.mode {
        Mode::Realm => lifecycle::Simulation::normal(
            content,
            Arc::new(content::Grounds::load(&client)?),
            db.clone(),
            REALM_SEED,
        )?,
        Mode::Arena => lifecycle::Simulation::new(content, db.clone()),
    };
    let (commands, receiver) = mpsc::channel(64);
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    let owner = tokio::spawn(simulation.run(receiver, true));
    let origin = config.origin(listener.local_addr()?);
    let app = Arc::new(App {
        db,
        commands,
        origin: origin.clone(),
    });
    let router = Router::new()
        .route("/game", get(upgrade))
        .route("/legacy/char/list", post(chars))
        .route(
            "/legacy/servers.txt",
            get(move || async move { format!("{},127.0.0.1\n", config.mode.server_name()) }),
        )
        .fallback_service(ServeDir::new(client))
        .with_state(app);
    tracing::info!(url=%origin,mode=?config.mode,state=%database_path.display(),seed=REALM_SEED,client_artifact=prepared.artifact,client_file=prepared.file,client_sha256=%prepared.sha256,"build100 ready; reconstructed Tutorial/realm by default, explicit --arena is fixture-only; local asset descriptor is not balance authentication");
    tokio::select! {
        result=axum::serve(listener,router).with_graceful_shutdown(async {let _=tokio::signal::ctrl_c().await;})=>result?,
        result=owner=>{result.context("build100 owner task panicked")??;anyhow::bail!("build100 owner stopped");}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPAIRED_FIXTURE: &[u8] = b"data-repaired fixture";

    fn repaired_manifest() -> serde_json::Value {
        serde_json::json!({
            "data_repairs": "july100-play-v1",
            "client_file": "client-data-repaired.swf",
            "original_client_sha256": ORIGINAL_CLIENT_SHA256,
            "client_sha256": format!("{:x}", Sha256::digest(REPAIRED_FIXTURE)),
        })
    }

    fn parse_manifest(value: &serde_json::Value) -> Result<PreparedClient> {
        PreparedClient::parse(&serde_json::to_vec(value).unwrap())
    }

    #[test]
    fn cli_prepared_client_explicit_original_and_repaired_contracts() {
        let original = parse_manifest(&serde_json::json!({
            "client_sha256": ORIGINAL_CLIENT_SHA256,
            "ruffle_zip_sha256": "metadata not used for client selection",
        }))
        .unwrap();
        assert_eq!(original.file, "client.swf");
        assert_eq!(original.artifact, "original");
        assert_eq!(original.sha256, ORIGINAL_CLIENT_SHA256);
        let repaired = parse_manifest(&repaired_manifest()).unwrap();
        assert_eq!(repaired.file, "client-data-repaired.swf");
        assert_eq!(repaired.artifact, "july100-play-v1");
        assert_eq!(
            repaired.sha256,
            format!("{:x}", Sha256::digest(REPAIRED_FIXTURE))
        );
    }

    #[test]
    fn cli_prepared_client_rejects_original_array_root() {
        let array = serde_json::json!([ORIGINAL_CLIENT_SHA256]);
        let rejected = ["", " \t\r\n"]
            .map(|prefix| PreparedClient::parse(format!("{prefix}{array}").as_bytes()).is_err());
        assert_eq!(rejected, [true, true]);
        let object = format!(" \t\r\n{{\"client_sha256\":\"{ORIGINAL_CLIENT_SHA256}\"}}");
        assert_eq!(
            PreparedClient::parse(object.as_bytes()).unwrap().artifact,
            "original"
        );
    }

    #[test]
    fn cli_prepared_client_rejects_repaired_array_root() {
        let array = serde_json::json!([
            format!("{:x}", Sha256::digest(REPAIRED_FIXTURE)),
            "july100-play-v1",
            "client-data-repaired.swf",
            ORIGINAL_CLIENT_SHA256,
        ]);
        let rejected = ["", " \t\r\n"]
            .map(|prefix| PreparedClient::parse(format!("{prefix}{array}").as_bytes()).is_err());
        assert_eq!(rejected, [true, true]);
        let object = format!(" \t\r\n{}", repaired_manifest());
        assert_eq!(
            PreparedClient::parse(object.as_bytes()).unwrap().artifact,
            "july100-play-v1"
        );
        let duplicate = format!(
            " \t\r\n{{\"data_repairs\":\"july100-play-v1\",{}",
            &repaired_manifest().to_string()[1..]
        );
        assert!(PreparedClient::parse(duplicate.as_bytes()).is_err());
    }

    #[test]
    fn cli_prepared_client_rejects_ambiguous_or_unsupported_metadata() {
        use serde_json::json;
        for (field, bad) in [
            ("data_repairs", json!(null)),
            ("data_repairs", json!("unknown")),
            ("data_repairs", json!(1)),
            ("client_file", json!(null)),
            ("client_file", json!("client.swf")),
            ("client_file", json!("../client-data-repaired.swf")),
            ("client_file", json!("/client-data-repaired.swf")),
            ("client_file", json!("other.swf")),
            ("original_client_sha256", json!(null)),
            ("original_client_sha256", json!("0".repeat(64))),
            ("client_sha256", json!(null)),
            ("client_sha256", json!("invalid")),
            ("client_sha256", json!("A".repeat(64))),
            ("client_sha256", json!(ORIGINAL_CLIENT_SHA256)),
        ] {
            let mut manifest = repaired_manifest();
            manifest[field] = bad;
            assert!(parse_manifest(&manifest).is_err(), "{manifest}");
        }
        for field in [
            "client_sha256",
            "client_file",
            "original_client_sha256",
            "data_repairs",
        ] {
            let mut manifest = repaired_manifest();
            manifest.as_object_mut().unwrap().remove(field);
            assert!(parse_manifest(&manifest).is_err(), "{manifest}");
        }
        for field in ["client_file", "original_client_sha256", "data_repairs"] {
            let mut manifest = json!({"client_sha256": ORIGINAL_CLIENT_SHA256});
            manifest[field] = json!(null);
            assert!(parse_manifest(&manifest).is_err(), "{manifest}");
            manifest[field] = json!("present");
            assert!(parse_manifest(&manifest).is_err(), "{manifest}");
        }
        for bytes in [b"null".as_slice(), b"[]", b"{}", b"{"] {
            assert!(PreparedClient::parse(bytes).is_err());
        }
        let duplicate = format!(
            "{{\"client_sha256\":\"{ORIGINAL_CLIENT_SHA256}\",\"client_sha256\":\"{ORIGINAL_CLIENT_SHA256}\"}}"
        );
        assert!(PreparedClient::parse(duplicate.as_bytes()).is_err());
        assert!(parse_manifest(&json!({"client_sha256": "0".repeat(64)})).is_err());
    }

    #[test]
    fn cli_prepared_client_hash_and_missing_selected_file_never_use_another_asset() {
        let path = std::env::temp_dir().join(format!(
            "build100-client-preflight-test-{}",
            rand::random::<u64>()
        ));
        std::fs::create_dir(&path).unwrap();
        let error = PreparedClient::load(&path).unwrap_err().to_string();
        assert!(error.contains("build100.json"), "{error}");
        std::fs::write(path.join("build100.json"), b"{").unwrap();
        assert!(
            PreparedClient::load(&path)
                .unwrap_err()
                .to_string()
                .contains("invalid")
        );
        let write_manifest = |value: &serde_json::Value| {
            std::fs::write(
                path.join("build100.json"),
                serde_json::to_vec(value).unwrap(),
            )
            .unwrap();
        };
        write_manifest(&repaired_manifest());
        std::fs::write(
            path.join("client.swf"),
            b"original decoy must not be selected",
        )
        .unwrap();
        std::fs::write(path.join("client-data-repaired.swf"), REPAIRED_FIXTURE).unwrap();
        assert_eq!(
            PreparedClient::load(&path).unwrap().artifact,
            "july100-play-v1"
        );
        std::fs::write(path.join("client-data-repaired.swf"), b"changed bytes").unwrap();
        let error = PreparedClient::load(&path).unwrap_err().to_string();
        assert!(
            error.contains("client-data-repaired.swf SHA256 mismatch"),
            "{error}"
        );
        std::fs::write(path.join("client-data-repaired.swf"), REPAIRED_FIXTURE).unwrap();
        let mut mismatch = repaired_manifest();
        mismatch["client_sha256"] = serde_json::json!("0".repeat(64));
        write_manifest(&mismatch);
        assert!(
            PreparedClient::load(&path)
                .unwrap_err()
                .to_string()
                .contains("SHA256 mismatch")
        );
        write_manifest(&repaired_manifest());
        std::fs::remove_file(path.join("client-data-repaired.swf")).unwrap();
        let error = PreparedClient::load(&path).unwrap_err().to_string();
        assert!(error.contains("client-data-repaired.swf"), "{error}");
        assert!(path.join("client.swf").is_file());
        write_manifest(&serde_json::json!({"client_sha256": ORIGINAL_CLIENT_SHA256}));
        let error = PreparedClient::load(&path).unwrap_err().to_string();
        assert!(error.contains("client.swf SHA256 mismatch"), "{error}");
        std::fs::remove_file(path.join("client.swf")).unwrap();
        std::fs::write(path.join("client-data-repaired.swf"), REPAIRED_FIXTURE).unwrap();
        let error = PreparedClient::load(&path).unwrap_err().to_string();
        assert!(error.contains("asset client.swf"), "{error}");
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    #[ignore = "requires private original BUILD100_CLIENT_DIR; run with --include-ignored"]
    fn cli_prepared_client_qualified_original_identity_is_verified() {
        let path =
            PathBuf::from(std::env::var_os("BUILD100_CLIENT_DIR").expect("BUILD100_CLIENT_DIR"));
        let client = PreparedClient::load(&path).unwrap();
        assert_eq!(client.file, "client.swf");
        assert_eq!(client.artifact, "original");
        assert_eq!(client.sha256, ORIGINAL_CLIENT_SHA256);
    }

    #[test]
    fn cli_default_realm_and_explicit_arena_keep_separate_state_files() {
        let args = || vec!["127.0.0.1:0".into(), "client".into(), "state".into()];
        let normal = Config::parse(args()).unwrap();
        let mut arena_args = args();
        arena_args.insert(0, "--arena".into());
        let arena = Config::parse(arena_args).unwrap();
        assert_eq!(normal.mode, Mode::Realm);
        assert_eq!(arena.mode, Mode::Arena);
        assert_eq!(
            normal.state.join(normal.mode.database_file()),
            PathBuf::from("state/build100-realm.sqlite")
        );
        assert_eq!(
            arena.state.join(arena.mode.database_file()),
            PathBuf::from("state/build100.sqlite")
        );
        assert_ne!(normal.mode.server_name(), arena.mode.server_name());
        assert_eq!(normal.listen.port(), 0);
        assert!(Config::parse(vec![]).is_err());
        assert!(
            Config::parse(vec!["0.0.0.0:8190".into(), "client".into(), "state".into()]).is_err()
        );
        assert!(Config::parse(vec!["--unknown".into(), "client".into(), "state".into()]).is_err());
    }
    fn cli(values: &[&str]) -> Result<Config> {
        Config::parse(values.iter().map(|value| (*value).into()).collect())
    }

    #[test]
    fn cli_local_origin_uses_bound_loopback_address() {
        for listen in ["127.0.0.1:0", "[::1]:0"] {
            for options in [vec![], vec!["--arena"]] {
                let args = [options, vec![listen, "client", "state"]].concat();
                let config = cli(&args).unwrap();
                let bound = SocketAddr::new(config.listen.ip(), 18910);
                assert_eq!(config.origin(bound), format!("http://{bound}"));
                assert!(config.public_origin.is_none());
            }
        }
    }

    #[test]
    fn cli_public_origin_is_explicit_and_keeps_realm_and_loopback() {
        for origin in [
            "https://openrotmg.org",
            "https://www.openrotmg.org",
            "https://preview.example:8443",
            "https://[::1]",
        ] {
            let config = cli(&[
                "--public-origin",
                origin,
                "127.0.0.1:18910",
                "client",
                "state",
            ])
            .unwrap();
            assert_eq!(config.origin(config.listen), origin);
            assert_eq!(config.listen, "127.0.0.1:18910".parse().unwrap());
            assert_eq!(config.mode, Mode::Realm);
            assert_eq!(config.mode.database_file(), "build100-realm.sqlite");
        }
    }

    #[test]
    fn cli_public_origin_rejects_noncanonical_or_non_https_values() {
        for origin in [
            "",
            "null",
            "openrotmg.org",
            "http://openrotmg.org",
            "wss://openrotmg.org",
            "HTTPS://openrotmg.org",
            "https://OpenRotMG.org",
            "https://openrotmg.org:443",
            "https://openrotmg.org/",
            "https://openrotmg.org/2010",
            "https://openrotmg.org?",
            "https://openrotmg.org#",
            "https://user@openrotmg.org",
            "https://user:password@openrotmg.org",
            "https://@openrotmg.org",
            " https://openrotmg.org",
            "https://openrotmg.org\n",
            "https://openrotmg.org\\path",
            "https:///openrotmg.org",
            "https://openrotmg.org:bad",
            "https://",
            "https://openrotmg.org,https://other.example",
        ] {
            assert!(
                cli(&[
                    "--public-origin",
                    origin,
                    "127.0.0.1:18910",
                    "client",
                    "state"
                ])
                .is_err(),
                "{origin:?}"
            );
        }
    }

    #[test]
    fn cli_rejects_duplicate_missing_unknown_misplaced_and_contradictory_options() {
        for options in [
            vec!["--arena", "--arena"],
            vec!["--unknown"],
            vec!["--public-origin"],
            vec!["--public-origin", "--arena"],
            vec!["--public-origin=https://openrotmg.org"],
            vec![
                "--public-origin",
                "https://openrotmg.org",
                "--public-origin",
                "https://openrotmg.org",
            ],
            vec!["--arena", "--public-origin", "https://openrotmg.org"],
            vec!["--public-origin", "https://openrotmg.org", "--arena"],
        ] {
            assert!(cli(&options).is_err(), "{options:?}");
            let args = [options, vec!["127.0.0.1:18910", "client", "state"]].concat();
            assert!(cli(&args).is_err(), "{args:?}");
        }
        for args in [
            vec!["127.0.0.1:18910", "--arena", "client", "state"],
            vec![
                "127.0.0.1:18910",
                "client",
                "state",
                "--public-origin",
                "https://openrotmg.org",
            ],
            vec![
                "--public-origin",
                "https://openrotmg.org",
                "0.0.0.0:18910",
                "client",
                "state",
            ],
            vec![
                "--public-origin",
                "https://openrotmg.org",
                "[::]:18910",
                "client",
                "state",
            ],
            vec!["127.0.0.1:18910", "client"],
            vec!["127.0.0.1:18910", "client", "state", "extra"],
            vec!["--arena", "0.0.0.0:18910", "client", "state"],
        ] {
            assert!(cli(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn cli_unknown_option_diagnostics_never_echo_credential_bearing_arguments() {
        const USER: &str = "P1_TEST_USER_SENTINEL";
        const SECRET: &str = "P1_TEST_SECRET_SENTINEL";
        for argument in [
            format!("--public-origin=https://{USER}:{SECRET}@example.invalid"),
            format!("--unsupported=https://{USER}:{SECRET}@example.invalid"),
            format!("--{USER}:{SECRET}"),
        ] {
            let error = cli(&[&argument, "127.0.0.1:18910", "client", "state"])
                .err()
                .expect("credential-bearing unknown option must be rejected");
            for diagnostic in [
                format!("{error}"),
                format!("{error:#}"),
                format!("{error:?}"),
                format!("{error:#?}"),
            ] {
                for forbidden in [argument.as_str(), USER, SECRET] {
                    assert!(
                        !diagnostic.contains(forbidden),
                        "unknown-option diagnostic disclosed credential-bearing input"
                    );
                }
            }
        }
    }

    #[test]
    fn upgrade_origin_authorization_uses_only_one_exact_configured_origin() {
        use axum::http::HeaderValue;
        for expected in [
            "https://openrotmg.org",
            "https://www.openrotmg.org",
            "http://127.0.0.1:18910",
            "http://[::1]:18910",
        ] {
            let mut headers = HeaderMap::new();
            assert!(!authorized_origin(&headers, expected));
            headers.insert(
                "forwarded",
                HeaderValue::from_static("host=openrotmg.org;proto=https"),
            );
            headers.insert(
                "x-forwarded-host",
                HeaderValue::from_static("openrotmg.org"),
            );
            headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
            assert!(!authorized_origin(&headers, expected));
            for rejected in [
                "null",
                "https://openrotmg.org/",
                "https://evil.example",
                "http://localhost:18910",
                "https://openrotmg.org,http://127.0.0.1:18910",
            ] {
                headers.insert("origin", HeaderValue::from_str(rejected).unwrap());
                assert!(!authorized_origin(&headers, expected), "{rejected}");
            }
            headers.insert("origin", HeaderValue::from_bytes(&[0xff]).unwrap());
            assert!(!authorized_origin(&headers, expected));
            headers.insert("origin", HeaderValue::from_str(expected).unwrap());
            assert!(authorized_origin(&headers, expected));
            headers.append("origin", HeaderValue::from_str(expected).unwrap());
            assert!(!authorized_origin(&headers, expected));
        }
        let public = cli(&[
            "--public-origin",
            "https://www.openrotmg.org",
            "127.0.0.1:18910",
            "client",
            "state",
        ])
        .unwrap();
        let expected = public.origin(public.listen);
        let mut headers = HeaderMap::new();
        for rejected in ["http://127.0.0.1:18910", "https://openrotmg.org"] {
            headers.insert("origin", HeaderValue::from_static(rejected));
            assert!(!authorized_origin(&headers, &expected));
        }
        headers.insert(
            "origin",
            HeaderValue::from_static("https://www.openrotmg.org"),
        );
        assert!(authorized_origin(&headers, &expected));
    }

    #[test]
    fn restart_preserves_rewards_inventory_and_permanent_death() {
        let path = std::env::temp_dir().join(format!(
            "openrotmg-build100-{}.sqlite",
            rand::random::<u64>()
        ));
        let open = || App {
            db: Arc::new(Mutex::new(database(&path).unwrap())),
            commands: mpsc::channel(1).0,
            origin: "http://127.0.0.1:8190".into(),
        };
        let id = "0123456789012345678901234567890123456789";
        let mut p = Character {
            generation: 1,
            kind: 0x30e,
            name: "Test".into(),
            x: 16.5,
            y: 16.5,
            stats: std::collections::BTreeMap::from([(1, 70), (6, 15), (12, 0xa22)]),
            alive: true,
            tutorial_complete: false,
            resources_version: 0,
            resources_carry: None,
            action_state: None,
        };
        {
            let app = open();
            app.save(id, &p).unwrap();
        }
        {
            let app = open();
            let got = app.character(id).unwrap().unwrap();
            assert_eq!(got.stats, p.stats);
            assert!(got.alive);
            p.alive = false;
            p.stats.insert(1, 0);
            app.save(id, &p).unwrap();
        }
        {
            let app = open();
            assert!(!app.character(id).unwrap().unwrap().alive);
            p.generation = 2;
            p.alive = true;
            p.stats.insert(1, 100);
            app.save(id, &p).unwrap();
        }
        {
            let app = open();
            assert_eq!(app.character(id).unwrap().unwrap().generation, 2);
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn anonymous_id_validation_rejects_markup_and_password_shapes() {
        assert!(guid("0123456789012345678901234567890123456789").is_ok());
        for id in ["", "a@b.com", "<Chars>", "../state"] {
            assert!(guid(id).is_err());
        }
    }
}
