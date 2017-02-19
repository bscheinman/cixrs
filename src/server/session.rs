use capnp;
use capnp::capability::Promise;
use engine::{EngineMessage, NewOrderMessage};
use futures::sink::Sink;
use libcix::cix_capnp as cp;
use self::cp::trading_session::*;
use libcix::order::trade_types::*;
use tokio_core::reactor;
use uuid::Uuid;

// XXX: this will expose things like symbol and any other information
// needed for routing orders, but right now we don't need any of that
pub struct OrderRoutingInfo;

pub trait OrderRouter {
    fn route_order(&self, o: &OrderRoutingInfo, msg: EngineMessage) ->
        Result<(), String>;
    fn create_order_id(&self, o: &OrderRoutingInfo) -> OrderId;
}

pub struct Session<R> where R: OrderRouter {
    handle: reactor::Handle,
    router: R,
    user: Uuid,
}

impl<R> Session<R> where R: OrderRouter {
    pub fn new(handle: reactor::Handle, router: R) -> Self {
        Session {
            handle: handle,
            router: router,
            user: Uuid::default()
        }
    }
}

impl<R> Server for Session<R> where R: OrderRouter {
    fn authenticate(&mut self, params: AuthenticateParams,
                    mut results: AuthenticateResults)
                    -> Promise<(), capnp::Error> {
        let raw_uuid = pry!(pry!(params.get()).get_user());
        let userid = pry!(read_uuid(raw_uuid).map_err(|e| {
            capnp::Error::failed("invalid userid".to_string())
        }));

        self.user = userid;

        println!("new session for user {}", self.user);

        results.get().set_response(cp::AuthCode::Ok);
        Promise::ok(())
    }

    fn new_order(&mut self, params: NewOrderParams,
                 mut results: NewOrderResults)
            -> Promise<(), capnp::Error> {
        let order_info = OrderRoutingInfo{};
        let order_id = self.router.create_order_id(&order_info);
        let order = pry!(pry!(params.get()).get_order());
        let symbol = pry!(read_symbol(pry!(order.get_symbol())).map_err(|e| {
            capnp::Error::failed("invalid symbol".to_string())
        }));

        let msg = EngineMessage::NewOrder(NewOrderMessage {
            user: self.user,
            order_id: order_id,
            symbol: symbol,
            side: OrderSide::from(pry!(order.get_side())),
            price: order.get_price(),
            quantity: order.get_quantity()
        });

        let send = self.router.route_order(&order_info, msg);
        
        match send {
            Err(s) => {
                results.get().set_code(cp::ErrorCode::InternalError);
                Promise::err(capnp::Error::failed(format!("internal error {}", s)))
            },
            Ok(()) => {
                results.get().set_code(cp::ErrorCode::Ok);
                pry!(results.get().get_id()).set_bytes(order_id.as_bytes());
                Promise::ok(())
            }
        }
    }
}
