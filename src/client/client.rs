extern crate capnp;
#[macro_use]
extern crate capnp_rpc;
extern crate futures;
extern crate libcix;
extern crate rand;
extern crate tokio_core;
extern crate uuid;

use capnp::capability::Promise;
use capnp_rpc as rpc;
use futures::Future;
use libcix::cix_capnp as cp;
use libcix::order::trade_types::*;
use self::cp::trading_session;
use rand::Rng;
use std::io;
use std::net::ToSocketAddrs;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpStream;
use uuid::Uuid;

struct ClientContext {
    pub core:   reactor::Core,
    pub orders: Vec<u64>,
    pub client: trading_session::Client
}

impl ClientContext {
    pub fn new(core: reactor::Core, client: trading_session::Client) -> Self {
        ClientContext {
            core:   core,
            orders: Vec::new(),
            client: client
        }
    }

    fn process_new_order_line(&mut self, line: &String) {
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

        let mut order_req = self.client.new_order_request();
        {
            let mut builder = order_req.get().get_order().unwrap();
            builder.set_symbol(symbol.as_str());
            builder.set_side(side);
            builder.set_price(price);
            builder.set_quantity(quantity);
        }

        let response = self.core.run(order_req.send().promise).unwrap();
        let response_data = response.get().unwrap();

        match response_data.get_code().unwrap() {
            cp::ErrorCode::Ok => {
                println!("order accepted with ID {}", response_data.get_id());
                self.orders.push(response_data.get_id());
            },
            cp::ErrorCode::NotAuthenticated => {
                println!("order rejected because user not signed in");
            },
            _ => { unreachable!() }
        }
    }

    fn process_cancel_line(&mut self, line: &String) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 2);

        let order_ix: usize = fields[1].parse().unwrap();
        assert!(order_ix < self.orders.len());

        let order_id = self.orders[order_ix];

        let mut cancel_req = self.client.cancel_order_request();
        {
            let mut builder = cancel_req.get().get_cancel().unwrap();
            builder.set_id(order_id);
        }

        let response = self.core.run(cancel_req.send().promise).unwrap();
        match response.get().unwrap().get_code().unwrap() {
            cp::ErrorCode::Ok => {
                println!("canceled order {}", order_id);
            },
            _ => {
                println!("failed to cancel order {}", order_id);
            }
        }
    }

    fn process_open_orders_line(&mut self) {
        let mut open_orders_req = self.client.get_open_orders_request();
        let response = self.core.run(open_orders_req.send().promise).unwrap();
        let contents = response.get().unwrap();

        match contents.get_code().unwrap() {
            cp::ErrorCode::Ok => {
                let orders = contents.get_orders().unwrap();
                println!("we have {} open orders:", orders.len());

                for order in orders.iter() {
                    println!("{:?}", Order::from_capnp(order).unwrap());
                }
            },
            _ => {
                println!("failed to get open orders");
            }
        }
    }

    pub fn process_line(&mut self, line: &String) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let action = fields[0].to_uppercase();
        if action == "CANCEL" {
            self.process_cancel_line(line);
        } else if action == "OPEN_ORDERS" {
            self.process_open_orders_line();
        } else {
            self.process_new_order_line(line);
        }
    }
}

struct ExecutionFeedImpl;
impl cp::execution_feed::Server for ExecutionFeedImpl {
    fn execution(&mut self, params: cp::execution_feed::ExecutionParams,
                 results: cp::execution_feed::ExecutionResults)
                 -> Promise<(), capnp::Error> {
        let execution = params.get().unwrap().get_execution().unwrap();
        let symbol = Symbol::from_capnp(execution.get_symbol().unwrap()).unwrap();

        println!("received execution {}: {} {} shares of {} @ {}",
                 execution.get_id(),
                 match execution.get_side().unwrap() {
                    cp::OrderSide::Buy => "bought",
                    cp::OrderSide::Sell => "sold"
                 }, execution.get_quantity(), symbol, execution.get_price());

        Promise::ok(())
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

    let mut context = ClientContext::new(core, cli);

    let mut auth_req = context.client.authenticate_request();
    let userid = rand::weak_rng().next_u64();

    println!("connecting with userid {}", userid);
    auth_req.get().set_user(userid);

    let response = context.core.run(auth_req.send().promise).unwrap();

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
    let mut feed_req = context.client.execution_subscribe_request();
    feed_req.get().set_feed(exec_feed);

    let feed = context.core.run(feed_req.send().promise).unwrap();

    let stdin = io::stdin();
    let mut line = String::new();

    while stdin.read_line(&mut line).unwrap() > 0 {
        line.trim();
        context.process_line(&line);
        line.clear();
    }

    loop{}
}
