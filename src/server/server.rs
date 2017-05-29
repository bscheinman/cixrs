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
mod messages;
mod session;

use engine::EngineHandle;
use futures::{future, Future, Stream};
use futures::sink::Sink;
use futures::sync::mpsc;
use libcix::book::{BasicMatcher, ExecutionHandler};
use libcix::cix_capnp as cp;
use libcix::order::trade_types;
use messages::{EngineMessage, SessionMessage};
use session::{OrderRouter, OrderRoutingInfo, ServerContext};
use std::cell::Cell;
use std::collections::HashMap;
use std::iter::repeat;
use std::net::ToSocketAddrs;
use std::rc::Rc;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpListener;

#[derive(Clone)]
struct FeedExecutionHandler {
    tx: mpsc::Sender<SessionMessage>
}

impl ExecutionHandler for FeedExecutionHandler {
    fn ack_order(&self, order_id: trade_types::OrderId,
                 status: trade_types::ErrorCode) {
        println!("ACK {}: {:?}", order_id, status);
        self.tx.clone().send(SessionMessage::NewOrderAck {
            order_id: order_id,
            status: status
        }).wait();
    }

    fn handle_match(&self, execution: trade_types::Execution) {
        println!("EXECUTION {}", execution);
        self.tx.clone().send(SessionMessage::Execution(execution)).wait();
    }

    fn handle_market_data_l1(&self, symbol: trade_types::Symbol,
                             bid: trade_types::MdEntry,
                             ask: trade_types::MdEntry) {
        println!("{} bid {}x{}, ask {}x{}", symbol, bid.price, bid.quantity,
                 ask.price, ask.quantity);
    }

    fn handle_market_data_l2(&self, symbol: trade_types::Symbol,
                             bids: Vec<trade_types::MdEntry>,
                             asks: Vec<trade_types::MdEntry>) {
        println!("Bids:");
        if bids.len() == 0 {
            println!("None");
        } else {
            for entry in bids {
                println!("\t{}x{}", entry.price, entry.quantity);
            }
        }

        println!("Asks:");
        if asks.len() == 0 {
            println!("None");
        } else {
            for entry in asks {
                println!("\t{}x{}", entry.price, entry.quantity);
            }
        }
    }
}

type SymbolId = usize;

struct SymbolLookup {
    symbols: Vec<trade_types::Symbol>,
    lookup: HashMap<trade_types::Symbol, SymbolId>
}

impl SymbolLookup {
    pub fn new(symbols: &Vec<trade_types::Symbol>) -> Result<Self, String> {
        let mut res = SymbolLookup {
            symbols: symbols.to_vec(),
            lookup: HashMap::new()
        };

        for (i, symbol) in symbols.iter().enumerate() {
            if let Some(_) = res.lookup.insert(symbol.clone(), i) {
                return Err(format!("duplicate symbol {}", symbol));
            }
        }

        Ok(res)
    }

    pub fn get_symbol(&self, id: SymbolId) -> Result<trade_types::Symbol, ()> {
        if id >= self.symbols.len() {
            Err(())
        } else {
            Ok(self.symbols[id])
        }
    }

    pub fn get_symbol_id(&self, symbol: &trade_types::Symbol) -> Result<SymbolId, ()> {
        match self.lookup.get(symbol) {
            Some(s) => Ok(*s),
            None => Err(())
        }
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }
}

// XXX: For now just use a single engine for all symbols
// Later on we can either shard by symbol or use a lookup or whatever
#[derive(Clone)]
struct SingleRouter {
    symbols: Rc<SymbolLookup>,
    tx: mpsc::Sender<EngineMessage>,
    seq_list: Vec<Cell<u64>>
}

impl SingleRouter {
    pub fn new(symbols: Rc<SymbolLookup>, tx: mpsc::Sender<EngineMessage>) -> Self {
        let len = symbols.len();
        SingleRouter {
            symbols: symbols,
            tx: tx,
            seq_list: repeat(Cell::new(0u64)).take(len).collect()
        }
    }
}

impl OrderRouter for SingleRouter {
    fn route_order(&self, o: &OrderRoutingInfo, msg: EngineMessage) -> Result<(), String> {
        self.tx.clone().send(msg).wait();
        Ok(())
    }

    fn create_order_id(&self, o: &OrderRoutingInfo) -> Result<trade_types::OrderId, String> {
        if let OrderRoutingInfo::NewOrderInfo { symbol: ref symbol, side: side } = *o {
            let sym_id = try!(self.symbols.get_symbol_id(symbol).map_err(|_| {
                format!("invalid symbol {}", symbol)
            }));
            let ref seq = self.seq_list[sym_id];
            let order_id = try!(trade_types::OrderId::new(sym_id as u32, side, seq.get()));

            // This is only accessed from the main thread so non-atomic updates like this are fine
            seq.set(seq.get() + 1);
            Ok(order_id)
        } else {
            unreachable!()
        }
    }
}

struct ExecutionPublisher<R> where R: 'static + Clone + OrderRouter {
    rx: mpsc::Receiver<SessionMessage>,
    context: ServerContext<R>
}

impl<R> ExecutionPublisher<R> where R: 'static + Clone + OrderRouter {
    fn new(rx: mpsc::Receiver<SessionMessage>, context: ServerContext<R>) -> Self {
        ExecutionPublisher {
            rx: rx,
            context: context
        }
    }

    fn handle_executions(self) {
        let context = self.context.clone();
        let exec_feed = self.rx.for_each(move |message| {
            match message {
                SessionMessage::Execution(execution) => {
                    Self::handle_execution_side(&context, &execution,
                                                trade_types::OrderSide::Buy);
                    Self::handle_execution_side(&context, &execution,
                                                trade_types::OrderSide::Sell);
                },
                SessionMessage::NewOrderAck{order_id, status} => {
                    let order_map = context.pending_orders.borrow_mut();
                    if let Some(waiter) = order_map.get(&order_id) {
                        waiter.ack(status);
                    } else {
                        println!("received ack for unknown order {}", order_id);
                    }
                }
            };

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
            builder.set_side(match side {
                trade_types::OrderSide::Buy => cp::OrderSide::Buy,
                trade_types::OrderSide::Sell => cp::OrderSide::Sell
            });
            builder.set_symbol(execution.symbol.as_str());
            builder.set_price(execution.price);
            builder.set_quantity(execution.quantity);
            builder.set_id(execution.id.raw());
            builder.set_order(order.raw());

            {
                let mut ts_builder = try!(builder.borrow().get_ts().map_err(|_| ()));
                ts_builder.set_seconds(execution.ts.sec);
                ts_builder.set_nanos(execution.ts.nsec);
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

    let symbols = vec!["AAPL", "FB", "GOOG"].into_iter().map(|x| {
        trade_types::Symbol::from_str(x).unwrap()
    }).collect();
    let matcher = BasicMatcher{};
    let (exec_tx, exec_rx) = mpsc::channel(1024 as usize);
    let handler = FeedExecutionHandler{ tx: exec_tx.clone() };
    let engine = EngineHandle::new(&symbols, matcher, handler).unwrap();

    let sym_context = Rc::new(SymbolLookup::new(&symbols).unwrap());
    let router = SingleRouter::new(sym_context, engine.tx.clone());
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
