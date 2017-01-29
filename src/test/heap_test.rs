extern crate libcix;

use libcix::heap;

fn main() {
    let mut h = heap::TreeHeap::new(16);
    println!("{}", h);

    //for x in vec![5u32, 1u32, 10u32] {
    for x in 0..10u32 {
        let _ = h.insert(|v| { *v =  x; });
        println!("added {}", x);
        println!("new heap contents:");
        println!("{}", h);
        //println!("{:?}", h);
    }

    println!("{}", h);

    while !h.is_empty() {
        let v = h.pop();
        println!("popped {}", v);
        println!("new heap contents:");
        println!("{}", h);
    }
}
