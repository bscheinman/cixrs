extern crate libcix;

use libcix::heap;

fn main() {
    let mut h = heap::TreeHeap::new(16);
    let mut handles = Vec::new();

    println!("{}", h);

    //for x in vec![5u32, 1u32, 10u32] {
    for x in 0..10u32 {
        handles.push(h.insert(|v| { *v =  x; }));
        println!("added {}", x);
        println!("new heap contents:");
        println!("{}", h);
        h.validate();
        //println!("{:?}", h);
    }

    for x in vec![8u32, 5u32, 2u32, 7u32] {
        println!("removing {}", x);
        h.remove(handles[x as usize].unwrap().clone());
        println!("new heap contents:");
        println!("{}", h);

        //println!("===============================\n{:?}\n\n", h);
        h.validate();
    }

    while !h.is_empty() {
        let v = h.pop();
        println!("popped {}", v);
        println!("new heap contents:");
        println!("{}", h);
        h.validate();
    }
}
