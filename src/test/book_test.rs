extern crate libcix;

use libcix::book::*;
use libcix::order::trade_types::*;

const SYMBOL: &'static str = "GOOG";

struct ExecutionPrinter;

impl ExecutionHandler for ExecutionPrinter {
    fn handle_match(&self, execution: Execution) {
        println!("{}", execution)
    }
}

fn create_order(side: OrderSide, price: Price, quantity: Quantity) -> Order {
    let mut o = Order::default();
    o.id = OrderId::new(0, side, 0).unwrap();
    o.symbol = Symbol::from_str(SYMBOL).unwrap();
    o.side = side;
    o.price = price;
    o.quantity = quantity;
    o
}

fn main() {
    let mut book = OrderBook::new(Symbol::from_str(SYMBOL).unwrap(), 0);
    let mut matcher = BasicMatcher{};
    let printer = ExecutionPrinter{};

    // Match two orders with same price against each other completely
    let mut order = create_order(OrderSide::Sell, 500f64, 1000u32);
    matcher.add_order(&mut book, order, &printer);

    order = create_order(OrderSide::Buy, 500f64, 1000u32);
    matcher.add_order(&mut book, order, &printer);

    // Check two orders that cross in price and leave some of the old order on
    // the book
    order = create_order(OrderSide::Buy, 500f64, 1000u32);
    matcher.add_order(&mut book, order, &printer);

    order = create_order(OrderSide::Sell, 450f64, 100u32);
    matcher.add_order(&mut book, order, &printer);

    // Cross that order and leave some of the new order on the book
    order = create_order(OrderSide::Buy, 475f64, 1200u32);
    matcher.add_order(&mut book, order, &printer);

    // Trade with remainder of last order
    order = create_order(OrderSide::Sell, 470f64, 100u32);
    matcher.add_order(&mut book, order, &printer);

    // Add another buy order to the book
    order = create_order(OrderSide::Buy, 472f64, 500u32);
    matcher.add_order(&mut book, order, &printer);

    // Trade through both sell orders on book
    order = create_order(OrderSide::Sell, 470f64, 2000u32);
    matcher.add_order(&mut book, order, &printer);
}
