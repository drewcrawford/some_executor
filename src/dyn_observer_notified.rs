use std::any::Any;
use std::marker::PhantomData;
use crate::observer::{ObserverNotified};

/**
Wraps an observer of known type into an observer of erased type.
*/
pub struct ObserverNotifiedErased<O,V>(O,PhantomData<V>) where O: ObserverNotified<V>;

impl<Observer,Value> ObserverNotified<dyn Any + Send + 'static> for ObserverNotifiedErased<Observer,Value>
where Observer: ObserverNotified<Value>,
    Value: Unpin + 'static,
{
    fn notify(&mut self, value: &(dyn Any + Send + 'static)) {
        let downcast = value.downcast_ref::<Value>().expect("Downcast failed");
        self.0.notify(downcast);
    }
}

impl<O,V> ObserverNotifiedErased<O,V> where O: ObserverNotified<V> {
    pub fn new(observer: O) -> Self {
        Self(observer, PhantomData)
    }
}

