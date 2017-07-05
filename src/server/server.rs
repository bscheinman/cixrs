extern crate bincode;
extern crate capnp;
#[macro_use]
extern crate capnp_rpc;
extern crate futures;
extern crate futures_cpupool;
extern crate libcix;
extern crate memmap;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate time;
extern crate tokio_core;
extern crate uuid;

mod engine;
mod events;
mod md;
mod messages;
mod session;
mod wal;

use engine::EngineHandle;
use futures::{future, Future, Stream};
use futures::sink::Sink;
use futures::sync::mpsc;
use libcix::book::{BasicMatcher, ExecutionHandler};
use libcix::cix_capnp as cp;
use libcix::order::trade_types;
use md::MdPublisherHandle;
use messages::{EngineMessage, MdMessage, SessionMessage};
use session::{OrderRouter, ServerContext, ServerState};
use std::cell::Cell;
use std::collections::HashMap;
use std::env::current_dir;
use std::error::Error;
use std::iter::repeat;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::rc::Rc;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpListener;
use wal::{Wal, WalDirectoryReader};

#[derive(Clone)]
struct FeedExecutionHandler {
    session_tx: mpsc::Sender<SessionMessage>,
    md_tx:      mpsc::Sender<MdMessage>
}

impl ExecutionHandler for FeedExecutionHandler {
    fn ack_order(&self, order_id: trade_types::OrderId,
                 status: trade_types::ErrorCode) {
        self.session_tx.clone().send(SessionMessage::NewOrderAck {
            order_id: order_id,
            status: status
        }).wait();
    }

    fn handle_match(&self, execution: &trade_types::Execution) {
        let md_execution = trade_types::MdExecution::from(execution.clone());
        let exec_id = execution.id;

        self.session_tx.clone().send(SessionMessage::Execution(*execution)).map_err(|e| {
                format!("failed to notify client of execution {}", exec_id).to_string()
            })
            .join(self.md_tx.clone().send(MdMessage::Execution(md_execution)).map_err(|e| {
                format!("failed to publish market datafor execution {}", exec_id).to_string()
            }))
            .wait();
    }

    fn handle_market_data_l1(&self, md: trade_types::L1Md) {
        self.md_tx.clone().send(MdMessage::L1Message(md)).wait();
    }

    fn handle_market_data_l2(&self, md: trade_types::L2Md) {
        self.md_tx.clone().send(MdMessage::L2Message(md)).wait();
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
    fn route_order(&self, msg: EngineMessage) -> Result<(), String> {
        self.broadcast_message(msg)
    }

    fn broadcast_message(&self, msg: EngineMessage) -> Result<(), String> {
        self.tx.clone().send(msg).wait().map(|_| ()).map_err(|e| e.description().to_string())
    }

    fn create_order_id(&self, symbol: &trade_types::Symbol, side: &trade_types::OrderSide)
            -> Result<trade_types::OrderId, String> {
        let sym_id = try!(self.symbols.get_symbol_id(symbol).map_err(|_| {
            format!("invalid symbol {}", symbol)
        }));
        let ref seq = self.seq_list[sym_id];
        let order_id = try!(trade_types::OrderId::new(sym_id as u32, *side, seq.get()));

        // This is only accessed from the main thread so non-atomic updates like this are fine
        seq.set(seq.get() + 1);
        Ok(order_id)
    }

    fn replay_message(&self, msg: EngineMessage) -> Result<(), String> {
        if let EngineMessage::NewOrder(new_order) = msg {
            //println!("replaying order {}", new_order.order_id);

            let order_id = new_order.order_id.clone();
            let sym_id = try!(self.symbols.get_symbol_id(&new_order.symbol).map_err(|_| {
                format!("invalid symbol {}", new_order.symbol)
            }));

            let order_seq = order_id.sequence();
            let ref sym_seq = self.seq_list[sym_id];

            if order_seq >= sym_seq.get() {
                sym_seq.set(order_seq + 1);
            }
        }
        self.route_order(msg)
    }

    fn n_engine(&self) -> u32 {
        1u32
    }
}

struct ExecutionPublisher<R> where R: 'static + Clone + OrderRouter {
    rx: mpsc::Receiver<SessionMessage>,
    context: Rc<ServerContext<R>>
}

impl<R> ExecutionPublisher<R> where R: 'static + Clone + OrderRouter {
    fn new(rx: mpsc::Receiver<SessionMessage>, context: Rc<ServerContext<R>>) -> Self {
        ExecutionPublisher {
            rx: rx,
            context: context
        }
    }

    fn notify_serializations(context: &ServerContext<R>, gen: u32) {
        let mut syncs = context.pending_syncs.borrow_mut();

        {
            let waiter = if let Some(w) = syncs.get(&gen) {
                w
            } else {
                return;
            };

            waiter.pending_count.set(waiter.pending_count.get() - 1);
            if waiter.pending_count.get() > 0 {
                return;
            }

            assert!(gen == context.sync_gen.get() + 1);
            context.sync_gen.set(gen);
            waiter.event.ack(());
        }

        syncs.remove(&gen);

    }

    fn handle_executions(self) {
        let context = self.context.clone();
        let exec_feed = self.rx.for_each(move |message| {
            let running = if let ServerState::Running = context.state.get() {
                true
            } else {
                false
            };

            match message {
                SessionMessage::Execution(execution) => {
                    if running {
                        //println!("EXECUTION {}", execution);
                        Self::handle_execution_side(context.as_ref(), &execution,
                                                    trade_types::OrderSide::Buy);
                        Self::handle_execution_side(context.as_ref(), &execution,
                                                    trade_types::OrderSide::Sell);
                    }
                },
                SessionMessage::NewOrderAck{order_id, status} => {
                    if running {
                        //println!("ACK {}: {:?}", order_id, status);
                        let order_map = context.pending_orders.borrow_mut();
                        if let Some(waiter) = order_map.get(&order_id) {
                            waiter.ack(status);
                        } else {
                            println!("received ack for unknown order {}", order_id);
                        }
                    }
                },
                SessionMessage::SerializationResponse(gen) => {
                    Self::notify_serializations(context.as_ref(), gen);
                },
                SessionMessage::OpenOrdersResponse(orders) => {
                    let order_map = context.pending_open_orders.borrow_mut();
                    if let Some(waiter) = order_map.get(&orders.seq) {
                        waiter.borrow_mut().recv(&orders);
                    } else {
                        println!("received response for unknown open order request {}/{}",
                                 orders.seq.user, orders.seq.seq);
                    }
                }
            };

            future::ok(())
        });

        self.context.handle.spawn(exec_feed);
    }

    fn handle_execution_side(context: &ServerContext<R>,
                             execution: &trade_types::Execution,
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

fn init_wal<P: AsRef<Path>, R: OrderRouter>(dir: P, router: &R) -> Wal {
    let reader = WalDirectoryReader::new(dir.as_ref()).unwrap();
    let mut replay_count = 0usize;

    // Replay all messages from existing log files to catch books up
    for entry in reader {
        match entry {
            Ok(msg) => {
                router.replay_message(msg).unwrap();
                replay_count += 1;
            },
            Err(e) => {
                panic!("failed to replay messages: {}", e);
            }
        }
    }

    println!("replayed {} events", replay_count);

    Wal::new(dir, (10 * 1024 * 1024) as usize).unwrap()
}

fn main() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();

    let symbols = vec!["AAPL", "FB", "GOOG"].into_iter().map(|x| {
        trade_types::Symbol::from_str(x).unwrap()
    }).collect();
    let matcher = BasicMatcher{};
    let md_publisher = MdPublisherHandle::new();
    let (exec_tx, exec_rx) = mpsc::channel(1024 as usize);
    let handler = FeedExecutionHandler{
        session_tx: exec_tx.clone(),
        md_tx: md_publisher.tx
    };
    let engine = EngineHandle::new(&symbols, &matcher, &handler, &exec_tx).unwrap();
    let sym_context = Rc::new(SymbolLookup::new(&symbols).unwrap());
    let router = SingleRouter::new(sym_context, engine.tx.clone());

    let wal_dir = Path::new("/home/brendon/wal");
    let wal = init_wal(wal_dir, &router);

    let context = Rc::new(ServerContext::new(handle.clone(), router, wal));
    let publisher = ExecutionPublisher::new(exec_rx, context.clone());
    publisher.handle_executions();

    let addr = "localhost:2468".to_socket_addrs().unwrap().next()
        .expect("could not parse address");
    let socket = TcpListener::bind(&addr, &handle).unwrap();

    // Don't start listening for connections until replay is complete
    // This future has to be created lazily so that there is an active task to register when we
    // call serialization_point
    let replay_sync = future::lazy(|| ServerContext::serialization_point(context.clone()));

    let listen_context = context.clone();
    let listen = socket.incoming().for_each(move |(s, _)| {
        let (reader, writer) = s.split();
        let network = capnp_rpc::twoparty::VatNetwork::new(reader, writer,
            capnp_rpc::rpc_twoparty_capnp::Side::Server, Default::default());

        let sess = cp::trading_session::ToClient::new(session::Session::new(listen_context.clone()))
            .from_server::<capnp_rpc::Server>();
        let rpc_system = capnp_rpc::RpcSystem::new(Box::new(network), Some(sess.client));

        handle.spawn(rpc_system.map_err(|_| ()));
        Ok(())
    }).map_err(|_| ());

    let done = replay_sync.and_then(|_| {
        println!("order replay complete");
        context.state.set(ServerState::Running);
        future::ok(())
    }).and_then(|_| listen);

    core.run(done).unwrap();
}
