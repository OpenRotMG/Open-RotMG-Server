use super::*;
#[cfg(test)]
mod tests;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{net::IpAddr, sync::Mutex};
use tokio::sync::{mpsc, watch};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    generation: u64,
    target: Target,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Target {
    Bot { bot: String },
    Player { source: i32, target: i32 },
}
struct ViewState {
    viewer: Option<Viewer>,
    bot: String,
    generation: u64,
    next_socket: u64,
    active: Option<u64>,
    attached: Option<(i32, i32)>,
    deadline: Instant,
}
pub(crate) struct View {
    ip: IpAddr,
    state: Mutex<ViewState>,
    changed: watch::Sender<()>,
    replies: mpsc::Sender<Value>,
    lifetime: tokio::sync::Notify,
}
impl View {
    fn new(viewer: Viewer, bot: String, ip: IpAddr) -> (Arc<Self>, mpsc::Receiver<Value>) {
        let (changed, _) = watch::channel(());
        let (replies, receiver) = mpsc::channel(1);
        (
            Arc::new(Self {
                ip,
                state: Mutex::new(ViewState {
                    viewer: Some(viewer),
                    bot,
                    generation: 0,
                    next_socket: 0,
                    active: None,
                    attached: None,
                    deadline: Instant::now() + Duration::from_secs(70),
                }),
                changed,
                replies,
                lifetime: tokio::sync::Notify::new(),
            }),
            receiver,
        )
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, ViewState> {
        self.state.lock().expect("native bot view poisoned")
    }
    fn close(&self) {
        self.close_socket(None);
    }
    fn close_socket(&self, socket: Option<u64>) {
        let viewer = {
            let mut state = self.lock();
            if socket.is_some_and(|socket| state.next_socket != socket) {
                return;
            }
            state.attached = None;
            let viewer = state.viewer.take();
            self.changed.send_replace(());
            self.lifetime.notify_one();
            viewer
        };
        // Viewer::drop takes the registry lock; never drop it while a view/registry lookup is locked.
        drop(viewer);
    }
    fn acquire(self: &Arc<Self>, ip: IpAddr) -> Result<Lease> {
        let mut state = self.lock();
        ensure!(ip == self.ip, "native view belongs to another address");
        ensure!(state.viewer.is_some(), "native view is closed");
        ensure!(
            state.active.is_none(),
            "native view already has a data socket"
        );
        ensure!(
            Instant::now() < state.deadline,
            "native view data connection deadline expired"
        );
        state.next_socket = state
            .next_socket
            .checked_add(1)
            .context("native view socket sequence exhausted")?;
        state.active = Some(state.next_socket);
        self.lifetime.notify_one();
        Ok(Lease {
            view: self.clone(),
            id: state.next_socket,
            bot: state.bot.clone(),
            generation: state.generation,
            changed: self.changed.subscribe(),
        })
    }
    fn selection(&self) -> (String, u64) {
        let state = self.lock();
        (state.bot.clone(), state.generation)
    }
    fn commit(
        &self,
        generation: u64,
        bot: &str,
        source: Option<(u64, (i32, i32))>,
    ) -> Result<Value> {
        let mut state = self.lock();
        ensure!(state.viewer.is_some(), "native view is closed");
        ensure!(
            state.generation.checked_add(1) == Some(generation),
            "stale switch generation"
        );
        if let Some((socket, source)) = source {
            ensure!(
                state.active == Some(socket) && state.attached == Some(source),
                "native perspective attachment changed"
            );
        }
        state
            .viewer
            .as_mut()
            .context("native view is closed")?
            .retarget(bot)?;
        state.bot = bot.to_owned();
        state.generation = generation;
        state.attached = None;
        self.changed.send_replace(());
        Ok(json!({"type":"selected","bot":bot,"generation":generation}))
    }
    async fn select(&self, a: &App, request: &Request) -> Result<Value> {
        let (bot, generation, active, attached) = {
            let state = self.lock();
            ensure!(state.viewer.is_some(), "native view is closed");
            ensure!(
                state.generation.checked_add(1) == Some(request.generation),
                "stale switch generation"
            );
            (
                state.bot.clone(),
                state.generation,
                state.active,
                state.attached,
            )
        };
        let (target, source) = match &request.target {
            Target::Bot { bot } => (bot.clone(), None),
            Target::Player { source, target } => {
                let bound = attached.context("native perspective is not attached")?;
                ensure!(bound.0 == *source, "stale native perspective source");
                ensure!(
                    a.cctv.allow_perspective(self.ip),
                    "too many perspective inspections; retry shortly"
                );
                let target = perspective::resolve_target(a, &bot, *source, *target)
                    .await
                    .map_err(|(_, error)| anyhow::anyhow!(error))?;
                (
                    target,
                    Some((active.context("native data socket is not attached")?, bound)),
                )
            }
        };
        ensure!(
            self.selection() == (bot, generation),
            "native selection changed during inspection"
        );
        self.commit(request.generation, &target, source)
    }
}
pub(super) struct Lease {
    view: Arc<View>,
    id: u64,
    pub(super) bot: String,
    generation: u64,
    pub(super) changed: watch::Receiver<()>,
}
impl Lease {
    pub(super) fn closed(&self) -> bool {
        self.view.lock().viewer.is_none()
    }
    pub(super) fn release(&mut self) {
        let mut state = self.view.lock();
        if state.active == Some(self.id) {
            state.active = None;
            state.attached = None;
            state.deadline = Instant::now() + Duration::from_secs(15);
            self.view.lifetime.notify_one();
        }
    }
    pub(super) fn attached(&self, a: &App, attached: &Attachment) -> Result<bool> {
        let mut state = self.view.lock();
        if state.viewer.is_none()
            || state.active != Some(self.id)
            || state.generation != self.generation
            || a.cctv.binding(&self.bot) != Some((attached.player, attached.account))
        {
            return Ok(false);
        }
        state.attached = Some((attached.player, attached.character));
        self.view
            .replies
            .try_send(
                json!({"type":"attached","bot":self.bot,"generation":self.generation,
            "player":attached.player,"character":attached.character}),
            )
            .context("native control reply queue is full or closed")?;
        Ok(true)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.release();
    }
}
struct Owner {
    view: Arc<View>,
    cctv: Arc<crate::bots::cctv::Cctv>,
    token: String,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.view.close();
        self.cctv.forget_view(&self.token);
    }
}
fn native_ip(
    a: &App,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> Result<IpAddr, (StatusCode, String)> {
    let origin = a.bot_view_origin.as_deref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "native bot viewing is not configured".into(),
    ))?;
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(origin) {
        return Err((
            StatusCode::FORBIDDEN,
            "native bot view origin is not approved".into(),
        ));
    }
    let mut headers = headers.clone();
    headers.remove("origin");
    request_ip(a, &headers, peer).map_err(|e| (StatusCode::FORBIDDEN, e.to_string()))
}
fn rejected_response((status, error): (StatusCode, String)) -> Response {
    (status, Json(json!({"error":error}))).into_response()
}
pub(in crate::server) async fn socket(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(bot): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let ip = match native_ip(&a, &headers, peer) {
        Ok(ip) => ip,
        Err(error) => return rejected_response(error),
    };
    let viewer = match Viewer::join(&a.cctv, &bot, ip, true) {
        Ok(viewer) => viewer,
        Err(error) => return rejected_response(error),
    };
    let (view, replies) = View::new(viewer, bot, ip);
    let token = match a.cctv.register_view(&view) {
        Ok(token) => token,
        Err(error) => return rejected_response(error),
    };
    let owner = Owner {
        view,
        token,
        cctv: a.cctv.clone(),
    };
    ws.max_message_size(512)
        .max_frame_size(512)
        .on_upgrade(move |mut socket| async move {
            if let Err(error) = run(&a, &owner, &mut socket, replies).await {
                owner.view.close();
                tracing::warn!(error=%error, "native bot control ended with error");
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    socket.send(Message::Close(Some(CloseFrame {
                        code: 1011,
                        reason: error
                            .to_string()
                            .chars()
                            .take(110)
                            .collect::<String>()
                            .into(),
                    }))),
                )
                .await;
            }
        })
}
async fn send(socket: &mut WebSocket, value: Value) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(2),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .context("native control viewer is too slow")??;
    Ok(())
}
async fn run(
    a: &App,
    owner: &Owner,
    socket: &mut WebSocket,
    mut replies: mpsc::Receiver<Value>,
) -> Result<()> {
    let (bot, generation) = owner.view.selection();
    send(socket, json!({"type":"ready","view":owner.token,"bot":bot,"generation":generation,"parent_origin":a.origin})).await?;
    let mut shutdown = a.shutdown.subscribe();
    let mut ticks = tokio::time::interval(Duration::from_secs(5));
    let mut last_pong = Instant::now();
    let mut window = Instant::now();
    let mut commands = 0;
    loop {
        let deadline = {
            let state = owner.view.lock();
            ensure!(state.viewer.is_some(), "native view is closed");
            state.active.is_none().then_some(state.deadline)
        };
        tokio::select! {
            _ = async { let _ = shutdown.wait_for(|stop| *stop).await; } => return Ok(()),
            _ = owner.view.lifetime.notified() => {},
            _ = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                    None => std::future::pending().await,
                }
            } => {
                let state = owner.view.lock();
                ensure!(state.active.is_some() || Instant::now() < state.deadline, "native data connection deadline expired");
            },
            _ = tokio::time::sleep_until((last_pong + Duration::from_secs(15)).into()) => bail!("native control heartbeat expired"),
            _ = ticks.tick() => {
                tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Ping(vec![].into())))
                    .await.context("native control ping timed out")??;
            },
            Some(reply) = replies.recv() => send(socket, reply).await?,
            message = socket.recv() => match message {
                None | Some(Ok(Message::Close(_))) => return Ok(()),
                Some(Ok(Message::Pong(_))) => last_pong = Instant::now(),
                Some(Ok(Message::Ping(_))) => {},
                Some(Ok(Message::Text(text))) => {
                    if window.elapsed() >= Duration::from_secs(1) { window = Instant::now(); commands = 0; }
                    commands += 1;
                    ensure!(commands <= 4, "native control command rate exceeded");
                    let request: Request = serde_json::from_str(&text).context("invalid native control command")?;
                    let reply = match owner.view.select(a, &request).await {
                        Ok(reply) => reply,
                        Err(error) => { let (bot, generation) = owner.view.selection();
                            json!({"type":"rejected","bot":bot,"generation":generation,"requested":request.generation,"error":error.to_string()})
                        }
                    };
                    send(socket, reply).await?;
                },
                Some(Ok(Message::Binary(_))) => bail!("native control requires JSON text"),
                Some(Err(error)) => return Err(error.into()),
            }
        }
    }
}
pub(in crate::server) async fn data(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(token): Path<String>,
    RawQuery(query): RawQuery,
    ws: WebSocketUpgrade,
) -> Response {
    if query.as_deref() != Some("cursor=2") {
        return rejected_response((
            StatusCode::BAD_REQUEST,
            "native capability socket requires cursor=2".into(),
        ));
    }
    let ip = match native_ip(&a, &headers, peer) {
        Ok(ip) => ip,
        Err(error) => return rejected_response(error),
    };
    if token.len() != 32
        || !token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return rejected_response((StatusCode::NOT_FOUND, "native view not found".into()));
    }
    let Some(view) = a.cctv.view(&token) else {
        return rejected_response((StatusCode::NOT_FOUND, "native view not found".into()));
    };
    let mut lease = match view.acquire(ip) {
        Ok(lease) => lease,
        Err(error) => return rejected_response((StatusCode::FORBIDDEN, error.to_string())),
    };
    ws.max_message_size(65_536)
        .max_frame_size(65_536)
        .on_upgrade(move |socket| async move {
            let mut stream = Stream::new(socket, a.shutdown.subscribe());
            let bot = lease.bot.clone();
            if let Err(error) = stream.watch(&a, &bot, 2, Some(&mut lease)).await {
                view.close_socket(Some(lease.id));
                tracing::warn!(bot, error=%error, "native capability stream ended with error");
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    stream.socket.send(Message::Close(Some(CloseFrame {
                        code: 1011,
                        reason: error
                            .to_string()
                            .chars()
                            .take(110)
                            .collect::<String>()
                            .into(),
                    }))),
                )
                .await;
            }
        })
}
