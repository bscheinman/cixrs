use libcix::order::trade_types::*;
use messages::*;
use session::{OpenOrderMap, OrderMap, OrderRouter, ServerContext};
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

pub struct OpenOrdersContext {
    in_flight: usize,
    orders: Rc<RefCell<Vec<Order>>>,
    task: Task
}

#[derive(Clone)]
pub struct OpenOrdersSend {
    seq: OpenOrdersSequence,
    context_map: Rc<RefCell<OpenOrderMap>>
}

impl OpenOrdersSend {
    pub fn new(seq: OpenOrdersSequence, context_map: Rc<RefCell<OpenOrderMap>>) -> Self {
        OpenOrdersSend {
            seq: seq,
            context_map: context_map
        }
    }
}

impl Future for OpenOrdersSend {
    type Item = Rc<RefCell<Vec<Order>>>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.context_map.borrow().get(&self.seq) {
            Some(c) => {
                let context = c.borrow();

                if context.in_flight > 0 {
                    Ok(Async::NotReady)
                } else {
                    Ok(Async::Ready(context.orders.clone()))
                }
            },
            None => {
                println!("received open order response for unregistered identifier {}/{}",
                         self.seq.user, self.seq.seq);
                Err(())
            }
        }
    }
}

impl OpenOrdersContext {
    pub fn new(in_flight: usize) -> Self {
        OpenOrdersContext {
            in_flight: in_flight,
            orders: Rc::new(RefCell::new(Vec::new())),
            task: park()
        }
    }

    pub fn recv(&mut self, msg: &OpenOrders) {
        assert!((msg.n_order as usize) < OPEN_ORDER_MSG_MAX_LENGTH);
        self.orders.borrow_mut().extend(msg.orders[0usize .. msg.n_order as usize].iter().map(|o| {
            o.clone()
        }).collect::<Vec<Order>>());
        println!("received {} orders for user {} ({} total)", msg.n_order, msg.seq.user,
            self.orders.borrow().len());
        if msg.last_response {
            self.in_flight -= 1;
            if self.in_flight == 0 {
                self.task.unpark();
            }
        }
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
