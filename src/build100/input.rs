use crate::wire::MAX_FRAME;
use anyhow::{Context, Result, ensure};
use axum::extract::ws::Message;
use futures_util::{Stream, StreamExt};
use std::{fmt::Display, sync::Arc, time::Instant};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinHandle,
};

pub(super) const MESSAGE_CAPACITY: usize = 64;
pub(super) const RAW_BYTE_CAPACITY: usize = MAX_FRAME * 2;

pub(super) struct Received {
    pub message: Message,
    pub received: Instant,
    pub sequence: u64,
    // Retain the budget through dispatch, not just until removal from the channel.
    _bytes: OwnedSemaphorePermit,
}
pub(super) struct Input {
    receiver: mpsc::Receiver<Received>,
    task: JoinHandle<Result<()>>,
    finished: bool,
}
impl Drop for Input {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub(super) fn receive<S, E>(stream: S) -> Input
where
    S: Stream<Item = std::result::Result<Message, E>> + Send + Unpin + 'static,
    E: Display + Send + 'static,
{
    receive_with_clock(stream, Instant::now)
}
pub(super) fn receive_with_clock<S, E, C>(mut stream: S, clock: C) -> Input
where
    S: Stream<Item = std::result::Result<Message, E>> + Send + Unpin + 'static,
    E: Display + Send + 'static,
    C: Fn() -> Instant + Send + 'static,
{
    let (sender, receiver) = mpsc::channel(MESSAGE_CAPACITY);
    let task = tokio::spawn(async move {
        let bytes = Arc::new(Semaphore::new(RAW_BYTE_CAPACITY));
        let mut sequence = 0_u64;
        while let Some(message) = stream.next().await {
            let received = clock();
            sequence = sequence.wrapping_add(1);
            let message =
                message.map_err(|e| anyhow::anyhow!("build100 WebSocket input failed: {e}"))?;
            let count = match &message {
                Message::Binary(bytes) | Message::Ping(bytes) => bytes.len(),
                Message::Close(_) => 0,
                _ => anyhow::bail!("expected binary build100 stream"),
            };
            ensure!(
                count <= RAW_BYTE_CAPACITY,
                "build100 receive message byte limit exceeded: incoming={count} capacity={RAW_BYTE_CAPACITY}"
            );
            let permit = bytes
                .clone()
                .try_acquire_many_owned(count as u32)
                .with_context(|| format!("build100 receive raw byte backlog exceeded: incoming={count} capacity={RAW_BYTE_CAPACITY}"))?;
            let close = matches!(message, Message::Close(_));
            tracing::debug!(
                frame_seq = sequence,
                byte_count = count,
                "build100 frame collected"
            );
            // Waiting here would recreate delayed collection and falsely compressed receipt gaps.
            sender
                .try_send(Received {
                    message,
                    received,
                    sequence,
                    _bytes: permit,
                })
                .map_err(|e| anyhow::anyhow!("build100 receive message backlog rejected: capacity={MESSAGE_CAPACITY}: {e}"))?;
            if close {
                break;
            }
        }
        Ok(())
    });
    Input {
        receiver,
        task,
        finished: false,
    }
}
impl Input {
    pub async fn recv(&mut self) -> Result<Option<Received>> {
        if !self.finished {
            let outcome = tokio::select! {
                biased;
                outcome=&mut self.task=>outcome,
                frame=self.receiver.recv()=>{
                    if let Some(frame)=frame { return Ok(Some(frame)); }
                    // Channel closure can wake dispatch before the task's error reaches its join handle.
                    (&mut self.task).await
                }
            };
            self.finished = true;
            if let Err(error) = outcome
                .context("build100 receive task failed")
                .and_then(|r| r)
            {
                self.receiver.close();
                while self.receiver.try_recv().is_ok() {}
                return Err(error);
            }
        }
        Ok(self.receiver.recv().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io, task::Poll, time::Duration};
    use tokio::sync::oneshot;

    fn binary(size: usize) -> std::result::Result<Message, io::Error> {
        Ok(Message::Binary(vec![0; size].into()))
    }
    async fn finished(input: &Input) {
        while !input.task.is_finished() {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn input_count_and_raw_byte_saturation_fail_before_queued_dispatch() {
        for messages in [
            (0..=MESSAGE_CAPACITY)
                .map(|_| binary(0))
                .collect::<Vec<_>>(),
            vec![binary(RAW_BYTE_CAPACITY), binary(1)],
            vec![binary(RAW_BYTE_CAPACITY + 1)],
        ] {
            let mut input = receive(futures_util::stream::iter(messages));
            finished(&input).await;
            let error = input.recv().await.err().expect("explicit saturation error");
            assert!(error.to_string().contains("build100 receive"), "{error:#}");
            assert!(input.recv().await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn input_capacity_boundaries_and_eof_preserve_order() {
        let mut input =
            receive(futures_util::stream::iter((0..MESSAGE_CAPACITY).map(|i| {
                Ok::<_, io::Error>(Message::Binary(vec![i as u8].into()))
            })));
        finished(&input).await;
        for i in 0..MESSAGE_CAPACITY {
            let frame = input.recv().await.unwrap().unwrap();
            assert_eq!(frame.sequence, i as u64 + 1);
            assert!(matches!(frame.message, Message::Binary(bytes) if bytes.as_ref()==[i as u8]));
        }
        assert!(input.recv().await.unwrap().is_none());
        let mut input = receive(futures_util::stream::iter([binary(RAW_BYTE_CAPACITY)]));
        finished(&input).await;
        assert!(input.recv().await.unwrap().is_some());
        assert!(input.recv().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn input_byte_permit_covers_current_dispatch_and_releases_on_drop() {
        for release in [false, true] {
            let (sender, mut rx) = mpsc::channel(2);
            let mut input = receive(futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx)));
            sender.send(binary(RAW_BYTE_CAPACITY)).await.unwrap();
            let held = input.recv().await.unwrap().unwrap();
            let held = if release {
                drop(held);
                None
            } else {
                Some(held)
            };
            sender
                .send(Ok(Message::Ping(vec![1].into())))
                .await
                .unwrap();
            drop(sender);
            finished(&input).await;
            if release {
                assert!(matches!(
                    input.recv().await.unwrap().unwrap().message,
                    Message::Ping(_)
                ));
            } else {
                assert!(
                    input
                        .recv()
                        .await
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("raw byte backlog")
                );
            }
            assert!(input.recv().await.unwrap().is_none());
            drop(held);
        }
    }

    #[tokio::test]
    async fn input_stream_failure_invalid_message_and_task_panic_surface_before_backlog() {
        for failure in [
            Err(io::Error::other("test read failure")),
            Ok(Message::Text("wrong".into())),
        ] {
            let mut input = receive(futures_util::stream::iter([binary(1), failure]));
            finished(&input).await;
            assert!(input.recv().await.is_err());
            assert!(input.recv().await.unwrap().is_none());
        }
        let stream = futures_util::stream::poll_fn(
            |_| -> Poll<Option<std::result::Result<Message, io::Error>>> {
                panic!("test collector panic");
            },
        );
        let mut input = receive(stream);
        finished(&input).await;
        assert!(
            input
                .recv()
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("receive task failed")
        );
        assert!(input.recv().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn input_close_and_empty_eof_finish_without_reordering() {
        let mut input = receive(futures_util::stream::iter([
            binary(1),
            Ok(Message::Close(None)),
            binary(2),
        ]));
        finished(&input).await;
        assert!(matches!(
            input.recv().await.unwrap().unwrap().message,
            Message::Binary(_)
        ));
        assert!(matches!(
            input.recv().await.unwrap().unwrap().message,
            Message::Close(_)
        ));
        assert!(input.recv().await.unwrap().is_none());
        let mut input = receive(futures_util::stream::empty::<
            std::result::Result<Message, io::Error>,
        >());
        assert!(input.recv().await.unwrap().is_none());
    }

    struct Dropped(Option<oneshot::Sender<()>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            let _ = self.0.take().unwrap().send(());
        }
    }
    #[tokio::test]
    async fn input_dispatch_timeout_is_cancel_safe_and_connection_exit_aborts_reader() {
        let (sender, mut rx) = mpsc::channel(1);
        let (dropped, notice) = oneshot::channel();
        let probe = Dropped(Some(dropped));
        let stream = futures_util::stream::poll_fn(move |cx| {
            let _keep = &probe;
            rx.poll_recv(cx)
        });
        let mut input = receive(stream);
        assert!(
            tokio::time::timeout(Duration::ZERO, input.recv())
                .await
                .is_err()
        );
        sender.send(binary(1)).await.unwrap();
        assert_eq!(input.recv().await.unwrap().unwrap().sequence, 1);
        drop(input);
        notice.await.unwrap();
        assert!(sender.send(binary(1)).await.is_err());
    }

    #[tokio::test]
    async fn input_split_and_coalesced_packets_use_completing_message_receipt_and_cipher_order() {
        use std::sync::atomic::{AtomicU64, Ordering};
        let base = Instant::now();
        let tick = Arc::new(AtomicU64::new(0));
        let mut client = crate::wire::Codec::server_with_keys(b"hPvJ(R}Mp$-cg", b"zCV2tsY0]s;|]");
        let mut encoded = client.frame(25, vec![1, 2, 3, 4]);
        encoded.extend(client.frame(8, vec![5, 6, 7]));
        let tail = encoded.split_off(7);
        let (sender, mut rx) = mpsc::channel(2);
        let mut input =
            receive_with_clock(futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx)), {
                let tick = tick.clone();
                move || base + Duration::from_millis(tick.load(Ordering::SeqCst))
            });
        let mut decoder = crate::protocol::server_codec();
        sender
            .send(Ok::<_, io::Error>(Message::Binary(encoded.into())))
            .await
            .unwrap();
        let frame = input.recv().await.unwrap().unwrap();
        let Message::Binary(bytes) = frame.message else {
            panic!("binary");
        };
        assert!(decoder.feed(&bytes).unwrap().is_empty());
        tick.store(360, Ordering::SeqCst);
        sender.send(Ok(Message::Binary(tail.into()))).await.unwrap();
        let frame = input.recv().await.unwrap().unwrap();
        assert_eq!(frame.received, base + Duration::from_millis(360));
        let Message::Binary(bytes) = frame.message else {
            panic!("binary");
        };
        let packets = decoder.feed(&bytes).unwrap();
        assert_eq!(packets.iter().map(|p| p.id).collect::<Vec<_>>(), [25, 8]);
        assert_eq!(packets[0].body, [1, 2, 3, 4]);
        assert_eq!(packets[1].body, [5, 6, 7]);
    }
}
