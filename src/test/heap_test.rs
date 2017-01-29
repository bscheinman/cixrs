extern crate libcix;

use libcix::heap;

fn main() {
    let mut h = heap::TreeHeap::new(256);
    println!("{}", h);

    for x in vec![5u32, 1u32, 10u32] {
        h.insert(|v| { *v =  x });
    }
}
