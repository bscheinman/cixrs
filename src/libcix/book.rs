use std::cmp::{min, Ordering};
use std::collections::HashMap;

use order::trade_types::*;
use heap;
use uuid::Uuid;

trait OrderComparer: heap::Comparer<Order> {
    fn does_cross(new_order: &Order, book_order: &Order) -> bool;
    fn create_execution(new_order: &Order, book_order: &Order,
                        quantity: Quantity) -> Execution;
}

struct BuyComparer;
struct SellComparer;

impl OrderComparer for BuyComparer {
    fn does_cross(new_order: &Order, book_order: &Order) -> bool {
        book_order.price >= new_order.price
    }

    fn create_execution(new_order: &Order, book_order: &Order,
                        quantity: Quantity) -> Execution {
        Execution {
            symbol:     book_order.symbol,
            id:         Uuid::new_v4(), 
            buy_order:  book_order.id,
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

    fn create_execution(new_order: &Order, book_order: &Order,
                        quantity: Quantity) -> Execution {
        Execution {
            symbol:     book_order.symbol,
            id:         Uuid::new_v4(), 
            buy_order:  new_order.id,
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
    fn add_order(&mut self, new_order: &Order) -> THandle;
    fn match_order(&mut self, new_order: &mut Order,
                   handler: &ExecutionHandler);
}

struct BookSide<TCmp> where TCmp: OrderComparer {
    orders: heap::TreeHeap<Order, TCmp>
}

pub trait ExecutionHandler {
    fn handle_match(&self, execution: &Execution);
}

impl<TCmp> BookSide<TCmp> where TCmp: OrderComparer {
    fn new() -> BookSide<TCmp> {
        BookSide {
            orders: heap::TreeHeap::new(1024)
        }
    }
}

impl<TCmp> OrderProcessor<heap::HeapHandle> for BookSide<TCmp>
        where TCmp: OrderComparer {
    fn add_order(&mut self, new_order: &Order) -> heap::HeapHandle {
        let handle = self.orders.insert(new_order).unwrap();

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

                assert_ne!(cross_quantity, 0);

                TCmp::create_execution(&new_order, book_order, cross_quantity)
            };

            handler.handle_match(&ex);
            new_order.quantity -= ex.quantity;

            self.orders.update(handle, |order| {
                order.quantity -= ex.quantity;
            });

            if self.orders.get(handle).quantity == 0 {
                self.orders.remove(handle);
            }

            if new_order.quantity == 0 {
                break;
            }
        }
    }
}

pub struct OrderBook {
    symbol:     Symbol,
    buys:       BookSide<BuyComparer>,
    sells:      BookSide<SellComparer>,
    all_orders: HashMap<OrderId, heap::HeapHandle>
}

impl OrderBook {
    pub fn new(symbol: Symbol) -> OrderBook {
        OrderBook {
            symbol:     symbol,
            buys:       BookSide::<BuyComparer>::new(),
            sells:      BookSide::<SellComparer>::new(),
            all_orders: HashMap::new()
        }
    }
}

pub trait OrderMatcher {
    fn add_order<T: ExecutionHandler>(&mut self, book: &mut OrderBook,
                                      order: &mut Order, handler: &T);
}

pub struct BasicMatcher;

impl OrderMatcher for BasicMatcher {
    fn add_order<T: ExecutionHandler>(&mut self, book: &mut OrderBook,
                                      order: &mut Order, handler: &T) {
        {
            let counter_book: &mut OrderProcessor<heap::HeapHandle> =
                    match order.side {
                OrderSide::Buy  => &mut book.sells,
                OrderSide::Sell => &mut book.buys
            };

            counter_book.match_order(order, handler);
        }

        if order.quantity > 0 {
            let book: &mut OrderProcessor<heap::HeapHandle> = match order.side {
                OrderSide::Buy  => &mut book.buys,
                OrderSide::Sell => &mut book.sells
            };

            book.add_order(order);
        }
    }
}
