#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_write_request_sends_value() {
        let lock = crate::RwLockWatch::new(0);
        let mut rx = lock.take_request_receiver().expect("receiver available");
        tokio::spawn(async move {
            while let Some((value, responder)) = rx.recv().await {
                assert_eq!(value, 42);
                let _ = responder.send(Some(43));
            }
        });

        let request_view = lock.write_request();
        {
            let mut req = request_view.write().await;
            *req = 42;
            req.send(std::time::Duration::from_secs(1)).await;
            assert_eq!(*req, 43);
        }
    }

    #[tokio::test]
    async fn test_multiple_requests() {
        let lock = crate::RwLockWatch::new(0);
        let mut rx = lock.take_request_receiver().expect("receiver available");
        tokio::spawn(async move {
            while let Some((value, responder)) = rx.recv().await {
                let _ = responder.send(Some(10+value));
            }
        });

        let request_view = lock.write_request();

        for i in 1..=3 {
            let mut req = request_view.write().await;
            *req = i * 10;
            req.send(std::time::Duration::from_secs(1)).await;
            assert_eq!(*req, (i * 10) + 10);
        }
    }
}
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError, mpsc, watch, oneshot};

/// A write-request view of an RwLockWatch.  Changes to the guarded value don't persist,
/// but instead creates a request to change the value via the original lock.
#[derive(Clone)]
pub struct WriteRequestLockWatch<T: Clone> {
    pub(crate) inner: Arc<TokioRwLock<T>>,
    pub(crate) rx: watch::Receiver<T>,
    pub(crate) request_tx: mpsc::Sender<(T, oneshot::Sender<Option<T>>)>,
}


impl<T: Clone> WriteRequestLockWatch<T> {
    /// Acquires a read lock on the RwLock
    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        self.inner.read().await
    }

    /// Tries to acquire a read lock on the RwLock
    pub fn try_read(&self) -> Result<RwLockReadGuard<'_, T>, TryLockError> {
        self.inner.try_read()
    }

    /// Subscribe to receive notifications when the value changes
    /// 
    /// Returns a watch::Receiver that will be notified whenever a write lock is released.
    pub fn subscribe(&self) -> watch::Receiver<T> {
        self.rx.clone()
    }

    /// Acquires a write lock on the RwLock
    /// 
    /// When the returned WriteGuard is dropped, the current value will be
    /// automatically sent to the request mpsc channel.
    pub async fn write(&self) -> WriteRequestGuard<'_, T> {
        let guard = self.inner.write().await;
        let value = (*guard).clone();
        WriteRequestGuard {
            guard, // keep the original guard to block others.
            value: value.clone(),
            original_value: value,
            tx: &self.request_tx,
        }
    }
}

/// A write guard that broadcasts the requested value change when dropped, but does not modify the original value.
pub struct WriteRequestGuard<'a, T: Clone> {
    #[allow(unused)]
    guard: RwLockWriteGuard<'a, T>,
    value: T,
    original_value: T,
    tx: &'a mpsc::Sender<(T, oneshot::Sender<Option<T>>)>,
}

impl<'a, T: Clone> Deref for WriteRequestGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'a, T: Clone> DerefMut for WriteRequestGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<'a, T: Clone> WriteRequestGuard<'a, T> {
    /// Sends the requested value change to the request channel.
    /// The value is set to the response from the channel, or reverts to the original value on timeout or error.
    pub async fn send(&mut self, timeout: std::time::Duration) {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send((self.value.clone(), tx)).await;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Some(value))) => {
                self.value = value;
            }
            _ => {
                self.value = self.original_value.clone();
            }
        }
    }
}
