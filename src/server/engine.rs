use futures;
use futures::future;
use futures::{Future, Sink, Stream};
use futures::stream::MergedItem;
use futures::sync::{mpsc, oneshot};
use libcix::book;
use libcix::cix_capnp as cp;
use libcix::order::trade_types::*;
use messages::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::rc::Rc;
use std::thread;
use std::time::Duration;
use time;
use tokio_core::reactor;

const BUFFER_SIZE: usize = 1024;

struct OrderEngine<TMatcher, THandler>
        where TMatcher: book::OrderMatcher,
              THandler: book::ExecutionHandler {
    symbols:        Vec<Symbol>,
    dirty_symbols:  HashSet<Symbol>,
    books:          HashMap<Symbol, book::OrderBook>,
    matcher:        TMatcher,
    handler:        THandler,
    responder:      mpsc::Sender<SessionMessage>
}

pub struct EngineHandle {
    // XXX: wrap this in a function EngineHandle::send to avoid exposing
    // implementation details
    pub tx: mpsc::Sender<EngineMessage>
}

impl EngineHandle {
    pub fn new<TMatcher, THandler> (symbols: &Vec<Symbol>, matcher: &TMatcher,
                                    handler: &THandler,
                                    responder: &mpsc::Sender<SessionMessage>) -> Result<Self, String>
            where TMatcher: 'static + book::OrderMatcher + Clone,
                  THandler: 'static + book::ExecutionHandler + Clone {
        let (channel_tx, channel_rx) = oneshot::channel();
        let s_clone = symbols.clone();
        let m_clone = matcher.clone();
        let h_clone = handler.clone();
        let r_clone = responder.clone();

        thread::spawn(move || -> Result<(), String> {
            let mut engine = OrderEngine::new(s_clone, m_clone, h_clone, r_clone)
                .unwrap_or_else(|e| {
                    panic!("failed to create order engine: {}", e)
                });
            let mut core = reactor::Core::new().unwrap();
            let handle = core.handle();
            let (tx, rx) = mpsc::channel(BUFFER_SIZE);

            // hand sender back to the calling thread
            channel_tx.complete(tx);

            // XXX: Make md frequency configurable
            let md_frequency = Duration::new(1, 0);
            let md_loop = reactor::Interval::new(md_frequency, &handle).unwrap().map_err(|e| {
                panic!("market data timer error: {}", e.description());
            });

            let full_loop = rx.merge(md_loop).for_each(move |item| {
                match item {
                    MergedItem::First(msg) => {
                        engine.process_message(msg);
                    },
                    MergedItem::Second(_) => {
                        engine.publish_md();
                    },
                    MergedItem::Both(msg, _) => {
                        engine.process_message(msg);
                        engine.publish_md();
                    }
                }

                future::ok(())
            });

            core.run(full_loop);
            Ok(())
        });

        Ok(EngineHandle {
            tx: channel_rx.wait().unwrap_or_else(|e| {
                panic!("failed to get channel handle: {}", e)
            })
        })
    }
}

impl<TMatcher, THandler> OrderEngine<TMatcher, THandler>
        where TMatcher: book::OrderMatcher,
              THandler: book::ExecutionHandler {
    pub fn new(symbols: Vec<Symbol>, matcher: TMatcher, handler: THandler,
               responder: mpsc::Sender<SessionMessage>) ->
            Result<OrderEngine<TMatcher, THandler>, String> {
        let mut engine = OrderEngine {
            symbols: symbols,
            dirty_symbols: HashSet::new(),
            books: HashMap::new(),
            matcher: matcher,
            handler: handler,
            responder: responder
        };

        // XXX: This is fine for now because we're only using one engine, but once we start
        // sharding symbols across engines, we won't be able to rely on the assumption that symbol
        // ids are sequential and zero-indexed.  The `symbols` argument here should then change to
        // a vector of (symbol, id) tuples
        for (i, symbol) in engine.symbols.iter().enumerate() {
            if let Some(_) = engine.books.insert(symbol.clone(),
                                 book::OrderBook::new(symbol.clone(), i as u32)) {
                return Err(format!("duplicate symbol {}", symbol.as_str()));
            }
        }

        Ok(engine)
    }

    fn new_order(&mut self, msg: NewOrderMessage) -> Result<(), String> {
        let symbol = msg.symbol;

        let order = Order {
            id:         msg.order_id,
            user:       msg.user,
            symbol:     symbol.clone(),
            side:       msg.side,
            price:      msg.price,
            quantity:   msg.quantity,
            update:     time::now().to_timespec()
        };

        {
            let mut book = self.books.get_mut(&symbol).unwrap();
            self.matcher.add_order(&mut book, order, &self.handler);
        }

        self.symbol_dirty(symbol);
        Ok(())
    }

    /*
    fn change_order(&mut self, msg: ChangeOrderMessage) -> Result<(), String> {

    }
    */

    fn cancel_order(&mut self, msg: CancelOrderMessage) -> Result<(), String> {
        let sym_id = msg.order_id.symbol_id();
        if (sym_id as usize) >= self.symbols.len() {
            return Err("invalid order id".to_string());
        }

        let symbol = self.symbols[sym_id as usize];

        {
            // XXX: really the books should be stored directly in a vector and the lookup hashmap
            // would point into that
            let mut book = self.books.get_mut(&symbol).unwrap();
            let target_user = {
                match book.get_order(msg.order_id) {
                    Some(order) => {
                        order.user
                    },
                    None => {
                        println!("Received cancel for unknown order {}", msg.order_id);
                        return Ok(());
                    }
                }
            };

            if target_user != msg.user {
                return Err(format!("order {} does not belong to user {}", msg.order_id, msg.user));
            }

            self.matcher.cancel_order(&mut book, msg.order_id, &self.handler);
        }

        self.symbol_dirty(symbol);
        Ok(())
    }

    fn serialization_point(&mut self, seq: u32) -> Result<(), String> {
        // If we process messages asynchronously then this will have to track which have been
        // processed but right now because we handle them synchronously we can already be sure that
        // we're caught up.
        self.responder.clone().send(SessionMessage::SerializationResponse(seq)).wait()
            .map(|_| ())
            .map_err(|e| {
                "failed to send serialization response".to_string()
            }
        )
    }

    fn get_open_orders(&mut self, seq: OpenOrdersSequence) -> Result<(), String> {
        let mut user_orders = self.books.values().flat_map(|book| {
            println!("reading orders for user {} from book {}", seq.user, book.symbol);
            book.orders()
        }).filter(|ref o| {
            o.user == seq.user
        });

        loop {
            let mut response = OpenOrders::new(seq.clone());

            for i in 0 .. OPEN_ORDER_MSG_MAX_LENGTH {
                match user_orders.next() {
                    Some(order) => {
                        response.orders[i] = order;
                        response.n_order += 1;
                    },
                    None => {
                        response.last_response = true;
                        break;
                    }
                };
            }

            let last_response = response.last_response;

            // XXX: probably don't need to wait on each of these sequentially
            // Instead we can combine them into a single future and make sure they all copmlete at
            // the end.  The channel should still guarantee delivery in the order that we attempt
            // to send them.
            try!(self.responder.clone().send(SessionMessage::OpenOrdersResponse(response))
                 .wait()
                .map(|_| ())
                .map_err(|e| {
                    format!("failed to send open orders response to {}/{}",
                            seq.user, seq.seq).to_string()
                }));

            if last_response {
                break;
            }
        }

        Ok(())
    }

    pub fn process_message(&mut self, message: EngineMessage) ->
            Result<(), String> {
        match message {
            EngineMessage::NewOrder(msg) => self.new_order(msg),
            //EngineMessage::ChangeOrder(msg) => self.change_order(msg),
            EngineMessage::CancelOrder(msg) => self.cancel_order(msg),
            EngineMessage::SerializationMessage(seq) => self.serialization_point(seq),
            EngineMessage::GetOpenOrdersMessaage(seq) => self.get_open_orders(seq),
            EngineMessage::NullMessage => unreachable!()
        }
    }

    fn symbol_dirty(&mut self, symbol: Symbol) {
        self.dirty_symbols.insert(symbol);
    }

    fn publish_md(&mut self) {
        for symbol in self.dirty_symbols.iter() {
            self.matcher.publish_md(self.books.get(symbol).unwrap(), &self.handler);
        }

        self.dirty_symbols.clear();
    }
}
