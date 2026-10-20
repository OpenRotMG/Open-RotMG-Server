use super::{MAX_FRAME, Packet};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

struct Queued {
    packet: Packet,
    _bytes: OwnedSemaphorePermit,
}
#[derive(Clone)]
pub(crate) struct Sender {
    queue: mpsc::Sender<Queued>,
    bytes: Arc<Semaphore>,
}
pub(crate) struct Receiver(mpsc::Receiver<Queued>);
pub(crate) fn channel(packets: usize) -> (Sender, Receiver) {
    let (queue, receiver) = mpsc::channel(packets);
    (
        Sender {
            queue,
            bytes: Arc::new(Semaphore::new(MAX_FRAME)),
        },
        Receiver(receiver),
    )
}
impl Sender {
    pub fn try_send(&self, packet: Packet) -> Result<(), mpsc::error::TrySendError<Packet>> {
        let bytes = match self.bytes.clone().try_acquire_many_owned(
            (packet.body.len() + 5)
                .try_into()
                .expect("output frame size"),
        ) {
            Ok(bytes) => bytes,
            Err(_) => return Err(mpsc::error::TrySendError::Full(packet)),
        };
        self.queue
            .try_send(Queued {
                packet,
                _bytes: bytes,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(queued) => {
                    mpsc::error::TrySendError::Full(queued.packet)
                }
                mpsc::error::TrySendError::Closed(queued) => {
                    mpsc::error::TrySendError::Closed(queued.packet)
                }
            })
    }
    #[cfg(test)]
    pub async fn send(&self, packet: Packet) -> Result<(), mpsc::error::SendError<Packet>> {
        let bytes = self
            .bytes
            .clone()
            .acquire_many_owned((packet.body.len() + 5).try_into().unwrap())
            .await
            .expect("outbox byte semaphore stays open");
        self.queue
            .send(Queued {
                packet,
                _bytes: bytes,
            })
            .await
            .map_err(|error| mpsc::error::SendError(error.0.packet))
    }
    pub fn is_closed(&self) -> bool {
        self.queue.is_closed()
    }
    pub fn same_channel(&self, other: &Self) -> bool {
        self.queue.same_channel(&other.queue)
    }
    pub fn available_bytes(&self) -> usize {
        self.bytes.available_permits()
    }
}
impl Receiver {
    pub async fn recv(&mut self) -> Option<Packet> {
        self.0.recv().await.map(|queued| queued.packet)
    }
    pub fn try_recv(&mut self) -> Result<Packet, mpsc::error::TryRecvError> {
        self.0.try_recv().map(|queued| queued.packet)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variable_packet_sizes_share_one_fixed_byte_budget_and_release_it() {
        let (tx, mut rx) = channel(4096);
        for _ in 0..200 {
            tx.try_send(Packet {
                id: 13,
                body: vec![0; 23],
            })
            .unwrap();
        }
        assert_eq!(tx.available_bytes(), MAX_FRAME - 200 * 28);
        assert!(
            tx.try_send(Packet {
                id: 26,
                body: vec![0; MAX_FRAME - 5]
            })
            .is_err()
        );
        while rx.try_recv().is_ok() {}
        assert_eq!(tx.available_bytes(), MAX_FRAME);
        tx.try_send(Packet {
            id: 26,
            body: vec![0; MAX_FRAME - 5],
        })
        .unwrap();
        assert!(
            tx.try_send(Packet {
                id: 11,
                body: vec![]
            })
            .is_err()
        );
        rx.try_recv().unwrap();
        assert_eq!(tx.available_bytes(), MAX_FRAME);
    }
    #[test]
    fn closed_or_packet_limited_queues_return_their_byte_reservation() {
        let (tx, rx) = channel(1);
        tx.try_send(Packet {
            id: 6,
            body: vec![0; 4],
        })
        .unwrap();
        assert!(
            tx.try_send(Packet {
                id: 6,
                body: vec![0; 4]
            })
            .is_err()
        );
        assert_eq!(tx.available_bytes(), MAX_FRAME - 9);
        drop(rx);
        assert!(tx.is_closed());
        assert!(matches!(
            tx.try_send(Packet {
                id: 11,
                body: vec![]
            }),
            Err(mpsc::error::TrySendError::Closed(_))
        ));
        assert_eq!(tx.available_bytes(), MAX_FRAME);
    }
}
