use super::*;
pub(crate) mod control;
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
    let admitted = control::native_ip(&a, &headers, peer)
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
            let result = stream.watch(&a, &name, cursor, None).await;
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

fn cursor_capability(query: Option<&str>) -> Result<u8> {
    match query {
        None => Ok(0),
        Some("cursor=1") => Ok(1),
        Some("cursor=2") => Ok(2),
        Some(_) => bail!("native bot viewer query must be exactly cursor=1, cursor=2 or absent"),
    }
}

#[cfg(test)]
#[test]
fn cursor_negotiation_is_explicit_and_strict() {
    assert_eq!(cursor_capability(None).unwrap(), 0);
    assert_eq!(cursor_capability(Some("cursor=1")).unwrap(), 1);
    assert_eq!(cursor_capability(Some("cursor=2")).unwrap(), 2);
    for query in [
        "",
        "cursor=0",
        "cursor=3",
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

async fn view_changed(changes: &mut Option<tokio::sync::watch::Receiver<()>>) {
    match changes {
        Some(changes) => {
            let _ = changes.changed().await;
        }
        None => std::future::pending().await,
    }
}

enum VisualEvent {
    Cursor,
    Input(Result<Option<Packet>>),
    Packet(std::result::Result<Packet, tokio::sync::broadcast::error::RecvError>),
}

async fn next_visual(
    input: impl std::future::Future<Output = Result<Option<Packet>>>,
    receiver: &mut tokio::sync::broadcast::Receiver<Packet>,
    ticks: &mut tokio::time::Interval,
    cursor: bool,
) -> VisualEvent {
    tokio::select! {
        biased;
        _ = ticks.tick(), if cursor => VisualEvent::Cursor,
        input = input => VisualEvent::Input(input),
        packet = receiver.recv() => VisualEvent::Packet(packet),
    }
}

#[cfg(test)]
#[tokio::test]
async fn slow_cursor_does_not_starve_queued_native_packets_or_close() {
    let (sender, mut receiver) = tokio::sync::broadcast::channel(4);
    for _ in 0..4 {
        sender
            .send(Packet {
                id: wire::NEW_TICK,
                body: vec![],
            })
            .unwrap();
    }
    let mut ticks = tokio::time::interval(Duration::from_millis(50));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut received = 0;
    let mut closed = false;
    for _ in 0..64 {
        match next_visual(
            async {
                if received == 4 {
                    Ok(None)
                } else {
                    std::future::pending().await
                }
            },
            &mut receiver,
            &mut ticks,
            true,
        )
        .await
        {
            VisualEvent::Cursor => tokio::time::sleep(Duration::from_millis(60)).await,
            VisualEvent::Packet(Ok(packet)) => {
                assert_eq!(packet.id, wire::NEW_TICK);
                received += 1;
            }
            VisualEvent::Input(Ok(None)) => {
                closed = true;
                break;
            }
            _ => panic!("unexpected native view event"),
        }
    }
    assert_eq!(received, 4, "cursor sampling starved native ticks");
    assert!(closed, "cursor sampling starved socket close");
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
    async fn attach(a: &App, name: &str) -> Result<Option<Attachment>> {
        let mut absent = false;
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                absent = false;
                if let Some((player, account)) = a.cctv.binding(name)
                    && let Some(attached) = a.world.spectate(player, account).await?
                {
                    return Ok::<_, anyhow::Error>(attached);
                }
                absent = true;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        match result {
            Ok(attached) => attached.map(Some),
            Err(_) if absent => Ok(None),
            Err(_) => bail!("bot attachment world lookup timed out"),
        }
    }
    async fn bootstrap(
        &mut self,
        a: &App,
        name: &str,
        changes: &mut Option<tokio::sync::watch::Receiver<()>>,
        lease: Option<&control::Lease>,
    ) -> Result<Option<(Attachment, Projection)>> {
        let mut stop = self.shutdown.clone();
        let hello = tokio::select! {
            biased;
            _ = async { let _ = stop.wait_for(|stop| *stop).await; } => return Ok(None),
            _ = view_changed(changes) => return Ok(None),
            result = tokio::time::timeout(Duration::from_secs(10), self.input()) => result??.context("viewer closed before HELLO")?,
        };
        ensure!(
            hello.id == wire::HELLO && Reader::new(&hello.body).utf()? == "123.5.1",
            "native bot viewer requires a 123.5.1 HELLO"
        );
        let attached = tokio::select! {
            biased;
            _ = async { let _ = stop.wait_for(|stop| *stop).await; } => return Ok(None),
            _ = view_changed(changes) => return Ok(None),
            result = Self::attach(a,name) => result?,
        };
        let mut attached = match attached {
            Some(attached) => attached,
            None => {
                let lease = lease.context(
                    "bot has no living session; it may be replacing a dead character or offline",
                )?;
                if !lease.unavailable() {
                    return Ok(None);
                }
                let mut poll = tokio::time::interval(Duration::from_secs(1));
                poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        biased;
                        _ = async { let _ = stop.wait_for(|stop| *stop).await; } => return Ok(None),
                        _ = view_changed(changes) => return Ok(None),
                        packet = self.input() => {
                            packet?.context("viewer closed while source unavailable")?;
                            self.ignored += 1;
                        },
                        _ = poll.tick() => {
                            if let Some((player, account)) = a.cctv.binding(name)
                                && let Some(attached) = tokio::time::timeout(Duration::from_secs(2), a.world.spectate(player, account))
                                    .await.context("bot source availability lookup timed out")??
                            { break attached; }
                        }
                    }
                }
            }
        };
        self.output(wire::MAP_INFO, std::mem::take(&mut attached.map))
            .await?;
        let entry = tokio::select! {
            biased;
            _ = async { let _ = stop.wait_for(|stop| *stop).await; } => return Ok(None),
            _ = view_changed(changes) => return Ok(None),
            result = tokio::time::timeout(Duration::from_secs(10), self.input()) => result??.context("viewer closed before LOAD")?,
        };
        ensure!(
            matches!(entry.id, wire::LOAD | wire::CREATE),
            "native bot viewer requires LOAD after MAP_INFO"
        );
        let mut own = Writer::new();
        own.i32(attached.player).i32(attached.character);
        self.output(wire::CREATE_SUCCESS, own.0).await?;
        let mut projection = Projection::new(attached.player);
        for packet in std::mem::take(&mut attached.snapshot) {
            if let Some(changes) = changes
                && changes.has_changed()?
            {
                return Ok(None);
            }
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
        Ok(Some((attached, projection)))
    }
    async fn transition(&mut self, lease: &mut Option<&mut control::Lease>) -> Result<()> {
        if let Some(lease) = lease {
            lease.release();
            if lease.closed() {
                return Ok(());
            }
        }
        self.reconnect().await
    }
    async fn watch(
        &mut self,
        a: &App,
        name: &str,
        cursor: u8,
        mut lease: Option<&mut control::Lease>,
    ) -> Result<()> {
        let mut changes = lease.as_ref().map(|lease| lease.changed.clone());
        let bootstrap = self
            .bootstrap(a, name, &mut changes, lease.as_deref())
            .await?;
        let Some((mut attached, mut projection)) = bootstrap else {
            if *self.shutdown.borrow() {
                return Ok(());
            }
            return self.transition(&mut lease).await;
        };
        if let Some(current) = &lease
            && !current.attached(a, &attached)
        {
            return self.transition(&mut lease).await;
        }
        let mut last_cursor_tick = None;
        let mut stop = self.shutdown.clone();
        let mut cursor_ticks = tokio::time::interval(Duration::from_millis(50));
        cursor_ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = async { let _ = stop.wait_for(|stop| *stop).await; } => break,
                _ = view_changed(&mut changes) => {
                    self.transition(&mut lease).await?;
                    break;
                },
                event = next_visual(self.input(), &mut attached.receiver, &mut cursor_ticks, cursor != 0) => match event {
                VisualEvent::Cursor => {
                    if attachment_changed(a.cctv.binding(name), attached.player, attached.account) {
                        self.transition(&mut lease).await?;
                        break;
                    }
                    let packets = tokio::select! {
                        biased;
                        _ = async { let _ = stop.wait_for(|stop| *stop).await; } => break,
                        _ = view_changed(&mut changes) => {
                            self.transition(&mut lease).await?;
                            break;
                        },
                        result = a.world.spectator_inputs(attached.world, attached.player, attached.account, attached.character, cursor) => result?,
                    };
                    if let Some(packets) = packets
                        && crate::world::spectator::advance_cursor(&mut last_cursor_tick, &packets[0])?
                    {
                        for packet in packets { self.output(packet.id, packet.body).await?; }
                    }
                },
                VisualEvent::Input(packet) => {
                    let Some(packet)=packet? else { break; };
                    // The recovered engine still reports collision/ACK packets; none belong to a gameplay session here.
                    if !matches!(packet.id, wire::MOVE | wire::UPDATE_ACK | wire::PONG | 14 | 24) {
                        self.ignored+=1;
                        if self.ignored==1 { tracing::info!(bot=name, packet=packet.id, "read-only bot viewer input ignored"); }
                    }
                },
                VisualEvent::Packet(packet) => match packet {
                    Ok(packet) => if let Some(packet)=projection.packet(packet,&a.content)? { self.output(packet.id,packet.body).await?; },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        self.transition(&mut lease).await?;
                        break;
                    },
                    Err(error) => return Err(error).context("native bot view overflowed; reconnect for a fresh snapshot"),
                }
                }
            }
        }
        Ok(())
    }
}
