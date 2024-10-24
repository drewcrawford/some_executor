use std::future::Future;
use std::ops::Sub;
use std::pin::Pin;
use crate::context::TaskLocalFuture;
use crate::hint::Hint;
use crate::task_local;

/**
A top-level future.

The Task contains information that can be useful to an executor when deciding how to run the future.
*/
pub struct Task<F> {
    future: TaskLocalFuture<String, F>,
    configuration: Configuration,
}

task_local! {
    static TASK_LABEL: String;
}

impl<F> Task<F> {
    pub fn new(label: String, future: F, configuration: Configuration) -> Self where F: Future {
        let apply_label = TASK_LABEL.scope(label, future);
        Task {
            future: apply_label,
            configuration,
        }
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
        let (future) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.future);
            (future)
        };

        future.poll(cx)


    }
}

impl Task<Pin<Box<dyn Future<Output=()> + Send + 'static>>> {
    pub fn new_objsafe(label: String, future: Box<dyn Future<Output=()> + Send + 'static>, configuration: Configuration) -> Self {
        let apply_label = TASK_LABEL.scope(label, Box::into_pin(future));
        Task {
            future: apply_label,
            configuration,
        }
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

#[cfg(test)] mod tests {
    use crate::task_local;
    use super::TaskLocalFuture;
    #[test] fn test_send() {
        task_local!(
            static FOO: u32;
        );

        let scoped = FOO.scope(42, async {});

        fn assert_send<T: Send>(_: T) {}
        assert_send(scoped);


    }
}