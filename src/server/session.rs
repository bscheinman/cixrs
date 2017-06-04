use capnp;
use capnp::capability::Promise;
use engine::*;
use events::*;
use messages::*;
use futures::{future, Future, Stream};
use futures::sink::Sink;
use futures::sync::mpsc;
use libcix::cix_capnp as cp;
use cp::trading_session::*;
use libcix::order::trade_types::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tokio_core::reactor;
use uuid::Uuid;
use wal::Wal;

type SubscripionMap = HashMap<UserId, ExecutionSubscription>;
type SymbolMap = HashMap<Symbol, u32>;
pub type OrderMap = HashMap<OrderId, OrderWait>;

// XXX: this will expose things like symbol and any other information
// needed for routing orders, but right now we don't need any of that
pub enum OrderRoutingInfo {
    NewOrderInfo    { symbol: Symbol, side: OrderSide },
    ModifyOrderInfo { symbol_id: u32 }
}

pub trait OrderRouter {
    fn route_order(&self, o: &OrderRoutingInfo, msg: EngineMessage) -> Result<(), String>;
    fn create_order_id(&self, o: &OrderRoutingInfo) -> Result<OrderId, String>;
}

pub struct ServerContext<R> where R: 'static + Clone + OrderRouter {
    pub handle: reactor::Handle,
    pub router: R,
    pub sub_map: Rc<RefCell<SubscripionMap>>,
    pub pending_orders: Rc<RefCell<OrderMap>>,
    pub wal: RefCell<Wal>
}

impl<R> ServerContext<R> where R: 'static + Clone + OrderRouter {
    pub fn new(handle: reactor::Handle, router: R, wal: Wal) -> Self {
        ServerContext {
            handle: handle,
            router: router,
            sub_map: Rc::new(RefCell::new(SubscripionMap::new())),
            pending_orders: Rc::new(RefCell::new(OrderMap::new())),
            wal: RefCell::new(wal)
        }
    }
}

pub struct Session<R> where R: 'static + Clone + OrderRouter {
    context: Rc<ServerContext<R>>,
    user: UserId,
    authenticated: bool,
}

impl<R> Session<R> where R: 'static + Clone + OrderRouter {
    pub fn new(context: Rc<ServerContext<R>>) -> Self {
        Session {
            context: context,
            user: 0u64,
            authenticated: false
        }
    }
}

pub struct ExecutionSubscription {
    pub client: cp::execution_feed::Client
}

impl ExecutionSubscription {
    pub fn new(client: cp::execution_feed::Client) -> Self {
        ExecutionSubscription {
            client: client
        }
    }
}

struct ExecutionSubscriptionMd {
    user: UserId,
    sub_map: Rc<RefCell<SubscripionMap>>
}

impl ExecutionSubscriptionMd {
    fn new(user: UserId, sub_map: Rc<RefCell<SubscripionMap>>) -> Self {
        ExecutionSubscriptionMd {
            user: user,
            sub_map: sub_map
        }
    }
}

impl Drop for ExecutionSubscriptionMd {
    fn drop(&mut self) {
        self.sub_map.borrow_mut().remove(&self.user);
    }
}

impl cp::execution_feed_subscription::Server for ExecutionSubscriptionMd {}

impl<R> Server for Session<R> where R: 'static + Clone + OrderRouter {
    fn authenticate(&mut self, params: AuthenticateParams, mut results: AuthenticateResults)
                    -> Promise<(), capnp::Error> {
        self.user = pry!(params.get()).get_user();
        self.authenticated = true;

        println!("new session for user {}", self.user);

        results.get().set_response(cp::AuthCode::Ok);
        Promise::ok(())
    }

    fn new_order(&mut self, params: NewOrderParams, mut results: NewOrderResults)
                 -> Promise<(), capnp::Error> {
        if !self.authenticated {
            results.get().set_code(cp::ErrorCode::NotAuthenticated);
            return Promise::ok(());
        }

        let order = pry!(pry!(params.get()).get_order());
        let symbol = pry!(read_symbol(pry!(order.get_symbol())).map_err(|e| {
            capnp::Error::failed("invalid symbol".to_string())
        }));
        let side = OrderSide::from(pry!(order.get_side()));
        let order_info = OrderRoutingInfo::NewOrderInfo {
            symbol: symbol,
            side: side
        };
        let order_id = pry!(self.context.router.create_order_id(&order_info).map_err(|e| {
            capnp::Error::failed(e)
        }));

        let msg = EngineMessage::NewOrder(NewOrderMessage {
            user: self.user,
            order_id: order_id,
            symbol: symbol,
            side: side,
            price: order.get_price(),
            quantity: order.get_quantity()
        });

        pry!(self.context.wal.borrow_mut().write_entry(&msg).map_err(|e| {
            capnp::Error::failed(e)
        }));

        let send = pry!(self.context.router.route_order(&order_info, msg).map_err(|e| {
            capnp::Error::failed("internal error".to_string())
        }));

        // Register this task to handle the engine's response and communicate it
        // to the client
        let send_future = NewOrderSend::new(order_id,
                                            self.context.pending_orders.clone());
        self.context.pending_orders.borrow_mut().insert(order_id,
                                                        OrderWait::new());

        Promise::from_future(send_future.and_then(move |c| {
            let ret_code = match c {
                ErrorCode::Success => cp::ErrorCode::Ok,
                _ => cp::ErrorCode::Other
            };
            println!("received ack for order {}", order_id);
            results.get().set_code(ret_code);
            results.get().set_id(order_id.raw());
            Ok(())
        }).map_err(|e| {
            capnp::Error::failed("internal error".to_string())
        }))
    }

    fn cancel_order(&mut self, params: CancelOrderParams, mut results: CancelOrderResults)
                    -> Promise<(), capnp::Error> {
        if !self.authenticated {
            results.get().set_code(cp::ErrorCode::NotAuthenticated);
            return Promise::ok(());
        }

        let raw_order_id = pry!(pry!(params.get()).get_cancel()).get_id();
        let order_id = match OrderId::from_raw(raw_order_id) {
            Ok(id) => id,
            Err(_) => {
                results.get().set_code(cp::ErrorCode::InvalidArgs);
                return Promise::ok(());
            }
        };

        let msg = EngineMessage::CancelOrder(CancelOrderMessage {
            user:       self.user,
            order_id:   order_id
        });

        pry!(self.context.wal.borrow_mut().write_entry(&msg).map_err(|e| {
            capnp::Error::failed(e)
        }));

        let order_info = OrderRoutingInfo::ModifyOrderInfo { symbol_id: order_id.symbol_id() };

        let send = pry!(self.context.router.route_order(&order_info, msg).map_err(|e| {
            capnp::Error::failed("internal error".to_string())
        }));

        results.get().set_code(cp::ErrorCode::Ok);
        Promise::ok(())
    }

    fn execution_subscribe(&mut self, params: ExecutionSubscribeParams,
                           mut results: ExecutionSubscribeResults)
            -> Promise<(), capnp::Error> {
        if !self.authenticated {
            results.get().set_code(cp::ErrorCode::NotAuthenticated);
            return Promise::ok(());
        }

        let ref mut sub_map = *(self.context.sub_map.borrow_mut());
        if sub_map.contains_key(&self.user) {
            results.get().set_code(cp::ErrorCode::AlreadySubscribed);
            return Promise::ok(());
        }

        let subscriber = pry!(pry!(params.get()).get_feed());
        sub_map.insert(self.user, ExecutionSubscription::new(subscriber));

        results.get().set_code(cp::ErrorCode::Ok);
        results.get().set_sub(cp::execution_feed_subscription::ToClient::new(
                ExecutionSubscriptionMd::new(self.user, self.context.sub_map.clone()))
                .from_server::<::capnp_rpc::Server>());
        Promise::ok(())
    }
}
