use capnp;
use capnp::capability::Promise;
use libcix::cix_capnp as cp;
use self::cp::trading_session::*;
use libcix::order::trade_types;
use tokio_core::reactor;
use uuid::Uuid;

pub struct Session {
    handle: reactor::Handle,
    user: Uuid
}

impl Session {
    pub fn new(handle: reactor::Handle) -> Self {
        Session {
            handle: handle,
            user: Uuid::default()
        }
    }
}

impl Server for Session {
    fn authenticate(&mut self, params: AuthenticateParams,
                    mut results: AuthenticateResults)
                    -> Promise<(), capnp::Error> {
        let raw_uuid = pry!(pry!(params.get()).get_user());
        let userid = pry!(trade_types::read_uuid(raw_uuid).map_err(|e| {
            capnp::Error::failed("invalid userid".to_string())
        }));

        self.user = userid;

        println!("new session for user {}", self.user);

        results.get().set_response(cp::AuthCode::Ok);
        Promise::ok(())
    }
}
