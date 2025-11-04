
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, TryLockError, watch};

/// A read-only view of an RwLockWatch that only allows read operations and subscriptions.
/// This type cannot acquire write locks.
#[derive(Clone)]
pub struct ReadOnlyLockWatch<T: Clone> {
    pub(crate) inner: Arc<TokioRwLock<T>>,
    pub(crate) rx: watch::Receiver<T>,
}

impl<T: Clone> ReadOnlyLockWatch<T> {
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

}

#[cfg(test)]
mod tests {
    use crate::RwLockWatch;

    #[tokio::test]
    async fn test_read_only_view() {
        let lock = RwLockWatch::new(100);
        let read_only = lock.read_only();

        // Can read from read-only view
        {
            let read = read_only.read().await;
            assert_eq!(*read, 100);
        }

        // Can subscribe from read-only view
        let mut rx = read_only.subscribe();
        assert_eq!(*rx.borrow(), 100);

        // Original lock can still write
        {
            let mut write = lock.write().await;
            *write = 200;
        }

        // Read-only view sees the change
        {
            let read = read_only.read().await;
            assert_eq!(*read, 200);
        }

        // Subscription from read-only view receives update
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), 200);
    }

    #[tokio::test]
    async fn test_read_only_clone() {
        let lock = RwLockWatch::new(42);
        let read_only1 = lock.read_only();
        let read_only2 = read_only1.clone();

        {
            let mut write = lock.write().await;
            *write = 99;
        }

        let read1 = read_only1.read().await;
        let read2 = read_only2.read().await;
        
        assert_eq!(*read1, 99);
        assert_eq!(*read2, 99);
    }

}
