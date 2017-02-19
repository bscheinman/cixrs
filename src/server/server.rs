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

use capnp_rpc as rpc;
use engine::{EngineHandle, EngineMessage};
use futures::{Future, Stream};
use futures::sink::Sink;
use futures::sync::mpsc;
use libcix::book::{BasicMatcher, ExecutionHandler};
use libcix::cix_capnp as cp;
use libcix::order::trade_types;
use session::{OrderRouter, OrderRoutingInfo};
use std::net::ToSocketAddrs;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpListener;

#[derive(Clone)]
struct ExecutionPrinter;

impl ExecutionHandler for ExecutionPrinter {
    fn handle_match(&self, execution: &trade_types::Execution) {
        println!("{}", execution)
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

fn main() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();
    
    let addr = "localhost:2468".to_socket_addrs().unwrap().next()
        .expect("could not parse address");
    let socket = TcpListener::bind(&addr, &handle).unwrap();

    let symbols = vec!["GOOG"].into_iter().map(|x| {
        trade_types::Symbol::from_str(x).unwrap()
    }).collect();
    let matcher = BasicMatcher{};
    let handler = ExecutionPrinter{};
    let engine = EngineHandle::new(symbols, matcher, handler).unwrap();
    let router = SingleRouter::new(engine.tx.clone());

    let done = socket.incoming().for_each(move |(s, _)| {
        let (reader, writer) = s.split();
        let network = rpc::twoparty::VatNetwork::new(reader, writer,
            rpc::rpc_twoparty_capnp::Side::Server, Default::default());

        let sess = cp::trading_session::ToClient::new(
            session::Session::new(handle.clone(), router.clone()))
            .from_server::<capnp_rpc::Server>();
        let rpc_system = rpc::RpcSystem::new(Box::new(network),
            Some(sess.client));

        handle.spawn(rpc_system.map_err(|_| ()));
        Ok(())
    });

    core.run(done).unwrap();
}
