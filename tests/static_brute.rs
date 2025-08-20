const NUM_ITERATIONS: usize = 8000;

use some_executor::task::{Configuration, Task};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(target_arch = "wasm32")]
use wasm_thread as thread;

#[test_executors::async_test]
async fn test_trampoline() {
    let (c, f) = r#continue::continuation::<()>();
    //due to use of wasm-thread we must run in browser
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
    thread::spawn(|| {
        let t = Task::without_notifications(
            "submit_to_main_thread_benchmark".to_string(),
            Configuration::default(),
            async {
                eprintln!("Calling brute...");
                brute().await;
                c.send(());
            },
        );
        t.spawn_static_current();
    });
    f.await;
}

async fn brute() {
    let mut senders = Vec::new();
    let mut futures = Vec::new();
    for _ in 0..NUM_ITERATIONS {
        let (tx, rx) = r#continue::continuation();
        senders.push(tx);
        futures.push(rx);
    }

    thread::spawn(move || {
        eprintln!(
            "Background thread started, will submit {count} tasks",
            count = senders.len()
        );
        for (s, sender) in senders.drain(..).enumerate() {
            eprintln!("Submitting task {}/{}", s + 1, NUM_ITERATIONS);
            sender.send(s);
            eprintln!("Task {}/{} finished", s + 1, NUM_ITERATIONS);
        }
        eprintln!(
            "Background thread finished submitting all {count} tasks",
            count = NUM_ITERATIONS
        );
    });

    // Collect results
    eprintln!(
        "Starting to collect {count} results...",
        count = futures.len()
    );
    for recv in futures {
        recv.await;
    }
    eprintln!("Finished collecting all results!");
}
