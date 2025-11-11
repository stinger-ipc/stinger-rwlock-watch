use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::watch;
#[cfg(feature = "write_request")]
use tokio::sync::{mpsc, oneshot};
#[cfg(feature = "write_request")]
use std::sync::Mutex;

/// An RwLock that automatically broadcasts value changes via a tokio::watch channel
/// whenever a write lock is released.
pub struct RwLockWatch<T: Clone> {
    inner: Arc<TokioRwLock<T>>,
    tx: watch::Sender<T>,
    #[cfg(feature = "write_request")]
    request_tx: mpsc::Sender<(T, Option<oneshot::Sender<Option<T>>>)>,
    #[cfg(feature = "write_request")]
    // Stored in an Option so it can be "taken" exactly once by a consumer.
    // Wrapped in Arc<Mutex<..>> so all clones share the same single receiver holder.
    request_rx: Arc<Mutex<Option<mpsc::Receiver<(T, Option<oneshot::Sender<Option<T>>>)>>>>,
}

#[cfg(feature = "read_only")]
mod readonly;

#[cfg(feature = "read_only")]
pub use readonly::ReadOnlyLockWatch;

#[cfg(feature = "write_request")]
mod writerequest;

#[cfg(feature = "write_request")]
pub use writerequest::WriteRequestLockWatch;

impl<T: Clone> RwLockWatch<T> {
    /// Creates a new RwLockWatch with the given initial value
    pub fn new(value: T) -> Self {
        let (tx, _rx) = watch::channel(value.clone());
        #[cfg(feature = "write_request")]
        let (request_tx, request_rx) = mpsc::channel(32);
        Self {
            inner: Arc::new(TokioRwLock::new(value)),
            tx,
            #[cfg(feature = "write_request")]
            request_tx,
            #[cfg(feature = "write_request")]
            request_rx: Arc::new(Mutex::new(Some(request_rx))),
        }
    }

    /// Acquires a read lock on the RwLock
    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        self.inner.read().await
    }

    /// Acquires a write lock on the RwLock
    /// 
    /// When the returned WriteGuard is dropped, the current value will be
    /// automatically sent to all watch receivers.
    pub async fn write(&self) -> WriteGuard<'_, T> {
        let guard = self.inner.write().await;
        WriteGuard {
            guard,
            tx: &self.tx,
        }
    }

    /// Subscribe to receive notifications when the value changes
    /// 
    /// Returns a watch::Receiver that will be notified whenever a write lock is released.
    pub fn subscribe(&self) -> watch::Receiver<T> {
        self.tx.subscribe()
    }

    /// Takes ownership of the write request receiver (if the `write_request` feature is enabled).
    ///
    /// This can be called at most once. Subsequent calls will return `None`.
    /// A single consumer should own the returned `mpsc::Receiver<(T, Option<oneshot::Sender<Option<T>>>)>` and handle
    /// incoming requested value changes (e.g. to apply validation or mutation logic
    /// before committing them to the underlying value with a real write()).
    #[cfg(feature = "write_request")]
    pub fn take_request_receiver(&self) -> Option<mpsc::Receiver<(T, Option<oneshot::Sender<Option<T>>>)>> {
        self.request_rx.lock().expect("poisoned mutex").take()
    }

    #[cfg(feature = "read_only")]
    /// Creates a read-only view of this RwLockWatch.
    /// 
    /// The returned `ReadOnlyLockWatch` can only acquire read locks and subscribe
    /// to changes, but cannot acquire write locks. This is useful for passing to
    /// code that should only observe the value but not modify it.
    pub fn read_only(&self) -> crate::readonly::ReadOnlyLockWatch<T> {
        crate::readonly::ReadOnlyLockWatch {
            inner: Arc::clone(&self.inner),
            rx: self.tx.subscribe(),
        }
    }

    #[cfg(feature = "write_request")]
    pub fn write_request(&self) -> WriteRequestLockWatch<T> {
        WriteRequestLockWatch {
            inner: Arc::clone(&self.inner),
            rx: self.tx.subscribe(),
            tx: self.tx.clone(),
            request_tx: self.request_tx.clone(),
        }
    }
}

impl<T: Clone> Clone for RwLockWatch<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            tx: self.tx.clone(),
            #[cfg(feature = "write_request")]
            request_tx: self.request_tx.clone(),
            #[cfg(feature = "write_request")]
            request_rx: Arc::clone(&self.request_rx),
        }
    }
}

/// A write guard that broadcasts the value when dropped
pub struct WriteGuard<'a, T: Clone> {
    guard: RwLockWriteGuard<'a, T>,
    tx: &'a watch::Sender<T>,
}

impl<'a, T: Clone> Drop for WriteGuard<'a, T> {
    fn drop(&mut self) {
        // Send the current value to all watch subscribers when the write lock is released
        let _ = self.tx.send(self.guard.clone());
    }
}

impl<'a, T: Clone> Deref for WriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a, T: Clone> DerefMut for WriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_basic_read_write() {
        let lock = RwLockWatch::new(42);

        // Read the initial value
        {
            let read = lock.read().await;
            assert_eq!(*read, 42);
        }

        // Write a new value
        {
            let mut write = lock.write().await;
            *write = 100;
        }

        // Read the updated value
        {
            let read = lock.read().await;
            assert_eq!(*read, 100);
        }
    }

    #[tokio::test]
    async fn test_watch_notification() {
        let lock = RwLockWatch::new(0);
        let mut rx = lock.subscribe();

        // Initial value should be available
        assert_eq!(*rx.borrow(), 0);

        // Update the value
        {
            let mut write = lock.write().await;
            *write = 1;
        }

        // Wait for the notification
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), 1);

        // Update again
        {
            let mut write = lock.write().await;
            *write = 2;
        }

        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), 2);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let lock = RwLockWatch::new(String::from("initial"));
        let mut rx1 = lock.subscribe();
        let mut rx2 = lock.subscribe();

        // Update the value
        {
            let mut write = lock.write().await;
            *write = String::from("updated");
        }

        // Both subscribers should receive the update
        rx1.changed().await.unwrap();
        rx2.changed().await.unwrap();

        assert_eq!(*rx1.borrow(), "updated");
        assert_eq!(*rx2.borrow(), "updated");
    }

    #[tokio::test]
    async fn test_clone() {
        let lock1 = RwLockWatch::new(10);
        let lock2 = lock1.clone();

        {
            let mut write = lock1.write().await;
            *write = 20;
        }

        let read = lock2.read().await;
        assert_eq!(*read, 20);
    }

    #[tokio::test]
    async fn test_concurrent_reads() {
        let lock = Arc::new(RwLockWatch::new(42));
        
        let lock1 = Arc::clone(&lock);
        let lock2 = Arc::clone(&lock);

        let handle1 = tokio::spawn(async move {
            let read = lock1.read().await;
            sleep(Duration::from_millis(10)).await;
            *read
        });

        let handle2 = tokio::spawn(async move {
            let read = lock2.read().await;
            sleep(Duration::from_millis(10)).await;
            *read
        });

        let (val1, val2) = tokio::join!(handle1, handle2);
        assert_eq!(val1.unwrap(), 42);
        assert_eq!(val2.unwrap(), 42);
    }

}
