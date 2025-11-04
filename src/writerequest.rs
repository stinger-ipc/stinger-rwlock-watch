#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_write_request_sends_value() {
        let lock = crate::RwLockWatch::new(0);
        let mut rx = lock.take_request_receiver().expect("receiver available");
        let request_view = lock.write_request();

        {
            let mut req = request_view.write().await;
            *req = 42;
        }

        let received = rx.recv().await.expect("should receive value");
        assert_eq!(received, 42);
    }

    #[tokio::test]
    async fn test_multiple_requests() {
        let lock = crate::RwLockWatch::new(0);
        let mut rx = lock.take_request_receiver().expect("receiver available");
        let request_view = lock.write_request();

        for i in 1..=3 {
            let mut req = request_view.write().await;
            *req = i * 10;
        }

        let mut results = vec![];
        for _ in 0..3 {
            results.push(rx.recv().await.unwrap());
        }
        assert_eq!(results, vec![10, 20, 30]);
    }

    #[tokio::test]
    async fn test_receiver_only_once() {
        let lock = crate::RwLockWatch::new(0);
        let _rx = lock.take_request_receiver().expect("receiver available");
        assert!(lock.take_request_receiver().is_none(), "second take should return None");
    }

    #[tokio::test]
    async fn test_drop_closes_channel() {
        let lock = crate::RwLockWatch::new(0);
        let mut rx = lock.take_request_receiver().expect("receiver available");
        drop(lock);
        let result = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        assert!(result.is_err() || result.unwrap().is_none(), "channel should be closed");
    }
}
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError, mpsc, watch};

/// A write-request view of an RwLockWatch.  Changes to the guarded value don't persist,
/// but instead creates a request to change the value via the original lock.
#[derive(Clone)]
pub struct WriteRequestLockWatch<T: Clone> {
    pub(crate) inner: Arc<TokioRwLock<T>>,
    pub(crate) rx: watch::Receiver<T>,
    pub(crate) request_tx: mpsc::Sender<T>,
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
            value,
            tx: &self.request_tx,
        }
    }
}

/// A write guard that broadcasts the requested value change when dropped, but does not modify the original value.
pub struct WriteRequestGuard<'a, T: Clone> {
    #[allow(unused)]
    guard: RwLockWriteGuard<'a, T>,
    value: T,
    tx: &'a mpsc::Sender<T>,
}


impl<'a, T: Clone> Drop for WriteRequestGuard<'a, T> {
    fn drop(&mut self) {
        // Try to send the requested value without waiting (we're in Drop, cannot .await)
        let _ = self.tx.try_send(self.value.clone());
    }
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
