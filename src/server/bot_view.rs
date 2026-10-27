use super::*;
pub(super) mod perspective;
use crate::{
    bots::cctv::Viewer,
    world::spectator::{Attachment, Projection},
};
use axum::extract::{Path, RawQuery, ws::CloseFrame};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(super) async fn socket(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
    ws: WebSocketUpgrade,
) -> Response {
    let cursor = match cursor_capability(query.as_deref()) {
        Ok(cursor) => cursor,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    let Some(origin) = &a.bot_view_origin else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "bot_view_origin must be configured for native bot viewing",
        )
            .into_response();
    };
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(origin.as_str()) {
        return (
            StatusCode::FORBIDDEN,
            "native bot view origin is not approved",
        )
            .into_response();
    }
    let mut forwarded = headers.clone();
    // The native iframe has its own origin; preserve the existing player's origin policy on every other route.
    forwarded.remove("origin");
    let admitted = request_ip(&a, &forwarded, peer)
        .map_err(|e| (StatusCode::FORBIDDEN, e.to_string()))
        .and_then(|ip| Viewer::join(&a.cctv, &name, ip, true));
    let viewer = match admitted {
        Ok(v) => v,
        Err((status, error)) => {
            return (status, Json(serde_json::json!({"error":error}))).into_response();
        }
    };
    ws.max_message_size(65_536)
        .max_frame_size(65_536)
        .on_upgrade(move |socket| async move {
            let _viewer = viewer;
            let mut stream = Stream::new(socket, a.shutdown.subscribe());
            let result = stream.watch(&a, &name, cursor).await;
            if let Err(error) = result {
                tracing::warn!(bot=name, error=%error, "native bot view ended with error");
                let reason = error.to_string().chars().take(110).collect::<String>();
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    stream.socket.send(Message::Close(Some(CloseFrame {
                        code: 1011,
                        reason: reason.into(),
                    }))),
                )
                .await;
            }
            tracing::info!(
                bot = name,
                ignored_inputs = stream.ignored,
                "native bot viewer left"
            );
        })
}

fn cursor_capability(query: Option<&str>) -> Result<bool> {
    match query {
        None => Ok(false),
        Some("cursor=1") => Ok(true),
        Some(_) => bail!("native bot viewer query must be exactly cursor=1 or absent"),
    }
}

#[cfg(test)]
#[test]
fn cursor_negotiation_is_explicit_and_strict() {
    assert!(!cursor_capability(None).unwrap());
    assert!(cursor_capability(Some("cursor=1")).unwrap());
    for query in [
        "",
        "cursor=0",
        "cursor=2",
        "cursor=true",
        "cursor=1&cursor=1",
        "other=1",
        "cursor=1&other=1",
    ] {
        assert!(cursor_capability(Some(query)).is_err(), "{query}");
    }
}

fn attachment_changed(current: Option<(i32, i32)>, player: i32, account: i32) -> bool {
    current != Some((player, account))
}
fn reconnect_packet() -> Packet {
    let mut reconnect = Writer::new();
    reconnect
        .utf("Bot CCTV")
        .utf("")
        .i32(2050)
        .i32(-2)
        .i32(0)
        .i16(0);
    Packet {
        id: 39,
        body: reconnect.0,
    }
}

#[cfg(test)]
#[test]
fn attachment_transition_uses_normal_native_reconnect_packet() {
    assert!(!attachment_changed(Some((7, 1)), 7, 1));
    for binding in [None, Some((8, 1)), Some((7, 2))] {
        assert!(attachment_changed(binding, 7, 1));
        let packet = reconnect_packet();
        assert_eq!(packet.id, 39);
        let mut r = Reader::new(&packet.body);
        assert_eq!(r.utf().unwrap(), "Bot CCTV");
        assert_eq!(r.utf().unwrap(), "");
        assert_eq!(r.i32().unwrap(), 2050);
        assert_eq!(r.i32().unwrap(), -2);
        assert_eq!(r.i32().unwrap(), 0);
        assert_eq!(r.i16().unwrap(), 0);
        r.done().unwrap();
    }
}

struct Stream {
    socket: WebSocket,
    codec: Codec,
    pending: VecDeque<Packet>,
    shutdown: tokio::sync::watch::Receiver<bool>,
    input_window: Instant,
    inputs: usize,
    ignored: u64,
}
impl Stream {
    fn new(socket: WebSocket, shutdown: tokio::sync::watch::Receiver<bool>) -> Self {
        Self {
            socket,
            codec: Codec::server(),
            pending: VecDeque::new(),
            shutdown,
            input_window: Instant::now(),
            inputs: 0,
            ignored: 0,
        }
    }
    async fn output(&mut self, id: u8, body: Vec<u8>) -> Result<()> {
        let frame = self.codec.frame(id, body);
        tokio::select! {
            _ = async { let _ = self.shutdown.wait_for(|stop| *stop).await; } => bail!("server is stopping"),
            sent = tokio::time::timeout(Duration::from_secs(2), self.socket.send(Message::Binary(frame.into()))) => sent.context("native bot viewer is too slow")??,
        }
        Ok(())
    }
    async fn reconnect(&mut self) -> Result<()> {
        let packet = reconnect_packet();
        self.output(packet.id, packet.body).await
    }
    async fn input(&mut self) -> Result<Option<Packet>> {
        loop {
            if let Some(packet) = self.pending.pop_front() {
                if self.input_window.elapsed() >= Duration::from_secs(1) {
                    self.input_window = Instant::now();
                    self.inputs = 0;
                }
                self.inputs += 1;
                ensure!(self.inputs <= 512, "native bot viewer input rate exceeded");
                return Ok(Some(packet));
            }
            match self.socket.recv().await {
                Some(Ok(Message::Binary(bytes))) => self.pending.extend(self.codec.feed(&bytes)?),
                Some(Ok(Message::Close(_))) | None => return Ok(None),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Text(_))) => bail!("native bot viewer requires binary packets"),
                Some(Err(error)) => return Err(error.into()),
            }
        }
    }
    async fn attach(a: &App, name: &str) -> Result<Attachment> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some((player, account)) = a.cctv.binding(name)
                    && let Some(attached) = a.world.spectate(player, account).await?
                {
                    return Ok::<_, anyhow::Error>(attached);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .context("bot has no living session; it may be replacing a dead character or offline")?
    }
    async fn watch(&mut self, a: &App, name: &str, cursor: bool) -> Result<()> {
        let hello = tokio::time::timeout(Duration::from_secs(10), self.input())
            .await??
            .context("viewer closed before HELLO")?;
        ensure!(
            hello.id == wire::HELLO && Reader::new(&hello.body).utf()? == "123.5.1",
            "native bot viewer requires a 123.5.1 HELLO"
        );
        let mut attached = tokio::select! {
            _ = async { let _ = self.shutdown.wait_for(|stop| *stop).await; } => bail!("server is stopping"),
            result = Self::attach(a,name) => result?,
        };
        self.output(wire::MAP_INFO, attached.map).await?;
        let entry = tokio::time::timeout(Duration::from_secs(10), self.input())
            .await??
            .context("viewer closed before LOAD")?;
        ensure!(
            matches!(entry.id, wire::LOAD | wire::CREATE),
            "native bot viewer requires LOAD after MAP_INFO"
        );
        let mut own = Writer::new();
        own.i32(attached.player).i32(attached.character);
        self.output(wire::CREATE_SUCCESS, own.0).await?;
        let mut projection = Projection::new(attached.player);
        for packet in attached.snapshot {
            let packet = projection
                .packet(packet, &a.content)?
                .context("snapshot packet was not visual")?;
            self.output(packet.id, packet.body).await?;
        }
        tracing::info!(
            bot = name,
            player = attached.player,
            character = attached.character,
            "native bot viewer attached"
        );
        let mut last_cursor_tick = None;
        let mut stop = self.shutdown.clone();
        let mut cursor_ticks = tokio::time::interval(Duration::from_millis(50));
        cursor_ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = async { let _ = stop.wait_for(|stop| *stop).await; } => break,
                _ = cursor_ticks.tick(), if cursor => {
                    if attachment_changed(a.cctv.binding(name), attached.player, attached.account) {
                        self.reconnect().await?;
                        break;
                    }
                    if let Some(packet) = a.world.spectator_cursor(attached.world, attached.player, attached.account, attached.character).await?
                        && crate::world::spectator::advance_cursor(&mut last_cursor_tick, &packet)?
                    {
                        self.output(packet.id, packet.body).await?;
                    }
                },
                packet = self.input() => {
                    let Some(packet)=packet? else { break; };
                    // The recovered engine still reports collision/ACK packets; none belong to a gameplay session here.
                    if !matches!(packet.id, wire::MOVE | wire::UPDATE_ACK | wire::PONG | 14 | 24) {
                        self.ignored+=1;
                        if self.ignored==1 { tracing::info!(bot=name, packet=packet.id, "read-only bot viewer input ignored"); }
                    }
                },
                packet = attached.receiver.recv() => match packet {
                    Ok(packet) => if let Some(packet)=projection.packet(packet,&a.content)? { self.output(packet.id,packet.body).await?; },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        self.reconnect().await?;
                        break;
                    },
                    Err(error) => return Err(error).context("native bot view overflowed; reconnect for a fresh snapshot"),
                }
            }
        }
        Ok(())
    }
}
