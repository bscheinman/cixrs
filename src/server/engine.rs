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
    books: HashMap<Symbol, book::OrderBook>,
    matcher: TMatcher,
    handler: THandler
}

pub struct NewOrderMessage {
    pub user:       UserId,
    pub order_id:   OrderId,
    pub symbol:     Symbol,
    pub side:       OrderSide,
    pub price:      Price,
    pub quantity:   Quantity
}

pub enum EngineMessage {
    NewOrder(NewOrderMessage),
}

pub struct EngineHandle {
    // XXX: wrap this in a function EngineHandle::send to avoid exposing
    // implementation details
    pub tx: mpsc::Sender<EngineMessage>
}

impl EngineHandle {
    pub fn new<TMatcher, THandler> (symbols: Vec<Symbol>, matcher: TMatcher,
                                    handler: THandler) -> Result<Self, String>
            where TMatcher: 'static + book::OrderMatcher + Clone,
                  THandler: 'static + book::ExecutionHandler + Clone {
        let (channel_tx, channel_rx) = oneshot::channel();
        let m_clone = matcher.clone();
        let h_clone = handler.clone();

        thread::spawn(move || -> Result<(), String> {
            let mut engine = try!(OrderEngine::new(symbols, m_clone, h_clone));
            let mut core = reactor::Core::new().unwrap();
            let (tx, rx) = mpsc::channel(BUFFER_SIZE);

            // hand sender back to the calling thread
            channel_tx.complete(tx);

            // process incoming messages on event loop
            let done = rx.for_each(|msg| {
                engine.process_message(msg).map_err(|_| ())
            });

            core.run(done).unwrap();
            Ok(())
        });

        Ok(EngineHandle {
            tx: channel_rx.wait().unwrap()
        })
    }
}

impl<TMatcher, THandler> OrderEngine<TMatcher, THandler>
        where TMatcher: book::OrderMatcher,
              THandler: book::ExecutionHandler {
    pub fn new(symbols: Vec<Symbol>, matcher: TMatcher, handler: THandler) ->
            Result<OrderEngine<TMatcher, THandler>, String> {
        let mut engine = OrderEngine {
            books: HashMap::new(),
            matcher: matcher,
            handler: handler
        };

        for symbol in symbols {
            if let None =
                    engine.books.insert(symbol, book::OrderBook::new(symbol)) {
                return Err("duplicate symbol".to_string());
            }
        }

        Ok(engine)
    }

    // XXX: send back result to calling thread
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

    pub fn process_message(&mut self, message: EngineMessage) ->
            Result<(), String> {
        match message {
            EngineMessage::NewOrder(msg) => {
               self.new_order(msg)
            },
        }
    }
}
