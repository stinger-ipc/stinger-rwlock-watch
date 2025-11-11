# stinger-rwlock-watch

An implementation of an `RwLock` with integrated `tokio::watch` notifications. Whenever a write lock is released with a changed value, the current value is automatically sent to all subscribers via a `tokio::watch` channel.

## Features

- **Asynchronous RwLock**: Built on top of `tokio::sync::RwLock` for async/await compatibility
- **Automatic Notifications**: Whenever a write lock is released, all subscribers are notified via `tokio::watch`
- **Multiple Subscribers**: Support for multiple concurrent watchers
- **Zero-Cost Reads**: Reading the lock doesn't trigger any notifications
- **Clone-able**: The lock can be cloned to share across tasks
- **Read-Only Views**: Create read-only views that can only read and subscribe, but not write
- **Write-Request Views**: Create views that can request a change to the value via a channel and response.

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
stinger-rwlock-watch = "0.2.0"
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
        let mut writer = lock.write().await;
        *writer = 100;
    } // Notification sent when write lock is dropped here.

    // Read the value (no notification)
    {
        let reader = lock.read().await;
        println!("Current value: {}", *reader);
    } // reader guard is dropped here.
}
```

## Optional Features

There are two extensions available with features:

* `read_only` - Adds support for creating read-only views of the lock
* `write_request` - Adds support for view of the lock that can request changes via a channel and receive optional responses.

```bash
cargo test --features=read_only,write_request
```

### Read-Only Feature

The read-only feature allows you to create views of the lock that can only read values and subscribe to changes, but cannot acquire write locks.

```rust
use stinger_rwlock_watch::RwLockWatch;

#[tokio::main]
async fn main() {
    // Create top-level RwLockWatch with an initial value.
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
    let mut writer = lock.write().await;
    *writer = 100;
}
```

#### Running the Examples

```bash
cargo run --example read_only --features=read_only
```

### Write-Request Feature

The write-request feature allows you to create views of the lock that can send the updated value via a channel to a different task.  The task can then process the write requests as needed and return the updated value.

There are two modes:

* Manual commit: Call the `.commit()` on the write guard to send the updated value.   Await on the method call to block until the value is updated.
* Automatic commit: When the write guard is dropped, the updated value is sent automatically, but without any promises of success by the remote task.

```rust
use stinger_rwlock_watch::RwLockWatch;

#[tokio::main]
async fn main() {
    // Create top-level RwLockWatch with an initial value.
    let lock = RwLockWatch::new(42);

    // Implement a handler to process write requests.
    let mut recv = lock.take_request_receiver().unwrap();
    tokio::spawn(async move {
        while let Some((new_value, optional_callback)) = recv.recv().await {
            // Do something with the new value.
            do_something(new_value).await;

            // Send response if callback is provided
            if let Some(callback) = optional_callback {
                let _ = callback.send(Some(new_value)); // Send back the updated value (can be different if needed)
                // If we wend back None, then it indicates failure and the write-guard-value will revert.
            }
        }
    });
    
    // Create a read-only view
    let request_lock = lock.write_request();
    
    // Request a change to the value
    {
        let value = request_lock.write().await;
        *value = 100;
        value.commit().await;
    };

    // The value has not been updated
    {
        let reader = lock.read().await;
        println!("Updated value: {}", *reader); // prints 42
    }


}
```

## License

See [LICENSE](LICENSE) for details.

Rust RwLock with a built in watch
