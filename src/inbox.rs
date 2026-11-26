//! The queue between the connection and the shard, bounded by count and by
//! bytes: 256 events can be a few kilobytes or, with large frames, gigabytes.
//! When it is full the reader stops reading until the shard catches up, so
//! nothing queued here is ever dropped.

use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

use crate::codec::MAX_FRAME_BYTES;
use crate::event::Event;

/// Frame bytes that may wait in the inbox. Larger than one maximum frame, so
/// any event can be delivered.
pub(crate) const INBOX_BYTES: usize = 2 * MAX_FRAME_BYTES;

/// A queued event, holding its share of the budget until it is taken.
type Queued = (Event, OwnedSemaphorePermit);

#[derive(Debug)]
pub(crate) struct Sender {
    tx: mpsc::Sender<Queued>,
    budget: Arc<Semaphore>,
    bytes: usize,
}

#[derive(Debug)]
pub(crate) struct Receiver {
    rx: mpsc::Receiver<Queued>,
}

/// A queue of at most `events` events adding up to at most `bytes` bytes.
pub(crate) fn channel(events: usize, bytes: usize) -> (Sender, Receiver) {
    let (tx, rx) = mpsc::channel(events.max(1));
    (Sender { tx, budget: Arc::new(Semaphore::new(bytes)), bytes }, Receiver { rx })
}

impl Sender {
    /// Queues `event`, which took `size` bytes on the wire, waiting for room.
    /// Returns `false` when the receiver is gone.
    pub(crate) async fn send(&self, event: Event, size: usize) -> bool {
        // An event bigger than the whole budget takes all of it.
        let share = u32::try_from(size.min(self.bytes)).unwrap_or(u32::MAX);
        let permit = tokio::select! {
            permit = Arc::clone(&self.budget).acquire_many_owned(share) => permit,
            () = self.tx.closed() => return false,
        };
        match permit {
            Ok(permit) => self.tx.send((event, permit)).await.is_ok(),
            Err(_) => false, // the semaphore is never closed
        }
    }
}

impl Receiver {
    /// The next event, releasing its share of the budget.
    pub(crate) async fn recv(&mut self) -> Option<Event> {
        self.rx.recv().await.map(|(event, _permit)| event)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;
    use crate::topic::Topic;

    fn event(n: u64) -> Event {
        Event::new(Topic::new("test:event").unwrap(), serde_json::json!(n))
    }

    /// Whether the send finishes without the consumer doing anything.
    async fn fits(tx: &Sender, n: u64, size: usize) -> bool {
        timeout(Duration::from_millis(50), tx.send(event(n), size)).await.is_ok()
    }

    #[tokio::test(start_paused = true)]
    async fn pending_bytes_never_exceed_the_budget() {
        let (tx, mut rx) = channel(100, 100);
        assert!(fits(&tx, 1, 60).await);
        assert!(fits(&tx, 2, 40).await);
        assert!(!fits(&tx, 3, 1).await, "the budget is spent");
        assert_eq!(rx.recv().await.unwrap().payload, 1);
        assert!(!fits(&tx, 3, 61).await, "only 60 bytes were freed");
        assert!(fits(&tx, 3, 60).await);
        assert_eq!(rx.recv().await.unwrap().payload, 2);
        assert_eq!(rx.recv().await.unwrap().payload, 3);
        // An event over the whole budget still arrives, alone.
        assert!(fits(&tx, 4, 1_000).await);
        assert!(!fits(&tx, 5, 1).await);
    }

    #[tokio::test(start_paused = true)]
    async fn the_count_limit_still_applies() {
        let (tx, mut rx) = channel(2, 1_000);
        assert!(fits(&tx, 1, 1).await && fits(&tx, 2, 1).await);
        assert!(!fits(&tx, 3, 1).await);
        rx.recv().await.unwrap();
        assert!(fits(&tx, 3, 1).await);
    }

    #[tokio::test]
    async fn a_closed_inbox_releases_a_waiting_sender() {
        let (tx, rx) = channel(10, 10);
        assert!(tx.send(event(1), 10).await);
        let waiting = tokio::spawn(async move { tx.send(event(2), 10).await });
        tokio::task::yield_now().await;
        drop(rx);
        assert!(!timeout(Duration::from_secs(5), waiting).await.unwrap().unwrap());
    }
}
