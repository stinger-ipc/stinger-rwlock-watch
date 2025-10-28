# stinger-rwlock-watch

A Rust implementation of an `RwLock` with integrated `tokio::watch` notifications. Whenever a write lock is released, the current value is automatically sent to all subscribers via a `tokio::watch` channel.

## Features

- **Asynchronous RwLock**: Built on top of `tokio::sync::RwLock` for async/await compatibility
- **Automatic Notifications**: Whenever a write lock is released, all subscribers are notified via `tokio::watch`
- **Multiple Subscribers**: Support for multiple concurrent watchers
- **Zero-Cost Reads**: Reading the lock doesn't trigger any notifications
- **Clone-able**: The lock can be cloned to share across tasks
- **Read-Only Views**: Create read-only views that can only read and subscribe, but not write

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
stinger-rwlock-watch = "0.1.0"
tokio = { version = "1.41", features = ["sync", "rt", "macros"] }
```

### Basic Example

```rust
use stinger_rwlock_watch::RwLockWatch;

#[tokio::main]
async fn main() {
    // Create a new RwLockWatch with an initial value
    let lock = RwLockWatch::new(42);

    // Subscribe to watch for changes
    let mut watcher = lock.subscribe();

    // Spawn a task to monitor changes
    tokio::spawn(async move {
        while watcher.changed().await.is_ok() {
            println!("Value changed to: {}", *watcher.borrow());
        }
    });

    // Write a new value (this will trigger the notification)
    {
        let mut write = lock.write().await;
        *write = 100;
    } // Notification sent when write lock is dropped

    // Read the value (no notification)
    let read = lock.read().await;
    println!("Current value: {}", *read);
}
```

### Read-Only View Example

You can create a read-only view that can only read values and subscribe to changes, but cannot acquire write locks:

```rust
use stinger_rwlock_watch::RwLockWatch;

#[tokio::main]
async fn main() {
    let lock = RwLockWatch::new(42);
    
    // Create a read-only view
    let read_only = lock.read_only();
    
    // Pass the read-only view to code that should only observe
    tokio::spawn(async move {
        let value = read_only.read().await;
        println!("Read-only view sees: {}", *value);
        
        // read_only.write() would not compile!
    });
    
    // Only the original lock can write
    let mut write = lock.write().await;
    *write = 100;
}
```

### Running the Examples

```bash
# Basic example
cargo run --example basic

# Read-only view example
cargo run --example read_only
```

### Running Tests

```bash
cargo test
```

## How It Works

The `RwLockWatch<T>` wraps a `tokio::sync::RwLock<T>` and maintains a `tokio::sync::watch::Sender<T>`. When you acquire a write lock via `.write().await`, you receive a `WriteGuard` that implements `Deref` and `DerefMut` to allow mutation of the inner value.

When the `WriteGuard` is dropped (i.e., when the write lock is released), the `Drop` implementation automatically sends the current value to the watch channel, notifying all subscribers.

## API

### `RwLockWatch<T: Clone>`

- `new(value: T) -> Self` - Creates a new RwLockWatch with an initial value
- `read() -> RwLockReadGuard<'_, T>` - Acquires a read lock
- `write() -> WriteGuard<'_, T>` - Acquires a write lock that broadcasts on drop
- `subscribe() -> watch::Receiver<T>` - Subscribe to receive change notifications
- `receiver_count() -> usize` - Get the number of active subscribers
- `read_only() -> ReadOnlyRwLockWatch<T>` - Creates a read-only view

### `ReadOnlyRwLockWatch<T: Clone>`

- `read() -> RwLockReadGuard<'_, T>` - Acquires a read lock
- `subscribe() -> watch::Receiver<T>` - Subscribe to receive change notifications
- `receiver_count() -> usize` - Get the number of active subscribers
- `clone() -> Self` - Clone the read-only view

## License

See [LICENSE](LICENSE) for details.

Rust RwLock with a built in watch
