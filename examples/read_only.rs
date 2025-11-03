use stinger_rwlock_watch::RwLockWatch;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    println!("Read-Only RwLockWatch Example\n");

    // Create a new RwLockWatch
    let lock = RwLockWatch::new(0);

    // Get a read-only view - this can only read and subscribe, not write
    let read_only = lock.read_only();

    // Clone the read-only view to pass to another task
    let read_only_clone = read_only.clone();

    {
        let guard = read_only.read().await;
        // *guard = 123; # This would not compile since it is read-only
        println!("Initial value via read-only view: {}", *guard);
    }

    // Spawn a reader task with the read-only view
    let reader_handle = tokio::spawn(async move {
        println!("Reader: Using read-only view");
        for _ in 0..5 {
            sleep(Duration::from_millis(400)).await;
            let value = read_only_clone.read().await;
            println!("Reader: Current value is {}", *value);
        }
    });

    // Spawn a watcher task with the read-only view
    let mut watcher = read_only.subscribe();
    let watcher_handle = tokio::spawn(async move {
        println!("Watcher: Started watching via read-only view\n");
        // Watch for 5 changes, then exit
        for _ in 0..5 {
            if watcher.changed().await.is_ok() {
                let value = *watcher.borrow();
                println!("Watcher: Detected change! New value: {}", value);
            } else {
                break;
            }
        }
        println!("Watcher: Finished watching");
    });

    // Only the original lock can write
    println!("Writer: Starting updates (only original lock can write)\n");
    for i in 1..=5 {
        sleep(Duration::from_millis(500)).await;
        
        let mut write = lock.write().await;
        *write = i * 10;
        println!("Writer: Set value to {}", *write);
        drop(write);
    }

    // Wait for tasks to complete
    sleep(Duration::from_millis(500)).await;
    
    println!("\nExample completed!");
    println!("Note: The read-only view could read and subscribe, but not write!");
    
    // Wait for both tasks to finish
    let _ = tokio::join!(reader_handle, watcher_handle);
}
