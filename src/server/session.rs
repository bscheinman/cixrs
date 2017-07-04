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
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use tokio_core::reactor;
use uuid::Uuid;
use wal::Wal;

type SubscripionMap = HashMap<UserId, ExecutionSubscription>;
type SymbolMap = HashMap<Symbol, u32>;
type OrderWait = WaitEvent<ErrorCode>;
type SyncWait = WaitEvent<()>;
pub type OrderMap = HashMap<OrderId, OrderWait>;
pub type SyncMap = HashMap<u32, SyncWaitRecord>;
pub type OpenOrderMap = HashMap<OpenOrdersSequence, RefCell<OpenOrdersContext>>;

pub struct SyncWaitRecord {
    pub event: SyncWait,
    pub pending_count: Cell<u32>
}

pub trait OrderRouter {
    fn route_order(&self, msg: EngineMessage) -> Result<(), String>;
    fn create_order_id(&self, symbol: &Symbol, side: &OrderSide) -> Result<OrderId, String>;
    fn broadcast_message(&self, msg: EngineMessage) -> Result<(), String>;
    fn replay_message(&self, msg: EngineMessage) -> Result<(), String>;
    fn n_engine(&self) -> u32;
}

#[derive(Clone, Copy)]
pub enum ServerState {
    Loading,
    Running
}

// XXX: The fact that everything in here has to be wrapped in Rc and Cells seems like a really bad
// sign but I also don't see a good way around it given that an arbitrary number of sessions need
// to be able to observe this state (even though it really will only be mutated by a single class
// (not sessions).  There are two things we should do to improve this in the immediate term:
//
// 1) Separate out all fields that are only needed by the server controller itself and not each
//    individual session
// 2) Explore using weak pointers within sessions, although I don't think this will actually help
//    at all because Rc still can't be upgraded to a mutable reference if there are any outstanding
//    weak references
pub struct ServerContext<R> where R: 'static + Clone + OrderRouter {
    pub handle: reactor::Handle,
    pub router: R,
    pub sub_map: Rc<RefCell<SubscripionMap>>,
    pub pending_orders: Rc<RefCell<OrderMap>>,
    pub wal: RefCell<Wal>,
    // This is an Rc so it can be observed without sharing the entire context
    pub sync_gen: Rc<Cell<u32>>,
    pub sync_ticket: Cell<u32>,
    pub pending_syncs: RefCell<SyncMap>,
    pub state: Cell<ServerState>,
    pub pending_open_orders: Rc<RefCell<OpenOrderMap>>
}

impl<R> ServerContext<R> where R: 'static + Clone + OrderRouter {
    pub fn new(handle: reactor::Handle, router: R, wal: Wal) -> Self {
        ServerContext {
            handle: handle,
            router: router,
            sub_map: Rc::new(RefCell::new(SubscripionMap::new())),
            pending_orders: Rc::new(RefCell::new(OrderMap::new())),
            wal: RefCell::new(wal),
            sync_gen: Rc::new(Cell::new(0u32)),
            sync_ticket: Cell::new(0u32),
            pending_syncs: RefCell::new(SyncMap::new()),
            state: Cell::new(ServerState::Loading),
            pending_open_orders: Rc::new(RefCell::new(OpenOrderMap::new()))
        }
    }

    pub fn serialization_point<T>(ctx: T) -> SerializationPoint<Rc<Cell<u32>>>
            where T: AsRef<Self> {
        let context = ctx.as_ref();
        let ticket = context.sync_ticket.get() + 1;
        context.sync_ticket.set(ticket);

        context.router.broadcast_message(EngineMessage::SerializationMessage(ticket)).unwrap();
        let sync_record = SyncWaitRecord {
            event: SyncWait::new(),
            pending_count: Cell::new(context.router.n_engine())
        };
        context.pending_syncs.borrow_mut().insert(ticket, sync_record);

        SerializationPoint {
            gen: context.sync_gen.clone(),
            target: ticket
        }
    }
}

pub struct Session<R> where R: 'static + Clone + OrderRouter {
    context: Rc<ServerContext<R>>,
    user: UserId,
    authenticated: bool,
    open_order_seq: u32
}

impl<R> Session<R> where R: 'static + Clone + OrderRouter {
    pub fn new(context: Rc<ServerContext<R>>) -> Self {
        Session {
            context: context,
            user: 0u64,
            authenticated: false,
            open_order_seq: 0u32
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
        let symbol = pry!(Symbol::from_capnp(pry!(order.get_symbol())).map_err(|e| {
            capnp::Error::failed("invalid symbol".to_string())
        }));
        let side = OrderSide::from(pry!(order.get_side()));
        let order_id = pry!(self.context.router.create_order_id(&symbol, &side).map_err(|e| {
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

        // XXX: Move the WAL write to engine threads; this would also allow order ID assignment to
        // happen on those threads and remove some of the Rc<RefCell<T>> garbage we have going on
        // here
        pry!(self.context.wal.borrow_mut().write_entry(&msg).map_err(|e| {
            capnp::Error::failed(e)
        }));

        let send = pry!(self.context.router.route_order(msg).map_err(|e| {
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

        let send = pry!(self.context.router.route_order(msg).map_err(|e| {
            capnp::Error::failed("internal error".to_string())
        }));

        results.get().set_code(cp::ErrorCode::Ok);
        Promise::ok(())
    }

    fn get_open_orders(&mut self, params: GetOpenOrdersParams,
                       mut results: GetOpenOrdersResults)
                       -> Promise<(), capnp::Error> {
        if !self.authenticated {
            results.get().set_code(cp::ErrorCode::NotAuthenticated);
            return Promise::ok(());
        }

        let seq = OpenOrdersSequence {
            user: self.user,
            seq: self.open_order_seq
        };

        self.open_order_seq += 1;

        let msg = EngineMessage::GetOpenOrdersMessaage(seq.clone());

        let send = pry!(self.context.router.broadcast_message(msg).map_err(|e| {
            capnp::Error::failed("internal error".to_string())
        }));

        let send_future = OpenOrdersSend::new(seq.clone(),
            self.context.pending_open_orders.clone());

        self.context.pending_open_orders.borrow_mut().insert(seq,
            RefCell::new(OpenOrdersContext::new(self.context.router.n_engine() as usize)));

        Promise::from_future(send_future.and_then(move |o| {
            let orders = o.borrow();
            println!("found {} orders", orders.len());
            results.get().set_code(cp::ErrorCode::Ok);

            let mut ret_orders = results.get().init_orders(orders.len() as u32);
            for (i, order) in orders.iter().enumerate() {
                let order_out = ret_orders.borrow().get(i as u32);
                order.to_capnp(order_out);
            }

            Ok(())
        }).map_err(|e| {
            capnp::Error::failed("internal error".to_string())
        }))
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
