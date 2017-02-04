extern crate capnpc;

fn main() {
    ::capnpc::CompilerCommand::new()
        .src_prefix("src/libcix/schema")
        .file("src/libcix/schema/cix.capnp")
        .run().expect("capnpc failed");
}
