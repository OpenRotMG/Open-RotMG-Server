use super::*;

pub(super) struct EncodedOutput {
    pub bytes: Vec<u8>,
    pub packets: u64,
    pub reconnect: bool,
    pub terminal: bool,
}
pub(super) fn encode_ready(
    codec: &mut Codec,
    mut packet: Packet,
    queue: &mut wire::outbox::Receiver,
    player: &mut Option<i32>,
    testing: Option<&testing::Testing>,
) -> EncodedOutput {
    let mut out = EncodedOutput {
        bytes: Vec::new(),
        packets: 0,
        reconnect: false,
        terminal: false,
    };
    loop {
        if packet.id == wire::CREATE_SUCCESS {
            *player = Some(i32::from_be_bytes(
                packet.body[..4]
                    .try_into()
                    .expect("CREATE_SUCCESS player identity"),
            ));
        }
        let nonce = (packet.id == wire::PING && packet.body.len() == 4)
            .then(|| i32::from_be_bytes(packet.body[..4].try_into().unwrap()));
        out.reconnect = packet.id == 39;
        out.terminal = out.reconnect || packet.id == 0;
        let frame = codec.frame(packet.id, packet.body);
        if let (Some(testing), Some(player), Some(nonce)) = (testing, *player, nonce) {
            testing.note_native_frame(nonce, player, &frame);
        }
        out.bytes.extend(frame);
        out.packets += 1;
        // Socket chunks may combine native packets, never split retirement or change cipher order.
        if out.terminal || out.packets == 32 || out.bytes.len() >= 65536 {
            break;
        }
        let Ok(next) = queue.try_recv() else {
            break;
        };
        packet = next;
    }
    out
}
#[derive(Default, Clone)]
pub(super) struct Stats {
    pub packets: u64,
    pub high_water: usize,
    pub queued_at_stop: usize,
    pub last_send: Option<std::time::Instant>,
    pub max_send_ms: f64,
}
enum Control {
    Pong(axum::body::Bytes),
    Retire(Packet),
    Stop,
}
pub(super) struct Writer {
    control: mpsc::Sender<Control>,
    owner: tokio::sync::watch::Sender<Option<tokio::sync::watch::Receiver<bool>>>,
    pub reconnected: tokio::sync::watch::Receiver<bool>,
    stats: Arc<Mutex<Stats>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Writer {
    pub fn start<S>(
        mut sink: S,
        mut queue: wire::outbox::Receiver,
        lost: tokio::sync::watch::Sender<bool>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
        mut drain: tokio::sync::watch::Receiver<u64>,
        testing: Option<testing::Testing>,
    ) -> Self
    where
        S: futures_util::Sink<Message> + Unpin + Send + 'static,
    {
        let (control, mut controls) = mpsc::channel(64);
        let (owner, owner_rx) =
            tokio::sync::watch::channel::<Option<tokio::sync::watch::Receiver<bool>>>(None);
        let (reconnected_tx, reconnected) = tokio::sync::watch::channel(false);
        let stats = Arc::new(Mutex::new(Stats::default()));
        let shared = stats.clone();
        let mut lost_rx = lost.subscribe();
        let task = tokio::spawn(async move {
            let mut codec = Codec::server();
            let mut player = None;
            let mut retired = false;
            let mut stopping = false;
            let result: std::result::Result<(), CloseReason> = async {
                loop {
                    let packet = if stopping {
                        let Ok(packet) = queue.try_recv() else { break; };
                        packet
                    } else { tokio::select! {
                        biased;
                        command = controls.recv() => match command {
                            Some(Control::Pong(bytes)) => {
                                let cancel = owner_rx.borrow().clone();
                                send_frame(&mut sink, Message::Pong(bytes), &mut lost_rx, &mut shutdown, cancel, &mut drain).await?;
                                continue;
                            },
                            Some(Control::Retire(packet)) => { while queue.try_recv().is_ok() {} packet },
                            Some(Control::Stop) | None => {
                                stopping = true;
                                let Ok(packet) = queue.try_recv() else { break; };
                                packet
                            },
                        },
                        packet = queue.recv() => { let Some(packet) = packet else { break; }; packet },
                        _ = lost_rx.changed() => return Err(CloseReason::Output),
                        _ = shutdown.changed() => return Err(CloseReason::Shutdown),
                        _ = drain.changed() => return Err(CloseReason::Shutdown),
                    }};
                    if retired { continue; }
                    let high_water = queue.len()+1;
                    let batch = encode_ready(&mut codec, packet, &mut queue, &mut player, testing.as_ref());
                    let cancel = if batch.reconnect { None } else { owner_rx.borrow().clone() };
                    let sending = std::time::Instant::now();
                    send_frame(&mut sink, Message::Binary(batch.bytes.into()), &mut lost_rx, &mut shutdown, cancel, &mut drain).await?;
                    let mut s = shared.lock().expect("output diagnostics poisoned");
                    s.packets += batch.packets;
                    s.high_water = s.high_water.max(high_water);
                    s.last_send = Some(std::time::Instant::now());
                    s.max_send_ms = s.max_send_ms.max(sending.elapsed().as_secs_f64()*1000.0);
                    if batch.reconnect { reconnected_tx.send_replace(true); }
                    if batch.terminal { retired=true; }
                }
                Ok(())
            }.await;
            shared
                .lock()
                .expect("output diagnostics poisoned")
                .queued_at_stop = queue.len();
            if let Err(reason) = result {
                tracing::debug!(%reason,"native writer stopped");
                if !matches!(reason, CloseReason::Controller | CloseReason::Shutdown) {
                    lost.send_replace(true);
                }
            }
        });
        Self {
            control,
            owner,
            reconnected,
            stats,
            task,
        }
    }
    pub fn owner(&self, cancel: Option<tokio::sync::watch::Receiver<bool>>) {
        self.owner.send_replace(cancel);
    }
    pub fn pong(&self, bytes: axum::body::Bytes) -> std::result::Result<(), ()> {
        self.control.try_send(Control::Pong(bytes)).map_err(|_| ())
    }
    pub fn retire(&self, packet: Packet) -> std::result::Result<(), ()> {
        self.control
            .try_send(Control::Retire(packet))
            .map_err(|_| ())
    }
    pub fn stats(&self) -> Stats {
        self.stats
            .lock()
            .expect("output diagnostics poisoned")
            .clone()
    }
    pub async fn stop(&mut self) {
        let _ = self.control.send(Control::Stop).await;
        if tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
        }
    }
}
