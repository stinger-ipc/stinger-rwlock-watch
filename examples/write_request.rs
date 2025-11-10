#[cfg(feature = "write_request")]
use stinger_rwlock_watch::RwLockWatch;
#[cfg(feature = "write_request")]
use tokio::time::{sleep, Duration};

#[cfg(not(feature = "write_request"))]
fn main() {
    eprintln!("Enable feature 'write_request' to run this example: cargo run --example write_request --features write_request");
}

#[cfg(feature = "write_request")]
#[tokio::main]
async fn main() {
    // Create a lock with write_request feature
    let lock = RwLockWatch::new(0);

    // Create a WriteRequestLockWatch
    let request_view = lock.write_request();

    // Spawn a task to handle incoming requests
    let mut rx = lock.take_request_receiver().expect("receiver available");
    let handler = tokio::spawn(async move {
        while let Some((value, responder)) = rx.recv().await {
            println!("Received write request: {}", value);
            let _ = responder.send(Some(value + 1));
        }
        println!("Request channel closed");
    });

    // Simulate requesting value changes
    for i in 1..=3 {
        let mut req = request_view.write().await;
        let req_value = i * 10;
        *req = req_value;
        req.send(std::time::Duration::from_secs(1)).await;
        println!("Requested value: {} which resulted in {}", req_value, *req);
        drop(req);
        sleep(Duration::from_millis(100)).await;
    }

    // Drop the lock to close the channel
    drop(lock);
    sleep(Duration::from_millis(100)).await;
    handler.await.unwrap();
}
