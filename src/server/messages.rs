use libcix::order::trade_types::*;

pub const OPEN_ORDER_MSG_MAX_LENGTH: usize = 10;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct OpenOrdersSequence {
    pub user: UserId,
    pub seq: u32
}

pub struct OpenOrders {
    pub seq: OpenOrdersSequence,
    pub n_order: u32,
    pub orders: [Order; OPEN_ORDER_MSG_MAX_LENGTH],
    pub last_response: bool
}

impl OpenOrders {
    pub fn new(seq: OpenOrdersSequence) -> Self {
        OpenOrders {
            seq: seq,
            n_order: 0u32,
            orders: [Order::default(); OPEN_ORDER_MSG_MAX_LENGTH],
            last_response: false
        }
    }
}

// XXX: Rename now that this includes control metadata as well
pub enum SessionMessage {
    NewOrderAck {
        order_id: OrderId,
        status: ErrorCode
    },
    Execution(Execution),
    SerializationResponse(u32),
    OpenOrdersResponse(OpenOrders)
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
    CancelOrder(CancelOrderMessage),
    // Don't respond to this until all previous messages have been processed
    SerializationMessage(u32),
    GetOpenOrdersMessaage(OpenOrdersSequence)
}

#[derive(Clone, Copy, Debug)]
pub enum MdMessage {
    L1Message(L1Md),
    L2Message(L2Md),
    Execution(MdExecution)
}
