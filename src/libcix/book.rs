use heap;
use order::trade_types::*;
use std::cell::Cell;
use std::cmp::{min, Ordering};
use std::collections::HashMap;
use std::fmt::Debug;
use std::rc::Rc;
use time;

trait OrderComparer: heap::Comparer<Order> {
    fn does_cross(new_order: &Order, book_order: &Order) -> bool;
    fn create_execution(id: ExecutionId, new_order: &Order, book_order: &Order, quantity: Quantity)
        -> Execution;
}

#[derive(Debug)]
struct BuyComparer;

#[derive(Debug)]
struct SellComparer;

impl OrderComparer for BuyComparer {
    fn does_cross(new_order: &Order, book_order: &Order) -> bool {
        book_order.price >= new_order.price
    }

    fn create_execution(id: ExecutionId, new_order: &Order, book_order: &Order, quantity: Quantity)
            -> Execution {
        Execution {
            symbol:     book_order.symbol,
            ts:         time::now().to_timespec(),
            id:         id, 
            buy_user:   book_order.user,
            buy_order:  book_order.id,
            sell_user:  new_order.user,
            sell_order: new_order.id,
            price:      book_order.price,
            quantity:   quantity
        }
    }
}

impl heap::Comparer<Order> for BuyComparer {
    fn compare(x: &Order, y: &Order) -> Ordering {
        match x.price.partial_cmp(&y.price).unwrap_or(Ordering::Equal) {
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
            Ordering::Equal => {
                match x.update.cmp(&y.update) {
                    Ordering::Greater => Ordering::Less,
                    Ordering::Less => Ordering::Greater,
                    Ordering::Equal => Ordering::Equal
                }
            }
        }
    }
}

impl OrderComparer for SellComparer {
    fn does_cross(new_order: &Order, book_order: &Order) -> bool {
        book_order.price <= new_order.price
    }

    fn create_execution(id: ExecutionId, new_order: &Order, book_order: &Order, quantity: Quantity)
            -> Execution {
        Execution {
            symbol:     book_order.symbol,
            ts:         time::now().to_timespec(),
            id:         id,
            buy_user:   new_order.user,
            buy_order:  new_order.id,
            sell_user:  book_order.user,
            sell_order: book_order.id,
            price:      book_order.price,
            quantity:   quantity
        }
    }
}

impl heap::Comparer<Order> for SellComparer {
    fn compare(x: &Order, y: &Order) -> Ordering {
        match x.price.partial_cmp(&y.price).unwrap_or(Ordering::Equal) {
            Ordering::Greater => Ordering::Less,
            Ordering::Less => Ordering::Greater,
            Ordering::Equal => {
                match x.update.cmp(&y.update) {
                    Ordering::Greater => Ordering::Less,
                    Ordering::Less => Ordering::Greater,
                    Ordering::Equal => Ordering::Equal
                }
            }
        }
    }
}

trait OrderProcessor<THandle> {
    fn add_order(&mut self, new_order: Order) -> THandle;
    fn match_order(&mut self, new_order: &mut Order,
                   handler: &ExecutionHandler);
}

struct BookSide<TCmp> where TCmp: OrderComparer {
    orders: heap::TreeHeap<Order, TCmp>,
    lookup: HashMap<OrderId, heap::HeapHandle>,
    id_gen: Rc<ExecutionIdGenerator>
}

pub trait ExecutionHandler: Send {
    fn handle_match(&self, execution: Execution);
    fn handle_market_data_l1(&self, symbol: Symbol, bid: MdEntry, ask: MdEntry);
    //fn handle_market_data_l2(&self, symbol: Symbol, bids: Vec<MdEntry>,
                             //asks: Vec<MdEntry>);
}

impl<TCmp> BookSide<TCmp> where TCmp: OrderComparer {
    fn new(id_gen: Rc<ExecutionIdGenerator>) -> BookSide<TCmp> {
        BookSide {
            orders: heap::TreeHeap::new(1024),
            lookup: HashMap::new(),
            id_gen: id_gen
        }
    }

    fn get_order(&self, order: OrderId) -> Option<&Order> {
        self.lookup.get(&order).map(|h| self.orders.get(h.clone()))
    }

    fn remove_order(&mut self, order: OrderId) {
        if let Some(h) = self.lookup.remove(&order) {
            self.orders.remove(h);
        }
    }

    fn top_order(&self) -> MdEntry {
        match self.orders.peek() {
            None => MdEntry { price: 0.0f64, quantity: 0u32 },
            Some(h) => {
                let order = self.orders.get(h);
                MdEntry { price: order.price, quantity: order.quantity }
            }
        }
    }
}

impl<TCmp> OrderProcessor<heap::HeapHandle> for BookSide<TCmp>
        where TCmp: Debug + OrderComparer {
    fn add_order(&mut self, new_order: Order) -> heap::HeapHandle {
        let order_id = new_order.id;
        let handle = self.orders.insert(new_order).unwrap();

        self.lookup.insert(order_id, handle.clone());

        handle
    }

    fn match_order(&mut self, new_order: &mut Order,
                 handler: &ExecutionHandler) {
        while let Some(handle) = self.orders.peek() {
            let ex = {
                let book_order = self.orders.get(handle);

                if !TCmp::does_cross(&new_order, book_order) {
                    break;
                }

                let cross_quantity = min(new_order.quantity,
                                         book_order.quantity);

                if cross_quantity == 0 {
                    println!("{}", self.orders);
                }

                assert_ne!(cross_quantity, 0);

                let exec_id = self.id_gen.next_id();
                TCmp::create_execution(exec_id, &new_order, book_order, cross_quantity)
            };
            let quantity = ex.quantity;

            handler.handle_match(ex);
            new_order.quantity -= quantity;

            self.orders.update(handle, |order| {
                order.quantity -= quantity;
            });

            let (rem_quantity, match_id) = {
                let book_order = self.orders.get(handle);
                (book_order.quantity, book_order.id)
            };

            if rem_quantity == 0 {
                self.remove_order(match_id);
            }

            if new_order.quantity == 0 {
                break;
            }
        }
    }
}

pub struct ExecutionIdGenerator {
    symbol_id: u32,
    seq: Cell<u64>
}

impl ExecutionIdGenerator {
    pub fn new(symbol_id: u32) -> Self {
        ExecutionIdGenerator {
            symbol_id: symbol_id,
            seq: Cell::new(0u64)
        }
    }

    pub fn next_id(&self) -> ExecutionId {
        let id = ExecutionId::new(self.symbol_id, self.seq.get()).unwrap();
        self.seq.set(self.seq.get() + 1);
        id
    }
}

pub struct OrderBook {
    symbol:     Symbol,
    buys:       BookSide<BuyComparer>,
    sells:      BookSide<SellComparer>
}

impl OrderBook {
    pub fn new(symbol: Symbol, symbol_id: u32) -> OrderBook {
        let id_gen = Rc::new(ExecutionIdGenerator::new(symbol_id));
        OrderBook {
            symbol:     symbol,
            buys:       BookSide::<BuyComparer>::new(id_gen.clone()),
            sells:      BookSide::<SellComparer>::new(id_gen.clone())
        }
    }

    pub fn print_books(&self) {
        println!("{}", self.buys.orders);
        println!("{}", self.sells.orders);
    }

    pub fn get_order(&self, order: OrderId) -> Option<&Order> {
        match order.side() {
            OrderSide::Buy => self.buys.get_order(order),
            OrderSide::Sell => self.sells.get_order(order)
        }
    }
}

pub trait OrderMatcher: Send {
    fn add_order<T: ExecutionHandler>(&mut self, book: &mut OrderBook, order: Order, handler: &T);
    fn cancel_order<T: ExecutionHandler>(&mut self, &mut OrderBook,
                                         order: OrderId, handler: &T);
}

#[derive(Clone)]
pub struct BasicMatcher;

impl BasicMatcher {
    fn publish_md<T: ExecutionHandler>(&self, book: &OrderBook, handler: &T) {
        let top_bid = book.buys.top_order();
        let top_ask = book.sells.top_order();
        handler.handle_market_data_l1(book.symbol, top_bid, top_ask);
    }
}

impl OrderMatcher for BasicMatcher {
    fn add_order<T: ExecutionHandler>(&mut self, book: &mut OrderBook,
                                      order: Order, handler: &T) {
        let mut o = order;

        {
            let counter_book: &mut OrderProcessor<heap::HeapHandle> =
                    match order.side {
                OrderSide::Buy  => &mut book.sells,
                OrderSide::Sell => &mut book.buys
            };

            counter_book.match_order(&mut o, handler);
        }

        if o.quantity > 0 {
            let book: &mut OrderProcessor<heap::HeapHandle> = match order.side {
                OrderSide::Buy  => &mut book.buys,
                OrderSide::Sell => &mut book.sells
            };

            book.add_order(o);
        }

        self.publish_md(book, handler);
    }

    fn cancel_order<T: ExecutionHandler>(&mut self, book: &mut OrderBook,
                                         order: OrderId, handler: &T) {
        match order.side() {
            OrderSide::Buy => book.buys.remove_order(order),
            OrderSide::Sell => book.sells.remove_order(order)
        }

        self.publish_md(book, handler);
    }
}
