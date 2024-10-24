use std::future::Future;
use std::ops::Sub;
use std::pin::Pin;
use crate::context::TaskContext;
use crate::hint::Hint;

/**
A top-level future.

The Task contains information that can be useful to an executor when deciding how to run the future.
*/
pub struct Task<F> {
    label: &'static str,
    future: F,
    configuration: Configuration,
    context: Option<TaskContext>,
}

impl<F> Task<F> {
    pub fn new(label: &'static str, future: F, configuration: Configuration) -> Self {
        Task {
            label,
            future,
            configuration,
            context: Some(TaskContext::default()),
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
        assert!(self.configuration.poll_after <= std::time::Instant::now(), "Conforming executors should not poll tasks before the poll_after time.");
        //destructure
        let (future, mut context) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.future);
            let context = Pin::new(&mut unchecked.context);
            (future,context)
        };


        TaskContext::current_mut(|c| {
            *c = context.take();
            assert!(c.is_some(), "TaskContext should be available.");
        });
        let poll_result = future.poll(cx);
        TaskContext::current_mut(|c| {
            context.replace(c.take().expect("TaskContext should be available."));
        });
        poll_result

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
            poll_after: self.poll_after.unwrap_or_else(|| std::time::Instant::now().sub(std::time::Duration::from_secs(1))),
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