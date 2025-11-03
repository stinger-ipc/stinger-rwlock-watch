use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::{RwLock as TokioRwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::watch;

/// An RwLock that automatically broadcasts value changes via a tokio::watch channel
/// whenever a write lock is released.
pub struct RwLockWatch<T: Clone> {
    inner: Arc<TokioRwLock<T>>,
    tx: watch::Sender<T>,
}

#[cfg(feature = "read_only")]
/// A read-only view of an RwLockWatch that only allows read operations and subscriptions.
/// This type cannot acquire write locks.
#[derive(Clone)]
pub struct ReadOnlyRwLockWatch<T: Clone> {
    inner: Arc<TokioRwLock<T>>,
    tx: watch::Sender<T>,
}

impl<T: Clone> RwLockWatch<T> {
    /// Creates a new RwLockWatch with the given initial value
    pub fn new(value: T) -> Self {
        let (tx, _rx) = watch::channel(value.clone());
        Self {
            inner: Arc::new(TokioRwLock::new(value)),
            tx,
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

    /// Gets a reference to the watch receiver
    /// 
    /// This allows you to check if there are any active subscribers.
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    #[cfg(feature = "read_only")]
    /// Creates a read-only view of this RwLockWatch.
    /// 
    /// The returned `ReadOnlyRwLockWatch` can only acquire read locks and subscribe
    /// to changes, but cannot acquire write locks. This is useful for passing to
    /// code that should only observe the value but not modify it.
    pub fn read_only(&self) -> ReadOnlyRwLockWatch<T> {
        ReadOnlyRwLockWatch {
            inner: Arc::clone(&self.inner),
            tx: self.tx.clone(),
        }
    }
}

impl<T: Clone> Clone for RwLockWatch<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            tx: self.tx.clone(),
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

#[cfg(feature = "read_only")]
impl<T: Clone> ReadOnlyRwLockWatch<T> {
    /// Acquires a read lock on the RwLock
    pub async fn read(&self) -> RwLockReadGuard<'_, T> {
        self.inner.read().await
    }

    /// Subscribe to receive notifications when the value changes
    /// 
    /// Returns a watch::Receiver that will be notified whenever a write lock is released.
    pub fn subscribe(&self) -> watch::Receiver<T> {
        self.tx.subscribe()
    }

    /// Gets the number of active subscribers
    /// 
    /// This allows you to check if there are any active subscribers.
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
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

        assert_eq!(lock.receiver_count(), 2);

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

    #[cfg(feature = "read_only")]
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

    #[cfg(feature = "read_only")]
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

    #[cfg(feature = "read_only")]
    #[tokio::test]
    async fn test_read_only_receiver_count() {
        let lock = RwLockWatch::new(0);
        let read_only = lock.read_only();

        let _rx1 = read_only.subscribe();
        let _rx2 = read_only.subscribe();

        assert_eq!(read_only.receiver_count(), 2);
        assert_eq!(lock.receiver_count(), 2);
    }
}
