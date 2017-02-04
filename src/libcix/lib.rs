extern crate capnp;
extern crate time;
extern crate uuid;

pub mod cix_capnp {
    include!(concat!(env!("OUT_DIR"), "/cix_capnp.rs"));
}

pub mod book;
pub mod heap;
pub mod order;

#[cfg(test)]
mod test {
    #[test]
    fn it_works() {
    }
}
