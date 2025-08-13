// SPDX-License-Identifier: MIT OR Apache-2.0
/*!
Task-local storage for async tasks.

This module provides task-local storage similar to thread-local storage,
but scoped to async tasks instead of threads. Task-locals allow you to
store data that is unique to each async task and can be accessed from
anywhere within that task's execution.

# Overview

The module provides two types of task-local storage:

- [`LocalKey`]: A mutable task-local that can be read and modified
- [`LocalKeyImmutable`]: An immutable task-local that can only be set via scoping

Task-locals are declared using the [`task_local!`](crate::task_local!) macro:

```
use some_executor::task_local;

task_local! {
    // Mutable task-local
    static COUNTER: u32;

    // Immutable task-local
    static const CONFIG: String;
}
```

# Task-Local vs Thread-Local

Unlike thread-local storage, task-local values:
- Are scoped to async tasks, not OS threads
- Are not inherited by spawned tasks
- Must be explicitly scoped using the `scope` method
- Are cleaned up when the task completes

# Usage Patterns

## Configuration and Context

Task-locals are ideal for storing configuration or context that needs
to be available throughout a task's execution:

```
# use some_executor::task_local;
task_local! {
    static const REQUEST_ID: String;
    static const USER_ID: u64;
}

async fn process_request(request_id: String, user_id: u64) {
    REQUEST_ID.scope(request_id, async {
        USER_ID.scope(user_id, async {
            // Both REQUEST_ID and USER_ID are available
            // to all code called from here
            handle_business_logic().await;
        }).await;
    }).await;
}

async fn handle_business_logic() {
    // Can access task-locals without passing them as parameters
    REQUEST_ID.with(|id| {
        log_event("Processing", id.unwrap(), USER_ID.get());
    });
}

fn log_event(action: &str, request_id: &str, user_id: u64) {
    println!("[{}] {} for user {}", request_id, action, user_id);
}
```

## Mutable State

Mutable task-locals can maintain state throughout a task's execution:

```
# use some_executor::task_local;
task_local! {
    static EVENTS: Vec<String>;
}

async fn track_events() {
    EVENTS.scope(Vec::new(), async {
        record_event("started");
        process_data().await;
        record_event("completed");

        // Print all events at the end
        EVENTS.with(|events| {
            for event in events.unwrap() {
                println!("Event: {}", event);
            }
        });
    }).await;
}

fn record_event(event: &str) {
    EVENTS.with_mut(|events| {
        if let Some(vec) = events {
            vec.push(event.to_string());
        }
    });
}

async fn process_data() {
    record_event("processing data");
    // ... actual processing ...
}
```

## Request Tracing Example

A practical example showing how to use task-locals for distributed tracing:

```
# use some_executor::task_local;
# use std::time::{SystemTime, UNIX_EPOCH};
task_local! {
    static const TRACE_ID: String;
    static const SPAN_ID: String;
    static REQUEST_METRICS: RequestMetrics;
}

#[derive(Default, Clone)]
struct RequestMetrics {
    start_time: u64,
    db_queries: u32,
    cache_hits: u32,
}

async fn handle_http_request(request_id: String) {
    let trace_id = generate_trace_id();
    let span_id = generate_span_id();

    TRACE_ID.scope(trace_id.clone(), async {
        SPAN_ID.scope(span_id, async {
            REQUEST_METRICS.scope(RequestMetrics::default(), async {
                log_info("Request started");

                // Process the request
                let user = fetch_user_from_db(123).await;
                let data = fetch_user_data(user).await;

                // Log metrics at the end
                REQUEST_METRICS.with(|metrics| {
                    if let Some(m) = metrics {
                        log_info(&format!(
                            "Request completed: {} DB queries, {} cache hits",
                            m.db_queries, m.cache_hits
                        ));
                    }
                });
            }).await
        }).await
    }).await;
}

async fn fetch_user_from_db(id: u64) -> String {
    REQUEST_METRICS.with_mut(|metrics| {
        if let Some(m) = metrics {
            m.db_queries += 1;
        }
    });

    log_info(&format!("Fetching user {}", id));
    // Database query here...
    "user".to_string()
}

async fn fetch_user_data(user: String) -> Vec<u8> {
    // Check cache first
    REQUEST_METRICS.with_mut(|metrics| {
        if let Some(m) = metrics {
            m.cache_hits += 1;
        }
    });

    log_info(&format!("Fetching data for {}", user));
    vec![]
}

fn log_info(message: &str) {
    TRACE_ID.with(|trace_id| {
        SPAN_ID.with(|span_id| {
            let trace = trace_id.as_ref().map(|s| s.as_str()).unwrap_or("none");
            let span = span_id.as_ref().map(|s| s.as_str()).unwrap_or("none");
            println!(
                "[trace:{} span:{}] {}",
                trace,
                span,
                message
            );
        });
    });
}

fn generate_trace_id() -> String {
    format!("trace-{}", SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis())
}

fn generate_span_id() -> String {
    format!("span-{}", SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() % 1000000)
}
```

# Nested Scoping

Task-locals support nested scoping, where inner scopes shadow outer values:

```
# use some_executor::task_local;
task_local! {
    static LEVEL: u32;
    static const CONTEXT: String;
}

async fn demonstrate_nesting() {
    LEVEL.scope(1, async {
        CONTEXT.scope("outer".to_string(), async {
            CONTEXT.with(|ctx| {
                println!("Outer: level={}, context={}",
                         LEVEL.get(),
                         ctx.as_ref().unwrap());
            });

            LEVEL.scope(2, async {
                CONTEXT.scope("inner".to_string(), async {
                    CONTEXT.with(|ctx| {
                        println!("Inner: level={}, context={}",
                                 LEVEL.get(),
                                 ctx.as_ref().unwrap());

                        // Inner scope values: level=2, context="inner"
                        assert_eq!(LEVEL.get(), 2);
                        assert_eq!(ctx.as_ref().unwrap().as_str(), "inner");
                    });
                }).await;
            }).await;

            // Back to outer scope values
            CONTEXT.with(|ctx| {
                println!("After inner: level={}, context={}",
                         LEVEL.get(),
                         ctx.as_ref().unwrap());
                assert_eq!(LEVEL.get(), 1);
                assert_eq!(ctx.as_ref().unwrap().as_str(), "outer");
            });
        }).await;
    }).await;
}
```

# Safety and Best Practices

1. **Always use `with` for safe access**: The `get` method panics if the
   value is not set. Use `with` to handle the `None` case gracefully.

2. **Scope values appropriately**: Task-locals should be scoped at the
   appropriate level to avoid unnecessary overhead and ensure cleanup.

3. **Don't rely on inheritance**: Task-locals are not inherited by
   spawned tasks. Each task starts with no task-locals set.

4. **Consider immutability**: Use `const` task-locals for values that
   shouldn't change during execution, like configuration or IDs.

5. **Avoid holding borrows across await points**: When using `with` or
   `with_mut`, complete the operation before awaiting.

6. **Be mindful of performance**: Task-local access has some overhead due
   to thread-local storage access and `RefCell` checks.
*/

// Submodules
mod futures;
mod local_key;
mod local_key_immutable;
mod macros;

// Re-exports
pub use local_key::LocalKey;
pub use local_key_immutable::LocalKeyImmutable;
