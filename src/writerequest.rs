use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError, mpsc, watch};

/// A write-request view of an RwLockWatch.  Changes to the guarded value don't persist,
/// but instead creates a request to change the value via the original lock.
#[derive(Clone)]
pub struct WriteRequestLockWatch<T: Clone> {
    inner: Arc<TokioRwLock<T>>,
    rx: watch::Receiver<T>,
    request_tx: mpsc::Sender<T>,
}


impl<T: Clone> WriteRequestLockWatch<T> {
    /// Acquires a read lock on the RwLock
    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        self.inner.read().await
    }

    /// Tries to acquire a read lock on the RwLock
    pub async fn try_read(&self) -> Result<RwLockReadGuard<'_, T>, TryLockError> {
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
    guard: RwLockWriteGuard<'a, T>,
    value: T,
    tx: &'a mpsc::Sender<T>,
}


impl<'a, T: Clone> Drop for WriteRequestGuard<'a, T> {
    fn drop(&mut self) {
        // Send the current value to all watch subscribers when the write lock is released
        let _ = self.tx.send(self.value.clone());
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
