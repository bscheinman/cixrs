extern crate capnp;
#[macro_use]
extern crate capnp_rpc;
extern crate futures;
extern crate futures_cpupool;
extern crate libcix;
extern crate time;
extern crate tokio_core;
extern crate uuid;

mod engine;
mod session;

use engine::{EngineHandle, EngineMessage};
use futures::{future, Future, Stream};
use futures::sink::Sink;
use futures::sync::mpsc;
use libcix::book::{BasicMatcher, ExecutionHandler};
use libcix::cix_capnp as cp;
use libcix::order::trade_types;
use session::{OrderRouter, OrderRoutingInfo, ServerContext};
use std::net::ToSocketAddrs;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpListener;

#[derive(Clone)]
struct ExecutionPrinter;

impl ExecutionHandler for ExecutionPrinter {
    fn handle_match(&self, execution: trade_types::Execution) {
        println!("{}", execution)
    }
}

#[derive(Clone)]
struct FeedExecutionHandler {
    tx: mpsc::Sender<trade_types::Execution>
}

impl ExecutionHandler for FeedExecutionHandler {
    fn handle_match(&self, execution: trade_types::Execution) {
        self.tx.clone().send(execution).wait();
    }
}

// XXX: For now just use a single engine for all symbols
// Later on we can either shard by symbol or use a lookup or whatever
#[derive(Clone)]
struct SingleRouter {
    tx: mpsc::Sender<EngineMessage>
}

impl SingleRouter {
    pub fn new(tx: mpsc::Sender<EngineMessage>) -> Self {
        SingleRouter {
            tx: tx
        }
    }
}

impl OrderRouter for SingleRouter {
    fn route_order(&self, o: &OrderRoutingInfo, msg: EngineMessage) ->
        Result<(), String> {
        self.tx.clone().send(msg).wait();
        Ok(())
    }

    fn create_order_id(&self, o: &OrderRoutingInfo) ->
            trade_types::OrderId {
        trade_types::OrderId::new_v4()
    }
}

struct ExecutionPublisher<R> where R: 'static + Clone + OrderRouter {
    rx: mpsc::Receiver<trade_types::Execution>,
    context: ServerContext<R>
}

impl<R> ExecutionPublisher<R> where R: 'static + Clone + OrderRouter {
    fn new(rx: mpsc::Receiver<trade_types::Execution>, context: ServerContext<R>) -> Self {
        ExecutionPublisher {
            rx: rx,
            context: context
        }
    }

    fn handle_executions(self) {
        let context = self.context.clone();
        let exec_feed = self.rx.for_each(move |execution| {
            Self::handle_execution_side(&context, &execution, trade_types::OrderSide::Buy);
            Self::handle_execution_side(&context, &execution, trade_types::OrderSide::Sell);
            future::ok(())
        });

        self.context.handle.spawn(exec_feed);
    }

    fn handle_execution_side(context: &ServerContext<R>, execution: &trade_types::Execution,
                             side: trade_types::OrderSide) -> Result<(), ()> {
        let exec_id = execution.id;
        let (user, order) = match side {
            trade_types::OrderSide::Buy => (execution.buy_user, execution.buy_order),
            trade_types::OrderSide::Sell => (execution.sell_user, execution.sell_order)
        };

        let sub_map = context.sub_map.borrow();
        let subscription = match sub_map.get(&user) {
            Some(sub) => sub,
            None => { return Ok(()); }
        };

        let mut msg = subscription.client.execution_request();
        {
            let mut builder = try!(msg.get().get_execution().map_err(|_| ()));
            builder.set_symbol(execution.symbol.as_str());
            builder.set_price(execution.price);
            builder.set_quantity(execution.quantity);

            {
                let mut ts_builder = try!(builder.borrow().get_ts().map_err(|_| ()));
                ts_builder.set_seconds(execution.ts.sec);
                ts_builder.set_nanos(execution.ts.nsec);
            }

            {
                let mut id_builder = try!(builder.borrow().get_id().map_err(|_| ()));
                id_builder.set_bytes(execution.id.as_bytes());
            }

            {
                let mut order_builder = try!(builder.borrow().get_order().map_err(|_| ()));
                order_builder.set_bytes(order.as_bytes());
            }
        }

        context.handle.spawn(msg.send().promise.then(move |r| {
            if let Err(e) = r {
                println!("failed to send execution {} to user {}: {}", exec_id, user, e);
            }

            Ok::<(), ()>(())
        }));
        Ok(())
    }
}

fn main() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();

    let symbols = vec!["GOOG"].into_iter().map(|x| {
        trade_types::Symbol::from_str(x).unwrap()
    }).collect();
    let matcher = BasicMatcher{};
    let (exec_tx, exec_rx) = mpsc::channel(1024 as usize);
    let handler = FeedExecutionHandler{ tx: exec_tx.clone() };
    let engine = EngineHandle::new(symbols, matcher, handler).unwrap();
    let router = SingleRouter::new(engine.tx.clone());

    let context = ServerContext::new(handle.clone(), router);
    let publisher = ExecutionPublisher::new(exec_rx, context.clone());
    publisher.handle_executions();

    let addr = "localhost:2468".to_socket_addrs().unwrap().next()
        .expect("could not parse address");
    let socket = TcpListener::bind(&addr, &handle).unwrap();

    let done = socket.incoming().for_each(move |(s, _)| {
        let (reader, writer) = s.split();
        let network = capnp_rpc::twoparty::VatNetwork::new(reader, writer,
            capnp_rpc::rpc_twoparty_capnp::Side::Server, Default::default());

        let sess = cp::trading_session::ToClient::new(session::Session::new(context.clone()))
            .from_server::<capnp_rpc::Server>();
        let rpc_system = capnp_rpc::RpcSystem::new(Box::new(network), Some(sess.client));

        handle.spawn(rpc_system.map_err(|_| ()));
        Ok(())
    });

    core.run(done).unwrap();
}
