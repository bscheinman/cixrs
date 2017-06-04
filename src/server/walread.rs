extern crate bincode;
extern crate libcix;
extern crate memmap;
extern crate serde;
#[macro_use]
extern crate serde_derive;

mod messages;
mod wal;

use libcix::order::trade_types::*;
use messages::EngineMessage;
use std::env::args;
use std::path::Path;

fn main() {
    let mut cli_args = args();
    cli_args.next();
    let path_str = cli_args.next().unwrap();
    let wal_path = Path::new(path_str.as_str());

    let reader = wal::WalReader::from_path(wal_path).unwrap();

    for msg in reader {
        match msg {
            EngineMessage::NewOrder(data) => {
                println!("new order {:?}", data);
            },
            EngineMessage::CancelOrder(data) => {
                println!("cancel order {}", data.order_id);
            }
        }
    }
}
