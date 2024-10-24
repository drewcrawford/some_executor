use std::any::Any;
use std::future::Future;
use std::ops::Sub;
use std::pin::Pin;
use std::task::Poll;
use priority::Priority;
use crate::context::TaskLocalFuture;
use crate::hint::Hint;
use crate::observer::{observer_channel, Observer, ObserverNotifier, ObserverSender};
use crate::task_local;

/**
A top-level future.

The Task contains information that can be useful to an executor when deciding how to run the future.
*/
pub struct Task<F> where F: Future {
    future: TaskLocalFuture<Priority, TaskLocalFuture<String, F>>,
    hint: Hint,
    poll_after: std::time::Instant,
}

/**
A task suitable for spawning.

Executors convert [Task] into this type in order to poll the future.
*/
pub struct SpanwedTask<F,Notifier> where F: Future {
    task: Task<F>,
    sender: ObserverSender<F::Output,Notifier>
}

task_local! {
    pub static TASK_LABEL: String;
    pub static TASK_PRIORITY: priority::Priority;
}

impl<F: Future> Task<F> {
    pub fn new(label: String, future: F, configuration: Configuration) -> Self where F: Future {
        let apply_label = TASK_LABEL.scope_internal(label, future);
        let apply = TASK_PRIORITY.scope_internal(configuration.priority, apply_label);
        Task {
            future: apply,
            hint: configuration.hint,
            poll_after: configuration.poll_after,
        }
    }


    pub fn hint(&self) -> Hint {
        self.hint
    }

    pub fn label(&self) -> String {
        self.future.get_future().get_val(|label| label.clone())
    }

    pub fn priority(&self) -> priority::Priority {
        self.future.get_val(|priority| *priority)
    }

    pub fn poll_after(&self) -> std::time::Instant {
        self.poll_after
    }

    pub fn spawn<Notifier: ObserverNotifier<F::Output>>(self, notify: Option<Notifier>) -> (SpanwedTask<F,Notifier>, Observer<F::Output>) {
        let (sender, receiver) = observer_channel(notify);
        let spawned_task = SpanwedTask {
            task: self,
            sender
        };
        (spawned_task, receiver)
    }

}


impl<F,Notifier> Future for SpanwedTask<F,Notifier> where F: Future, Notifier: ObserverNotifier<F::Output> {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        assert!(self.task.poll_after <= std::time::Instant::now(), "Conforming executors should not poll tasks before the poll_after time.");
        //destructure
        let (future,sender) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.task.future);
            let sender = Pin::new_unchecked(&mut unchecked.sender);
            (future,sender)
        };

        match future.poll(cx) {
            Poll::Ready(r) => {

                sender.get_mut().send(r);
                Poll::Ready(())
            }
            Poll::Pending => {
                Poll::Pending
            }
        }


    }
}

impl Task<Pin<Box<dyn Future<Output=Box<dyn Any>> + Send + 'static>>> {
    pub fn new_objsafe(label: String, future: Box<dyn Future<Output=Box<dyn Any>> + Send + 'static>, configuration: Configuration) -> Self {
        Self::new(label, Box::into_pin(future), configuration)
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
    pub fn new(hint: Hint, priority: priority::Priority, poll_after: std::time::Instant) -> Self {
        Configuration {
            hint,
            priority,
            poll_after
        }
    }
}

#[cfg(test)] mod tests {
    use crate::task_local;
    #[test] fn test_send() {
        task_local!(
            static FOO: u32;
        );

        let scoped = FOO.scope(42, async {});

        fn assert_send<T: Send>(_: T) {}
        assert_send(scoped);


    }
}