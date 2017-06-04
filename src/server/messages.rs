use libcix::order::trade_types::*;
use session::OrderMap;
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

pub struct OrderWait {
    pub status: Cell<Option<ErrorCode>>,
    task: Task
}

impl OrderWait {
    pub fn new() -> Self {
        OrderWait {
            status: Cell::new(None),
            task: park()
        }
    }

    pub fn ack(&self, status: ErrorCode) {
        self.status.set(Some(status));
        self.task.unpark();
    }
}

pub enum SessionMessage {
    NewOrderAck {
        order_id: OrderId,
        status: ErrorCode
    },
    Execution(Execution)
}

#[derive(Serialize, Deserialize)]
pub struct NewOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId,
    pub symbol:     Symbol,
    pub side:       OrderSide,
    pub price:      Price,
    pub quantity:   Quantity
}

#[derive(Serialize, Deserialize)]
pub struct ChangeOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId,
    pub price:      Price,
    pub quantity:   Quantity
}

#[derive(Serialize, Deserialize)]
pub struct CancelOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId
}

#[derive(Serialize, Deserialize)]
pub enum EngineMessage {
    NewOrder(NewOrderMessage),
    //ChangeOrder(ChangeOrderMessage),
    CancelOrder(CancelOrderMessage)
}


