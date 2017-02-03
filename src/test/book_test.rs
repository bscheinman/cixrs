extern crate libcix;

use libcix::book::*;
use libcix::order::trade_types::*;

struct ExecutionPrinter;

impl ExecutionHandler for ExecutionPrinter {
    fn handle_match(&self, execution: &Execution) {
        println!("{}", execution)
    }
}

fn main() {
    let mut book = OrderBook::new("GOOG");
    let mut matcher = BasicMatcher{};
    let printer = ExecutionPrinter{};

    let mut order = Order::default();
    order.symbol = "GOOG";
    order.side = OrderSide::Sell;
    order.price = 500f64;
    order.quantity = 1000u32;

    matcher.add_order(&mut book, &mut order, &printer);

    order.side = OrderSide::Buy;
    matcher.add_order(&mut book, &mut order, &printer);
}
