//! Live gateway connections, so a payload a plugin queues outside a gateway frame (a REST event,
//! a timer) reaches Discord at once instead of waiting for the next frame.
//!
//! Each bridged connection ([`crate::gateway::bridge`]) registers a channel while it runs; the
//! [`LiveGuard`] it holds removes it again when the connection ends, however it ends.

use phoecord_core::Val;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Frames waiting per connection before more are dropped (plugins cannot flood a socket).
const QUEUE: usize = 256;

/// Client → server frames (`{"op":…,"d":…}`, already through the Send chain) for one connection.
pub type LiveRx = mpsc::Receiver<Val>;

#[derive(Default)]
struct Inner {
    next_id: u64,
    /// Oldest first; the last one is the newest connection.
    conns: Vec<(u64, mpsc::Sender<Val>)>,
}

/// The registry of live gateway connections.
#[derive(Clone, Default)]
pub struct LiveGateways {
    inner: Arc<Mutex<Inner>>,
}

/// Keeps a connection registered; dropping it unregisters the connection.
pub struct LiveGuard {
    inner: Arc<Mutex<Inner>>,
    id: u64,
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut inner = self.inner.lock().unwrap();
        inner.conns.retain(|(id, _)| *id != self.id);
    }
}

impl LiveGateways {
    /// Register a new connection; it becomes the newest one.
    pub fn register(&self) -> (LiveGuard, LiveRx) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let mut inner = self.inner.lock().unwrap();
        inner.next_id += 1;
        let id = inner.next_id;
        inner.conns.push((id, tx));
        let guard = LiveGuard {
            inner: self.inner.clone(),
            id,
        };
        (guard, rx)
    }

    /// How many connections are registered.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().conns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Hand `frames` to the newest live connection. Returns how many were dropped (no
    /// connection, or its queue is full); each drop is logged.
    pub fn push(&self, frames: Vec<Val>) -> usize {
        if frames.is_empty() {
            return 0;
        }
        let tx = self
            .inner
            .lock()
            .unwrap()
            .conns
            .last()
            .map(|(_, tx)| tx.clone());
        let Some(tx) = tx else {
            tracing::info!(
                "{} plugin gateway payload(s) dropped: no live gateway connection",
                frames.len()
            );
            return frames.len();
        };
        let mut dropped = 0;
        for f in frames {
            if tx.try_send(f).is_err() {
                dropped += 1;
            }
        }
        if dropped > 0 {
            tracing::info!(
                "{dropped} plugin gateway payload(s) dropped: the gateway connection is busy or closing"
            );
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(op: i64) -> Val {
        let mut v = Val::map();
        v.set("op", Val::Int(op));
        v
    }

    #[test]
    fn the_newest_connection_gets_the_frames_and_a_closed_one_is_removed() {
        let live = LiveGateways::default();
        assert_eq!(live.push(vec![frame(1)]), 1, "nobody connected: dropped");
        let (old_guard, mut old_rx) = live.register();
        let (new_guard, mut new_rx) = live.register();
        assert_eq!(live.len(), 2);
        assert_eq!(live.push(vec![frame(3), frame(4)]), 0);
        assert_eq!(new_rx.try_recv().unwrap(), frame(3));
        assert_eq!(new_rx.try_recv().unwrap(), frame(4));
        assert!(old_rx.try_recv().is_err());
        drop(new_guard);
        assert_eq!(live.len(), 1);
        live.push(vec![frame(5)]);
        assert_eq!(old_rx.try_recv().unwrap(), frame(5));
        drop(old_guard);
        assert!(live.is_empty());
    }
}
