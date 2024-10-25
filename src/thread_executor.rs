use crate::SomeExecutor;

thread_local! {
    // pub static THREAD_EXECUTOR: Box<dyn SomeExecutor> = Box::new(crate::thread::ThreadExecutor::new());
}