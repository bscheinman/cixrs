extern crate capnp;
#[macro_use]
extern crate capnp_rpc;
extern crate futures;
extern crate libcix;
extern crate tokio_core;
extern crate uuid;

use capnp_rpc as rpc;
use futures::Future;
use libcix::cix_capnp as cp;
use self::cp::trading_session;
use std::net::ToSocketAddrs;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpStream;
use uuid::Uuid;

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
            println!("failed to authenticate as user {}", userid);
        }
    }
}
