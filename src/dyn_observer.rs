use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use crate::observer::{FinishedObservation, Observation, Observer};
use crate::task::TaskID;

/**
This takes another observer of type `Observer<Value = Box<(dyn Any + Send + 'static)>>`
and downcasts the value to `Value` before calling the inner observer.
*/
pub struct DowncastObserver<O,V>(O,PhantomData<V>);

impl<O,V> Observer for DowncastObserver<O,V>
where O: Observer<Value=Box<dyn Any + Send + 'static>> + 'static,
V: 'static {
    type Value = V;

    fn observe(&self) -> Observation<Self::Value> {
        let inner = self.0.observe();
        match inner {
            Observation::Pending => Observation::Pending,

            Observation::Ready(e) => {
                let downcasted = e.downcast::<V>().expect("Downcast failed");
                Observation::Ready(*downcasted)
            }
            Observation::Done => Observation::Done,
            Observation::Cancelled => Observation::Cancelled
        }
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

impl<O,V> Future for DowncastObserver<O,V>
where O: Future<Output = FinishedObservation<Box<dyn Any + Send + 'static>>> + 'static,
V: 'static {
    type Output = FinishedObservation<V>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let f = unsafe{self.map_unchecked_mut(|s| &mut s.0)}.poll(cx);
        match f {
            Poll::Pending => Poll::Pending,
            Poll::Ready(e) => {
                match e {
                    FinishedObservation::Cancelled => Poll::Ready(FinishedObservation::Cancelled),
                    FinishedObservation::Ready(e) => {
                        let downcasted = e.downcast::<V>().expect("Downcast failed");
                        Poll::Ready(FinishedObservation::Ready(*downcasted))
                    }
                }
            }
        }

    }
}

/**
This implements Observer for Box<dyn type>

todo: could probably be made more generic
*/
impl<V> Observer for Box<dyn Observer<Value = V, Output = FinishedObservation<V>>>
where V: 'static {
    type Value = V;

    fn observe(&self) -> Observation<Self::Value> {
        self.as_ref().observe()
    }

    fn task_id(&self) -> &TaskID {
        self.as_ref().task_id()
    }
}

impl<V> Future for Box<dyn Observer<Value = V, Output = FinishedObservation<V>>>
where
    V: 'static,
{
    type Output = FinishedObservation<V>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //Box guarantees the inner value is pinned as long as the Box itself is pinned,
        let map = unsafe{self.as_mut().map_unchecked_mut(|s| &mut **s)};
        map.poll(cx)
    }
}