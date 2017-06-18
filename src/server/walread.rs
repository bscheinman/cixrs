extern crate bincode;
extern crate libcix;
extern crate memmap;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;

mod messages;
mod wal;

use libcix::order::trade_types::*;
use messages::EngineMessage;
use std::env::args;
use std::path::Path;

fn print_entries<L>(iter: L) where L: Iterator<Item=Result<EngineMessage, String>> {
    for entry in iter {
        match entry {
            Ok(msg) => {
                match msg {
                    EngineMessage::NewOrder(data) => {
                        println!("new order {:?}", data);
                    },
                    EngineMessage::CancelOrder(data) => {
                        println!("cancel order {}", data.order_id);
                    },
                    _ => unreachable!()
                }
            },
            Err(e) => {
                println!("failed to read entry: {}", e);
                break;
            }
        }
    }
}

fn main() {
    let mut cli_args = args();
    cli_args.next();
    let path_str = cli_args.next().unwrap();
    let wal_path = Path::new(path_str.as_str());

    if wal_path.is_dir() {
        let reader = wal::WalDirectoryReader::new(wal_path).unwrap();
        print_entries(reader);
    } else {
        let reader = wal::WalReader::from_path(wal_path).unwrap();
        print_entries(reader);
    }
}
