use stinger_rwlock_watch::RwLockWatch;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    println!("RwLockWatch Example\n");

    // Create a new RwLockWatch with an initial value
    let lock = RwLockWatch::new(0);

    // Subscribe to watch for changes
    let mut watcher = lock.subscribe();

    // Spawn a task to monitor changes
    let monitor_handle = tokio::spawn(async move {
        println!("Monitor: Started watching for changes...");
        // Watch for 5 changes, then exit
        for _ in 0..5 {
            if watcher.changed().await.is_ok() {
                let value = *watcher.borrow();
                println!("Monitor: Detected change! New value: {}", value);
            } else {
                break;
            }
        }
        println!("Monitor: Finished watching");
    });

    // Spawn a task to perform writes
    let writer_lock = lock.clone();
    let writer_handle = tokio::spawn(async move {
        for i in 1..=5 {
            sleep(Duration::from_millis(500)).await;
            
            let mut write = writer_lock.write().await;
            *write = i * 10;
            println!("Writer: Set value to {}", *write);
            drop(write); // Explicitly drop to trigger the watch notification
            
            sleep(Duration::from_millis(100)).await;
        }
    });

    // Spawn a task to perform reads
    let reader_lock = lock.clone();
    let reader_handle = tokio::spawn(async move {
        for _ in 0..10 {
            sleep(Duration::from_millis(300)).await;
            let read = reader_lock.read().await;
            println!("Reader: Current value is {}", *read);
        }
    });

    // Wait for the writer to complete
    writer_handle.await.unwrap();
    
    // Wait a bit for the reader to finish
    sleep(Duration::from_millis(500)).await;
    
    println!("\nExample completed!");
    
    // Wait for tasks to finish
    let _ = tokio::join!(monitor_handle, reader_handle);
}
