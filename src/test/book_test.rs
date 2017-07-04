extern crate libcix;

use libcix::book::*;
use libcix::order::trade_types::*;

const SYMBOL: &'static str = "GOOG";

struct ExecutionPrinter;

fn entry_display(entry: Option<MdEntry>) -> (String, String) {
    match entry {
        Some(data) => {
            (format!("{}", data.price).to_string(), format!("{}", data.quantity).to_string())
        },
        None => {
            ("___".to_string(), "___".to_string())
        }
    }
}

impl ExecutionHandler for ExecutionPrinter {
    fn ack_order(&self, order_id: OrderId, status: ErrorCode) {
        println!("ACK {}", order_id)
    }

    fn handle_match(&self, execution: Execution) {
        println!("{}", execution)
    }

    fn handle_market_data_l1(&self, md:L1Md) {
        let (bid_price, bid_quantity) = entry_display(md.bid);
        let (ask_price, ask_quantity) = entry_display(md.ask);

        println!("{} bid {} x {}, ask {} x {}", md.symbol,
                 bid_price,
                 bid_quantity,
                 ask_price,
                 ask_quantity);
    }

    fn handle_market_data_l2(&self, md: L2Md) {
        println!("Bids:");
        if md.bids.n_entry == 0 {
            println!("None");
        } else {
            for entry in md.bids.iter() {
                println!("\t{}x{}", entry.price, entry.quantity);
            }
        }

        println!("Asks:");
        if md.asks.n_entry == 0 {
            println!("None");
        } else {
            for entry in md.asks.iter() {
                println!("\t{}x{}", entry.price, entry.quantity);
            }
        }
    }
}

fn create_order(side: OrderSide, price: Price, quantity: Quantity,
                order_seq: &mut u64) -> Order {
    let mut o = Order::default();
    o.id = OrderId::new(0, side, *order_seq).unwrap();
    o.symbol = Symbol::from_str(SYMBOL).unwrap();
    o.side = side;
    o.price = price;
    o.quantity = quantity;
    *order_seq += 1;
    o
}

fn main() {
    let mut book = OrderBook::new(Symbol::from_str(SYMBOL).unwrap(), 0);
    let mut matcher = BasicMatcher{};
    let printer = ExecutionPrinter{};
    let mut order_seq = 0u64;

    // Match two orders with same price against each other completely
    let mut order = create_order(OrderSide::Sell, 500f64, 1000u32,
                                 &mut order_seq);
    matcher.add_order(&mut book, order, &printer);

    order = create_order(OrderSide::Buy, 500f64, 1000u32, &mut order_seq);
    matcher.add_order(&mut book, order, &printer);

    // Check two orders that cross in price and leave some of the old order on
    // the book
    order = create_order(OrderSide::Buy, 500f64, 1000u32, &mut order_seq);
    matcher.add_order(&mut book, order, &printer);

    order = create_order(OrderSide::Sell, 450f64, 100u32, &mut order_seq);
    matcher.add_order(&mut book, order, &printer);

    // Cross that order and leave some of the new order on the book
    order = create_order(OrderSide::Buy, 475f64, 1200u32, &mut order_seq);
    matcher.add_order(&mut book, order, &printer);

    // Trade with remainder of last order
    order = create_order(OrderSide::Sell, 470f64, 100u32, &mut order_seq);
    matcher.add_order(&mut book, order, &printer);

    // Add another buy order to the book
    order = create_order(OrderSide::Buy, 472f64, 500u32, &mut order_seq);
    matcher.add_order(&mut book, order, &printer);

    // Trade through both sell orders on book
    order = create_order(OrderSide::Sell, 470f64, 2000u32, &mut order_seq);
    matcher.add_order(&mut book, order, &printer);
}
