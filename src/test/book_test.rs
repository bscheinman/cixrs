extern crate libcix;
extern crate uuid;

use libcix::book::*;
use libcix::order::trade_types::*;
use uuid::Uuid;

struct ExecutionPrinter;

impl ExecutionHandler for ExecutionPrinter {
    fn handle_match(&self, execution: &Execution) {
        println!("{}", execution)
    }
}

fn create_order(side: OrderSide, price: Price, quantity: Quantity) -> Order {
    let mut o = Order::default();
    o.id = Uuid::new_v4();
    o.symbol = "GOOG";
    o.side = side;
    o.price = price;
    o.quantity = quantity;
    o
}

fn main() {
    let mut book = OrderBook::new("GOOG");
    let mut matcher = BasicMatcher{};
    let printer = ExecutionPrinter{};

    let mut order = create_order(OrderSide::Sell, 500f64, 1000u32);

    // Match two orders with same price against each other completely
    matcher.add_order(&mut book, &mut order, &printer);

    order.side = OrderSide::Buy;
    matcher.add_order(&mut book, &mut order, &printer);

    // Check two orders that cross in price and leave some of the old order on
    // the book
    order = create_order(OrderSide::Buy, 500f64, 1000u32);
    matcher.add_order(&mut book, &mut order, &printer);

    order = create_order(OrderSide::Sell, 450f64, 100u32);
    matcher.add_order(&mut book, &mut order, &printer);

    // Cross that order and leave some of the new order on the book
    order = create_order(OrderSide::Buy, 475f64, 1200u32);
    matcher.add_order(&mut book, &mut order, &printer);

    // Trade with remainder of last order
    order = create_order(OrderSide::Sell, 470f64, 100u32);
    matcher.add_order(&mut book, &mut order, &printer);

    // Add another buy order to the book
    order = create_order(OrderSide::Buy, 472f64, 500u32);
    matcher.add_order(&mut book, &mut order, &printer);

    // Trade through both sell orders on book
    order = create_order(OrderSide::Sell, 470f64, 2000u32);
    matcher.add_order(&mut book, &mut order, &printer);
}
