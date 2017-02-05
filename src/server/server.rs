extern crate capnp;
#[macro_use]
extern crate capnp_rpc;
extern crate futures;
extern crate libcix;
extern crate tokio_core;
extern crate uuid;

use capnp_rpc as rpc;
use futures::{Future, Stream};
use libcix::cix_capnp as cp;
use std::net::ToSocketAddrs;
use tokio_core::reactor;
use tokio_core::io::Io;
use tokio_core::net::TcpListener;

mod session;

fn main() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();
    
    let addr = "localhost:2468".to_socket_addrs().unwrap().next()
        .expect("could not parse address");
    let socket = TcpListener::bind(&addr, &handle).unwrap();

    let done = socket.incoming().for_each(move |(s, _)| {
        let (reader, writer) = s.split();
        let network = rpc::twoparty::VatNetwork::new(reader, writer,
            rpc::rpc_twoparty_capnp::Side::Server, Default::default());

        let sess = cp::trading_session::ToClient::new(
            session::Session::new(handle.clone()))
            .from_server::<capnp_rpc::Server>();
        let rpc_system = rpc::RpcSystem::new(Box::new(network),
            Some(sess.client));

        handle.spawn(rpc_system.map_err(|_| ()));
        Ok(())
    });

    core.run(done).unwrap();
}
