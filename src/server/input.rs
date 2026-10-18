use crate::wire::{Codec, MAX_FRAME, Packet};
use axum::{body::Bytes, extract::ws::Message};
use futures_util::{Stream, StreamExt};
use std::{
    fmt::Display,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch},
    task::JoinHandle,
};

pub(super) enum Incoming {
    Command(Packet, Instant, OwnedSemaphorePermit),
    Ping(Bytes),
}
pub(super) struct InputTask {
    task: JoinHandle<()>,
    last_frame: Arc<Mutex<Option<Instant>>>,
}
impl InputTask {
    pub fn abort(&self) {
        self.task.abort();
    }
    pub fn last_frame_age_ms(&self) -> Option<f64> {
        self.last_frame
            .lock()
            .expect("input diagnostics poisoned")
            .map(|at| at.elapsed().as_secs_f64() * 1000.)
    }
}
impl Drop for InputTask {
    fn drop(&mut self) {
        self.abort();
    }
}
pub(super) fn receive<S, E>(
    mut stream: S,
    lost: watch::Sender<bool>,
    clock: crate::clock::Clock,
) -> (mpsc::Receiver<Incoming>, InputTask)
where
    S: Stream<Item = Result<Message, E>> + Send + Unpin + 'static,
    E: Display + Send + 'static,
{
    let (tx, rx) = mpsc::channel(64);
    let last_frame = Arc::new(Mutex::new(None));
    let received_frame = last_frame.clone();
    let task = tokio::spawn(async move {
        let mut codec = Codec::server();
        let bytes = Arc::new(Semaphore::new(MAX_FRAME));
        let mut window = Instant::now();
        let mut count = 0usize;
        let mut dispatched = 0;
        while let Some(message) = stream.next().await {
            *received_frame.lock().expect("input diagnostics poisoned") = Some(Instant::now());
            let received = clock.now();
            let inputs = match message {
                Ok(Message::Binary(frame)) => {
                    let packets = match codec.feed(&frame) {
                        Ok(packets) => packets,
                        Err(error) => {
                            tracing::error!(%error, "invalid packet stream");
                            lost.send_replace(true);
                            return;
                        }
                    };
                    if window.elapsed().as_secs() >= 1 {
                        window = Instant::now();
                        count = 0;
                    }
                    count = count.saturating_add(packets.len());
                    if count > 512 {
                        tracing::warn!("input packet rate exceeded");
                        lost.send_replace(true);
                        return;
                    }
                    let mut inputs = Vec::with_capacity(packets.len());
                    for packet in packets {
                        let permit = match bytes
                            .clone()
                            .try_acquire_many_owned((packet.body.len() + 5) as u32)
                        {
                            Ok(permit) => permit,
                            Err(error) => {
                                tracing::warn!(%error, "native input byte backlog exceeded");
                                lost.send_replace(true);
                                return;
                            }
                        };
                        inputs.push(Incoming::Command(packet, received, permit));
                    }
                    inputs
                }
                Ok(Message::Ping(bytes)) => vec![Incoming::Ping(bytes)],
                Ok(Message::Close(_)) => return,
                Ok(_) => {
                    tracing::warn!("non-binary game message rejected");
                    lost.send_replace(true);
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, "WebSocket failed");
                    lost.send_replace(true);
                    return;
                }
            };
            let batch = inputs.len();
            for (index, input) in inputs.into_iter().enumerate() {
                if let Err(error) = tx.try_send(input) {
                    tracing::warn!(%error, batch, index, "native input queue rejected before receipt timing diverges");
                    lost.send_replace(true);
                    return;
                }
                // Buffered separate frames must also let the consumer run before the bounded queue fills.
                dispatched = (dispatched + 1) % 16;
                if dispatched == 0 {
                    tokio::task::yield_now().await;
                }
            }
        }
    });
    (rx, InputTask { task, last_frame })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Writer;
    fn frames(count: usize) -> Message {
        let mut codec = Codec::client();
        let mut bytes = Vec::new();
        for time in 0..count {
            let mut body = Writer::new();
            body.i32(time as i32);
            bytes.extend(codec.frame(38, body.0));
        }
        Message::Binary(bytes.into())
    }
    #[tokio::test]
    async fn receipt_precedes_processing_and_preserves_frame_order() {
        let (lost, _) = watch::channel(false);
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(frames(2))]);
        let (mut rx, _task) = receive(stream, lost, crate::clock::Clock::default());
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let Incoming::Command(first, received, _first_bytes) = rx.recv().await.unwrap() else {
            panic!("expected command")
        };
        let Incoming::Command(second, next, _second_bytes) = rx.recv().await.unwrap() else {
            panic!("expected command")
        };
        assert_eq!(first.body, 0i32.to_be_bytes());
        assert_eq!(second.body, 1i32.to_be_bytes());
        assert_eq!(received, next);
        assert!(received.elapsed() >= std::time::Duration::from_millis(20));
        assert!(rx.recv().await.is_none());
    }
    #[tokio::test]
    async fn coalesced_frames_yield_to_an_active_consumer_without_expanding_the_queue() {
        let (lost, notice) = watch::channel(false);
        let (mut rx, _task) = receive(
            futures_util::stream::iter([Ok::<_, std::io::Error>(frames(130))]),
            lost,
            crate::clock::Clock::default(),
        );
        let mut receipts = Vec::new();
        while let Some(Incoming::Command(packet, received, _bytes)) = rx.recv().await {
            receipts.push(received);
            assert_eq!(packet.body, ((receipts.len() - 1) as i32).to_be_bytes());
        }
        assert_eq!(receipts.len(), 130);
        assert!(receipts.iter().all(|r| *r == receipts[0]));
        assert!(!*notice.borrow());
    }
    #[tokio::test]
    async fn buffered_separate_frames_yield_to_an_active_consumer() {
        let (lost, notice) = watch::channel(false);
        let mut codec = Codec::client();
        let frames = (0i32..130)
            .map(|time| {
                Ok::<_, std::io::Error>(Message::Binary(
                    codec.frame(38, time.to_be_bytes().to_vec()).into(),
                ))
            })
            .collect::<Vec<_>>();
        let (mut rx, _task) = receive(
            futures_util::stream::iter(frames),
            lost,
            crate::clock::Clock::default(),
        );
        let mut count = 0i32;
        while let Some(Incoming::Command(packet, _, _bytes)) = rx.recv().await {
            assert_eq!(packet.body, count.to_be_bytes());
            count += 1;
        }
        assert_eq!(count, 130);
        assert!(!*notice.borrow());
    }
    #[tokio::test]
    async fn paused_ingress_keeps_original_adjusted_receipt_before_actor_processing() {
        let clock = crate::clock::Clock::default();
        clock.set_paused(true);
        let frozen = clock.now();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let (lost, _) = watch::channel(false);
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(frames(1))]);
        let (mut rx, _task) = receive(stream, lost, clock);
        let Incoming::Command(_, received, _) = rx.recv().await.unwrap() else {
            panic!("expected command")
        };
        assert_eq!(received, frozen);
    }
    #[tokio::test]
    async fn queued_bytes_are_bounded_independently_of_packet_count() {
        let (lost, mut notice) = watch::channel(false);
        let mut codec = Codec::client();
        let frames = (0..2)
            .map(|_| {
                Ok::<_, std::io::Error>(Message::Binary(
                    codec.frame(38, vec![0; MAX_FRAME / 2 + 128]).into(),
                ))
            })
            .collect::<Vec<_>>();
        let (mut rx, _task) = receive(
            futures_util::stream::iter(frames),
            lost,
            crate::clock::Clock::default(),
        );
        notice.changed().await.unwrap();
        assert!(*notice.borrow());
        assert!(matches!(rx.recv().await, Some(Incoming::Command(_, _, _))));
        assert!(rx.recv().await.is_none());
    }
    #[tokio::test]
    async fn rate_and_backlog_limits_close_instead_of_dropping_or_restamping_input() {
        for count in [65, 513] {
            let (lost, mut notice) = watch::channel(false);
            let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(frames(count))]);
            let (mut rx, _task) = receive(stream, lost, crate::clock::Clock::default());
            notice.changed().await.unwrap();
            assert!(*notice.borrow());
            if count == 513 {
                assert!(rx.recv().await.is_none());
            }
        }
    }
}
