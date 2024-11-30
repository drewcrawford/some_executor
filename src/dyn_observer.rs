use std::any::Any;
use std::marker::PhantomData;
use crate::observer::{Observation, Observer};
use crate::task::TaskID;

/**
This takes another observer of type `Observer<Value = Box<(dyn Any + Send + 'static)>>`
and downcasts the value to `Value` before calling the inner observer.
*/
pub struct DowncastObserver<O,V>(O,PhantomData<V>);

impl<O,V> Observer for DowncastObserver<O,V>
where O: Observer<Value=V> + 'static,
V: 'static {
    type Value = V;

    fn observe(&self) -> Observation<Self::Value> {
        self.0.observe()
    }

    fn task_id(&self) -> &TaskID {
        self.0.task_id()
    }
}

impl<O,V> DowncastObserver<O,V> {
    pub fn new(observer: O) -> Self {
        Self(observer, PhantomData)
    }
}


/**
This implements Observer for Box<dyn type>

todo: could probably be made more generic
*/
impl Observer for Box<dyn Observer<Value = Box<(dyn Any + Send + 'static)>>> {
    type Value = Box<dyn Any + Send + 'static>;

    fn observe(&self) -> Observation<Self::Value> {
        self.as_ref().observe()
    }

    fn task_id(&self) -> &TaskID {
        self.as_ref().task_id()
    }
}