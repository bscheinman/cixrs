use libcix::order::trade_types::*;

pub enum SessionMessage {
    NewOrderAck {
        order_id: OrderId,
        status: ErrorCode
    },
    Execution(Execution)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct NewOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId,
    pub symbol:     Symbol,
    pub side:       OrderSide,
    pub price:      Price,
    pub quantity:   Quantity
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ChangeOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId,
    pub price:      Price,
    pub quantity:   Quantity
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CancelOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum EngineMessage {
    // This is a temporary hack to avoid reading messages from empty log files
    NullMessage,
    NewOrder(NewOrderMessage),
    //ChangeOrder(ChangeOrderMessage),
    CancelOrder(CancelOrderMessage)
}
