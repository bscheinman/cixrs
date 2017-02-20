extern crate capnp;
#[macro_use]
extern crate capnp_rpc;
extern crate futures;
extern crate libcix;
extern crate tokio_core;
extern crate uuid;

use capnp::capability::Promise;
use capnp_rpc as rpc;
use futures::Future;
use libcix::cix_capnp as cp;
use libcix::order::trade_types::*;
use self::cp::trading_session;
use std::io;
use std::net::ToSocketAddrs;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpStream;
use uuid::Uuid;

struct ExecutionFeedImpl;
impl cp::execution_feed::Server for ExecutionFeedImpl {
    fn execution(&mut self, params: cp::execution_feed::ExecutionParams,
                 results: cp::execution_feed::ExecutionResults)
                 -> Promise<(), capnp::Error> {
        let execution = params.get().unwrap().get_execution().unwrap();
        let exec_id = Uuid::from_bytes(execution.get_id().unwrap().get_bytes().unwrap()).unwrap();
        let symbol = read_symbol(execution.get_symbol().unwrap()).unwrap();

        println!("received execution {}: {} {} shares of {} @ {}",
                 exec_id, match execution.get_side().unwrap() {
                    cp::OrderSide::Buy => "bought",
                    cp::OrderSide::Sell => "sold"
                 }, execution.get_quantity(), symbol, execution.get_price());

        Promise::ok(())
    }
}

fn process_line(core: &mut reactor::Core, cli: &trading_session::Client,
                line: &String) {
    let fields: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(fields.len(), 4);

    let symbol = Symbol::from_str(fields[0])
        .expect(format!("invalid symbol {}", fields[0]).as_str());

    let side_str = fields[1].to_uppercase();
    let side = if side_str == "B" || side_str == "BUY" {
        cp::OrderSide::Buy
    } else if side_str == "S" || side_str == "SELL" {
        cp::OrderSide::Sell
    } else {
        panic!("invalid side {}", fields[1]);
    };

    let quantity = fields[2].parse().unwrap();
    let price = fields[3].parse().unwrap();

    let mut order_req = cli.new_order_request();
    {
        let mut builder = order_req.get().get_order().unwrap();
        builder.set_symbol(symbol.as_str());
        builder.set_side(side);
        builder.set_price(price);
        builder.set_quantity(quantity);
    }

    let response = core.run(order_req.send().promise).unwrap();
    let response_data = response.get().unwrap();

    match response_data.get_code().unwrap() {
        cp::ErrorCode::Ok => {
            let order_id =
                Uuid::from_bytes(response_data.get_id().unwrap().get_bytes()
                                 .unwrap()).unwrap();
            println!("order accepted with ID {}", order_id);
        },
        cp::ErrorCode::NotAuthenticated => {
            println!("order rejected because user not signed in");
        },
        _ => { unreachable!() }
    }
}

fn main() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();
    let addr = "localhost:2468".to_socket_addrs().unwrap().next()
        .expect("failed to parse address");
    let stream = core.run(TcpStream::connect(&addr, &handle)).unwrap();

    let (reader, writer) = stream.split();
    let rpc_network = Box::new(rpc::twoparty::VatNetwork::new(reader, writer,
        rpc::rpc_twoparty_capnp::Side::Client, Default::default()));

    let mut rpc_system = rpc::RpcSystem::new(rpc_network, None);
    let cli: trading_session::Client = rpc_system.bootstrap(
        rpc::rpc_twoparty_capnp::Side::Server);

    handle.spawn(rpc_system.map_err(|_| ()));

    let mut auth_req = cli.authenticate_request();
    let userid = Uuid::new_v4();

    println!("connecting with userid {}", userid);
    auth_req.get().get_user().unwrap().set_bytes(userid.as_bytes());

    let response = core.run(auth_req.send().promise).unwrap();

    match response.get().unwrap().get_response().unwrap() {
        cp::AuthCode::Ok => {
            println!("authenticated as user {}", userid);
        },
        _ => {
            panic!("failed to authenticate as user {}", userid);
        }
    }

    let exec_feed = cp::execution_feed::ToClient::new(ExecutionFeedImpl)
        .from_server::<::capnp_rpc::Server>();
    let mut feed_req = cli.execution_subscribe_request();
    feed_req.get().set_feed(exec_feed);

    let feed = core.run(feed_req.send().promise).unwrap();

    let stdin = io::stdin();
    let mut line = String::new();

    while stdin.read_line(&mut line).unwrap() > 0 {
        line.trim();
        process_line(&mut core, &cli, &line);
        line.clear();
    }

    loop{}
}
