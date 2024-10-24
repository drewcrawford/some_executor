use std::future::Future;
use crate::hint::Hint;

/**
A top-level future.

The Task contains information that can be useful to an executor when deciding how to run the future.
*/
pub struct Task<F> {
    label: &'static str,
    future: F,
    configuration: Configuration,
}

impl<F> Task<F> {
    pub fn new(label: &'static str, future: F, configuration: Configuration) -> Self {
        Task {
            label,
            future,
            configuration
        }
    }
    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn hint(&self) -> Hint {
        self.configuration.hint
    }

    pub fn priority(&self) -> priority::Priority {
        self.configuration.priority
    }

    pub fn poll_after(&self) -> std::time::Instant {
        self.configuration.poll_after
    }
}

impl<F> Future for Task<F> where F: Future {
    type Output = F::Output;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let f = unsafe { self.map_unchecked_mut(|s| &mut s.future) };
        f.poll(cx)
    }
}

/**
Information needed to spawn a task.
*/
pub struct Configuration {
    hint: Hint,
    priority: priority::Priority,
    poll_after: std::time::Instant,
}

pub struct ConfigurationBuilder {
    hint: Option<Hint>,
    priority: Option<priority::Priority>,
    poll_after: Option<std::time::Instant>,
}

impl ConfigurationBuilder {
    pub fn new() -> Self {
        ConfigurationBuilder {
            hint: None,
            priority: None,
            poll_after: None,
        }
    }

    /**
    Provide a hint about the runtime characteristics of the future.
    */

    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }

    /**
    Provide a priority for the future.

    See the [Priority] type for details.
    */
    pub fn priority(mut self, priority: priority::Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /**
    Provide a time after which the future should be polled.
*/
    pub fn poll_after(mut self, poll_after: std::time::Instant) -> Self {
        self.poll_after = Some(poll_after);
        self
    }

    pub fn build(self) -> Configuration {
        Configuration {
            hint: self.hint.unwrap_or_else(|| Hint::default()),
            priority: self.priority.unwrap_or_else(|| priority::Priority::Unknown),
            poll_after: self.poll_after.unwrap_or_else(|| std::time::Instant::now()),
        }
    }
}

impl Configuration {
    fn new(hint: Hint, priority: priority::Priority, poll_after: std::time::Instant) -> Self {
        Configuration {
            hint,
            priority,
            poll_after
        }
    }
}