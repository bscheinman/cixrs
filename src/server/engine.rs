use futures;
use futures::future;
use futures::{Future, Stream};
use futures::sync::{mpsc, oneshot};
use libcix::book;
use libcix::cix_capnp as cp;
use libcix::order::trade_types::*;
use std::collections::HashMap;
use std::error::Error;
use std::thread;
use time;
use tokio_core::reactor;

const BUFFER_SIZE: usize = 1024;

struct OrderEngine<TMatcher, THandler>
        where TMatcher: book::OrderMatcher,
              THandler: book::ExecutionHandler {
    symbols:        Vec<Symbol>,
    books:          HashMap<Symbol, book::OrderBook>,
    matcher:        TMatcher,
    handler:        THandler
}

pub struct NewOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId,
    pub symbol:     Symbol,
    pub side:       OrderSide,
    pub price:      Price,
    pub quantity:   Quantity
}

pub struct ChangeOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId,
    pub price:      Price,
    pub quantity:   Quantity
}

pub struct CancelOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId
}

pub enum EngineMessage {
    NewOrder(NewOrderMessage),
    //ChangeOrder(ChangeOrderMessage),
    CancelOrder(CancelOrderMessage)
}

pub struct EngineHandle {
    // XXX: wrap this in a function EngineHandle::send to avoid exposing
    // implementation details
    pub tx: mpsc::Sender<EngineMessage>
}

impl EngineHandle {
    pub fn new<TMatcher, THandler> (symbols: &Vec<Symbol>, matcher: TMatcher,
                                    handler: THandler) -> Result<Self, String>
            where TMatcher: 'static + book::OrderMatcher + Clone,
                  THandler: 'static + book::ExecutionHandler + Clone {
        let (channel_tx, channel_rx) = oneshot::channel();
        let s_clone = symbols.clone();
        let m_clone = matcher.clone();
        let h_clone = handler.clone();

        thread::spawn(move || -> Result<(), String> {
            let mut engine = OrderEngine::new(s_clone, m_clone, h_clone)
                .unwrap_or_else(|e| {
                    panic!("failed to create order engine: {}", e)
                });
            let mut core = reactor::Core::new().unwrap();
            let (tx, rx) = mpsc::channel(BUFFER_SIZE);

            // hand sender back to the calling thread
            channel_tx.complete(tx);

            // process incoming messages on event loop
            let done = rx.for_each(|msg| {
                engine.process_message(msg).map_err(|e| {
                    println!("error processing message: {}", e);
                })
            });

            core.run(done);

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
    pub fn new(symbols: Vec<Symbol>, matcher: TMatcher, handler: THandler) ->
            Result<OrderEngine<TMatcher, THandler>, String> {
        let mut engine = OrderEngine {
            symbols: symbols,
            books: HashMap::new(),
            matcher: matcher,
            handler: handler
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
        let order = Order {
            id:         msg.order_id,
            user:       msg.user,
            symbol:     msg.symbol,
            side:       msg.side,
            price:      msg.price,
            quantity:   msg.quantity,
            update:     time::now().to_timespec()
        };

        let mut book = self.books.get_mut(&order.symbol).unwrap();
        self.matcher.add_order(&mut book, order, &self.handler);

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

        // XXX: really the books should be stored directly in a vector and the lookup hashmap
        // would point into that
        let mut book = self.books.get_mut(&self.symbols[sym_id as usize]).unwrap();
        let target_user = {
            let order = try!(book.get_order(msg.order_id)
                             .ok_or("nonexistent order id".to_string()));
            order.user
        };

        if target_user != msg.user {
            return Err(format!("order {} does not belong to user {}", msg.order_id, msg.user));
        }

        self.matcher.cancel_order(&mut book, msg.order_id, &self.handler);
        Ok(())
    }

    pub fn process_message(&mut self, message: EngineMessage) ->
            Result<(), String> {
        match message {
            EngineMessage::NewOrder(msg) => self.new_order(msg),
            //EngineMessage::ChangeOrder(msg) => self.change_order(msg),
            EngineMessage::CancelOrder(msg) => self.cancel_order(msg)
        }
    }
}
