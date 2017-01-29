use std::cmp::min;
use std::collections::HashMap;
use std::core::Option;
use cix;
use Uuid;

mod cixsrv {
    trait OrderComparer {
        fn does_cross(&self, new_order: &Order, book_order: &Order) -> bool;
        fn create_execution(&self, new_order: &Order, book_order: &Order) ->
            Execution;
    }

    struct BuyComparer {}
    struct SellComparer {}

    impl OrderComparer for BuyComparer {
        fn does_cross(&self, new_order: &Order, book_order: &Order) {
            book_order.price >= new_order.price
        }

        fn create_execution(&self, new_order: &Order, book_order: &Order,
                            quantity: Quantity) {
            Execution {
                id:         Uuid::new_v4(), 
                buy_order:  book_order.id,
                sell_order: new_order.id,
                price:      book_order.price,
                quantity:   quantity
            }
        }
    }

    impl OrderComparer for SellComparer {
        fn does_cross(&self, first: &Order, second: &Order) {
            book_order.price < new_order.price
        }

        fn create_execution(&self, new_order: &Order, book_order: &Order,
                            quantity: Quantity) {
            Execution {
                id:         Uuid::new_v4(), 
                buy_order:  new_order.id,
                sell_order: book_order.id,
                price:      book_order.price,
                quantity:   quantity
            }
        }
    }

    struct BookSide {
        orders: BinaryHeap<Cell<Order>>,
        crosser: OrderComparer
    }

    pub trait ExecutionHandler {
        fn handle_match(&self, execution: &Execution);
    }

    struct ExecutionPrinter{}

    impl ExecutionHandler for ExecutionPrinter {
        fn handle_match(&self, execution: &Execution) {
            println!("{}", execution)
        }
    }

    impl BookSide {
        fn new(crosser: OrderComparer) -> BookSide {
            BookSide { orders: BinaryHeap::new(), crosser: crosser }
        }

        fn match_order(&mut self, side: new_order: &mut Order,
                     handler: &ExecutionHandler) {
            while let Some(book_order) = self.orders.peek() {
                if self.crosser.cross(new_order, book_order) ==
                        OrderSide::Greater {
                    break;
                }

                let cross_quantity = min(new_order.quantity,
                                         book_order.quantity);
                assert_ne!(cross_quantity, 0);

                let ex = self.crosser.create_execution(new_order, book_order);
                handler.handle_match(ex);

                new_order.quantity -= cross_quantity;
                cross_order.quantity -= cross_quantity;

                if cross_order.quantity == 0 {
                    self.orders.pop();
                }

                if new_order.quantity == 0 {
                    break;
                }
            }
        }

        fn add_order(&mut self, order: &Order) {
            self.orders.push(Cell::new(order))
        }
    }

    pub struct OrderBook {
        symbol:     Symbol,
        buys:       BookSide,
        sells:      BookSide,
        all_orders: HashMap<Cell<Order>>
    }

    impl OrderBook {
        fn new(symbol: Symbol) -> OrderBook {
            OrderBook {
                symbol:     symbol,
                buys:       BookSide::new(BuyComparer{}),
                sells:      BookSide::new(SellComparer{}),
                all_orders: HashMap::new()
            }
        }
    }

    pub trait OrderMatcher {
        fn add_order<T: ExecutionHandler>(&self, book: mut &OrderBook,
                                          order: &Order, handler: T);

        // match overlapping orders and notify listener of resulting executions
        fn match<T: ExecutionHandler>(&self, book: mut &OrderBook, handler: T);
    }

    pub struct BasicMatcher {}

    pub struct BatchMatcher {}

    impl OrderMatcher for BasicMatcher {
        fn add_order<T: ExecutionHandler>(&self, order: &Order, handler: T) {
            let counter_book = match order.side {
                Buy  => &self.sells,
                Sell => &self.buys
            }

            counter_book.add_order(order, handler);

            if order.quantity > 0 {
                let book = match order.side {
                    Buy  => &self.buys,
                    Sell => &self.sells
                }

                book.add_order(order)
            }
        }
    }

    impl OrderMatcher for BatchMatcher {
        fn add_order(&self, order: Order) {
            match order.side {
                OrderSide::Buy  => buys.push(order),
                OrderSide::Sell => sells.push(order)
            }
        }
    }

}
