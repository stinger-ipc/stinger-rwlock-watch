
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError, mpsc, watch, oneshot};

/// A write-request view of an RwLockWatch.  Changes to the guarded value don't persist,
/// but instead creates a request to change the value via the original lock.
#[derive(Clone)]
pub struct WriteRequestLockWatch<T: Clone> {
    pub(crate) inner: Arc<TokioRwLock<T>>,
    pub(crate) tx: watch::Sender<T>,
    pub(crate) rx: watch::Receiver<T>,
    pub(crate) request_tx: mpsc::Sender<(T, Option<oneshot::Sender<Option<T>>>)>,
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
            tx: self.tx.clone(),
            request_tx: self.request_tx.clone(),
        }
    }
}

/// A lock that sends requested value changes via mpsc channel when `commit` is called or when dropped.
pub struct WriteRequestGuard<'a, T: Clone> {
    #[allow(unused)]
    guard: RwLockWriteGuard<'a, T>,
    value: T,
    original_value: T,
    tx: watch::Sender<T>,
    request_tx: mpsc::Sender<(T, Option<oneshot::Sender<Option<T>>>)>,
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

impl<'a, T: Clone> Drop for WriteRequestGuard<'a, T> {
    fn drop(&mut self) {
        *(self.guard) = self.value.clone();
        let _ = self.request_tx.try_send((self.value.clone(), None));
        let _ = self.tx.send(self.value.clone());
    }
}

impl<'a, T: Clone> WriteRequestGuard<'a, T> {
    /// Sends the requested value change to the request channel.
    /// The value is set to the response from the channel, or reverts to the original value on timeout or error.
    pub async fn commit(&mut self, timeout: std::time::Duration) {
        let (resp_tx, resp_rx) = oneshot::channel();
        let _ = self.request_tx.send((self.value.clone(), Some(resp_tx))).await;
        match tokio::time::timeout(timeout, resp_rx).await {
            Ok(Ok(Some(value))) => {
                self.value = value.clone();
                self.original_value = value;
            }
            _ => {
                self.value = self.original_value.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_write_request_commits_value() {
        let lock = crate::RwLockWatch::new(0);
        let mut rx = lock.take_request_receiver().expect("receiver available");
        tokio::spawn(async move {
            while let Some((value, opt_responder)) = rx.recv().await {
                assert_eq!(value, 42);
                if let Some(responder) = opt_responder {
                    let _ = responder.send(Some(43));
                }
            }
        });

        let request_view = lock.write_request();
        {
            let mut req = request_view.write().await;
            *req = 42;
            req.commit(std::time::Duration::from_secs(5)).await;
            assert_eq!(*req, 43);
        }

        let reader = lock.read().await;
        assert_eq!(*reader, 43);
    }

    #[tokio::test]
    async fn test_write_request_value() {
        let lock = crate::RwLockWatch::new(0);

        let mut rx = lock.take_request_receiver().expect("receiver available");
        tokio::spawn(async move {
            while let Some((value, opt_responder)) = rx.recv().await {
                assert_eq!(value, 45);
            }
        });

        let request_view = lock.write_request();
        {
            let mut req = request_view.write().await;
            *req = 45;
            assert_eq!(*req, 45);
        }

        let reader = lock.read().await;
        assert_eq!(*reader, 45);
    }

    #[tokio::test]
    async fn test_multiple_requests() {
        let lock = crate::RwLockWatch::new(0);
        let mut rx = lock.take_request_receiver().expect("receiver available");
        tokio::spawn(async move {
            while let Some((value, opt_responder)) = rx.recv().await {
                if let Some(responder) = opt_responder {
                    let _ = responder.send(Some(11 + value));
                }
            }
        });

        let request_view = lock.write_request();

        for i in 1..=3 {
            let mut req = request_view.write().await;
            *req = i * 10;
            req.commit(std::time::Duration::from_secs(5)).await;
            assert_eq!(*req, (i * 10) + 11);
        }
    }
}