// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::observer::ObserverNotified;
use std::any::Any;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/**
Wraps an observer of known value type into an observer of 'any' value type.

The resulting observer implements ObserverNotified<dyn Any + Send + 'static> and can be used in place of the original observer.
*/
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObserverNotifiedErased<O, V>(O, PhantomData<V>)
where
    O: ObserverNotified<V>;

impl<Observer, Value> ObserverNotified<dyn Any + Send + 'static>
    for ObserverNotifiedErased<Observer, Value>
where
    Observer: ObserverNotified<Value>,
    Value: Unpin + 'static,
{
    fn notify(&mut self, value: &(dyn Any + Send + 'static)) {
        let downcast = value.downcast_ref::<Value>().expect("Downcast failed");
        self.0.notify(downcast);
    }
}

impl<O, V> ObserverNotifiedErased<O, V>
where
    O: ObserverNotified<V>,
{
    pub fn new(observer: O) -> Self {
        Self(observer, PhantomData)
    }
}

//boilerplate

impl<O, V> From<O> for ObserverNotifiedErased<O, V>
where
    O: ObserverNotified<V>,
{
    fn from(observer: O) -> Self {
        Self::new(observer)
    }
}

impl<O, V> ObserverNotifiedErased<O, V>
where
    O: ObserverNotified<V>,
{
    // pub fn into_inner(self) -> O {
    //     self.0
    // }
}

impl<O, V> AsRef<O> for ObserverNotifiedErased<O, V>
where
    O: ObserverNotified<V>,
{
    fn as_ref(&self) -> &O {
        &self.0
    }
}

impl<O, V> AsMut<O> for ObserverNotifiedErased<O, V>
where
    O: ObserverNotified<V>,
{
    fn as_mut(&mut self) -> &mut O {
        &mut self.0
    }
}

impl<O, V> Deref for ObserverNotifiedErased<O, V>
where
    O: ObserverNotified<V>,
{
    type Target = O;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<O, V> DerefMut for ObserverNotifiedErased<O, V>
where
    O: ObserverNotified<V>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
