use libcix::order::trade_types::*;
use session::{OrderMap, OrderRouter, ServerContext};
use futures::{Async, Poll};
use futures::future::Future;
use futures::task::{park, Task};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone)]
pub struct NewOrderSend {
    order_id: OrderId,
    status_map: Rc<RefCell<OrderMap>>
}

impl NewOrderSend {
    pub fn new(order_id: OrderId, status_map: Rc<RefCell<OrderMap>>) -> Self {
        NewOrderSend {
            order_id: order_id,
            status_map: status_map
        }
    }
}

impl Future for NewOrderSend {
    type Item = ErrorCode;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(ref entry) = self.status_map.borrow().get(&self.order_id) {
            match entry.status.get() {
                Some(c) => Ok(Async::Ready(c.clone())),
                None => Ok(Async::NotReady)
            }
        } else {
            Ok(Async::NotReady)
        }
    }
}

impl Drop for NewOrderSend {
    fn drop(&mut self) {
        self.status_map.borrow_mut().remove(&self.order_id);
    }
}

#[derive(Clone)]
pub struct SerializationPoint<T> where T: AsRef<Cell<u32>> {
    pub gen: T,
    pub target: u32
}

impl<T> Future for SerializationPoint<T> where T: AsRef<Cell<u32>> {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if self.gen.as_ref().get() >= self.target {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }
}

// This must already be provided somewhere in tokio
pub struct WaitEvent<T> {
    pub status: Cell<Option<T>>,
    task: Task
}

impl<T> WaitEvent<T> {
    pub fn new() -> Self {
        WaitEvent {
            status: Cell::new(None),
            task: park()
        }
    }

    pub fn ack(&self, result: T) {
        self.status.set(Some(result));
        self.task.unpark();
    }
}
