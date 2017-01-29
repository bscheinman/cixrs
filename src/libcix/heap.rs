use std::cell::Cell;
use std::cmp::{Ord, Ordering};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::vec::Vec;

// I would prefer to use Option<u32> or something similar here but I don't
// think the compiler would be able to perform null pointer optimization on
// it because in theory it could hold any value that can fit in 32 bits
type HeapPtr = i32;

#[derive(Clone, Copy)]
struct HeapNodeMd {
    parent:         HeapPtr,
    left_child:     HeapPtr,
    right_child:    HeapPtr,
    size:           u32
}

struct HeapNode<T> where T: Default + Ord{
    value:  T,
    md:     Cell<HeapNodeMd>
}

pub struct TreeHeap<T> where T: Default + Ord {
    root: HeapPtr,
    pool: Vec<HeapNode<T>>,
    free_list: Vec<HeapPtr>
}

pub struct HeapHandle {
    index: HeapPtr
}

impl HeapNodeMd {
    fn new() -> HeapNodeMd {
        HeapNodeMd {
            parent:         -1,
            left_child:     -1,
            right_child:    -1,
            size:           0
        }
    }

    fn reset(&mut self) {
        self.parent = -1;
        self.left_child = -1;
        self.right_child = -1;
        self.size = 0;
    }
}

impl<T> HeapNode<T> where T: Default + Ord {
    fn reset<F>(&mut self, ctor: F) where F: Fn(&mut T) {
        self.md.get_mut().reset();
        ctor(&mut self.value);
    }

    fn new() -> HeapNode<T> {
        HeapNode {
            value:  T::default(),
            md:     Cell::new(HeapNodeMd::new())
        }
    }
}

impl<T> Display for HeapNode<T> where T: Default + Display + Ord {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl<T> TreeHeap<T> where T: Default + Ord {
    pub fn new(capacity: usize) -> TreeHeap<T> {
        let mut heap = TreeHeap {
            root: -1,
            pool: Vec::with_capacity(capacity),
            free_list: (0..(capacity as i32)).collect()
        };

        for _ in 0..capacity {
            heap.pool.push(HeapNode::new());
        }

        heap
    }

    fn as_option(i: HeapPtr) -> Option<HeapPtr> {
        if i < 0 {
            None
        } else {
            Some(i)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root < 0
    }

    pub fn peek(&self) -> Option<HeapHandle> {
        Self::as_option(self.root).map(|x| {HeapHandle{ index: x }})
    }

    pub fn get(&self, h: HeapHandle) -> &T {
        let i = h.index;
        assert!(i >= 0 && (i as usize) < self.pool.len());
        &self.pool[i as usize].value
    }

    pub fn get_mut(&mut self, h: HeapHandle) -> &mut T {
        let i = h.index;
        assert!(i >= 0 && (i as usize) < self.pool.len());
        &mut self.pool[i as usize].value
    }

    fn get_node(&self, i: HeapPtr) -> &HeapNode<T> {
        assert!(i >= 0 && (i as usize) < self.pool.len());
        &self.pool[i as usize]
    }

    fn get_node_mut(&mut self, i: HeapPtr) -> &mut HeapNode<T> {
        assert!(i >= 0 && (i as usize) < self.pool.len());
        &mut self.pool[i as usize]
    }

    fn get_node_md(&self, i: HeapPtr) -> HeapNodeMd {
        assert!(i >= 0 && (i as usize) < self.pool.len());
        self.pool[i as usize].md.get()
    }

    fn set_node_md(&self, i: HeapPtr, md: HeapNodeMd) {
        assert!(i >= 0 && (i as usize) < self.pool.len());
        self.pool[i as usize].md.set(md)
    }

    fn get_node_md_mut(&mut self, i: HeapPtr) -> &mut HeapNodeMd {
        assert!(i >= 0 && (i as usize) < self.pool.len());
        self.pool[i as usize].md.get_mut()
    }

    fn update_size(&self, index: HeapPtr) {
        let mut node = self.get_node_md(index);

        node.size = 0;
        if node.left_child >= 0 {
             node.size += self.get_node_md(node.left_child).size;
        }

        if node.right_child >= 0 {
            node.size += self.get_node_md(node.right_child).size;
        }

        self.set_node_md(index, node);
    }

    fn decrement_size(&mut self, index: HeapPtr) {
        let mut i = index;
        while i >= 0 {
            let md = self.get_node_md_mut(i);
            md.size -= 1;
            i = md.parent;
        }
    }

    fn pull_up(&self, left: HeapPtr, right: HeapPtr) -> HeapPtr {
        assert!(left != right || (left < 0 && right < 0));

        if left < 0 {
            return right;
        }

        if right < 0 {
            return left;
        }

        let go_left = {
            let left_node = self.get_node(left);
            let right_node = self.get_node(right);
            left_node.value > right_node.value
        };

        if go_left {
            let mut left_md = self.get_node_md(left);
            left_md.left_child =
                self.pull_up(left_md.left_child,
                             left_md.right_child);
            left_md.right_child = right;
            self.set_node_md(left, left_md);
            self.update_size(left);
            left
        } else {
            let mut right_md = self.get_node_md(right);
            right_md.right_child =
                self.pull_up(right_md.left_child,
                             right_md.right_child);
            right_md.left_child = left;
            self.set_node_md(right, right_md);
            self.update_size(right);
            right
        }
    }

    fn insert_node(&mut self, head: HeapPtr, new: HeapPtr) -> HeapPtr {
        assert!(head >= 0 && new >= 0);
        let (parent_index, descend_index) = {
            let head_node = self.get_node(head);
            let new_node = self.get_node(new);

            match head_node.value.cmp(&new_node.value) {
                Ordering::Greater => (head, new),
                _       => (new, head)
            }
        };

        let head_node = self.get_node_md(head);
        let mut parent_node = self.get_node_md(parent_index);
        let mut descend_node = self.get_node_md(descend_index);

        parent_node.size += 1;

        if head_node.left_child < 0 {
            parent_node.right_child = head_node.right_child;
            if parent_node.right_child >= 0 {
                self.get_node_md_mut(head_node.right_child).parent =
                    parent_index;
            }
            parent_node.left_child = descend_index;
            descend_node.parent = parent_index;
            descend_node.left_child = -1;
            descend_node.right_child = -1;
            self.set_node_md(parent_index, parent_node);
            self.set_node_md(descend_index, descend_node);
            return parent_index;
        }

        if head_node.right_child < 0 {
            parent_node.left_child = head_node.left_child;
            self.get_node_md_mut(head_node.left_child).parent = parent_index;
            parent_node.right_child = descend_index;
            descend_node.parent = parent_index;
            descend_node.left_child = -1;
            descend_node.right_child = -1;
            self.set_node_md(parent_index, parent_node);
            self.set_node_md(descend_index, descend_node);
            return parent_index;
        }

        let child_index = {
            let left_size = {
                let left_node = self.get_node_md_mut(head_node.left_child);
                left_node.parent = parent_index;
                left_node.size
            };

            let right_size = {
                let right_node = self.get_node_md_mut(head_node.right_child);
                right_node.parent = parent_index;
                right_node.size
            };

            if left_size <= right_size {
                head_node.left_child
            } else {
                head_node.right_child
            }
        };

        self.insert_node(child_index, descend_index)
    }

    // XXX: only implement this if T: Copy
    // That way we can copy out the node's value and safely reuse the node
    /*
    pub fn pop(&mut self) -> HeapPtr {
        assert!(self.root >= 0);
        let head_index = self.root;
        let &head = self.get(head_index);
        self.root = self.pull_up(head.left_child,
                                 head.right_child);
        if self.root >= 0 {
            let &mut new_head = self.get_mut(self.root);
            new_head.parent = -1;
        }

        head
    }
    */

    pub fn insert<F>(&mut self, ctor: F) -> Result<HeapHandle, &'static str>
            where F: Fn(&mut T) {
        // XXX: add option to grow list if necessary or make future-aware to
        // add when possible, but for now just fail
        let index = match self.free_list.pop() {
            Some(i) => i,
            None => { return Err("heap full"); }
        };

        self.get_node_mut(index).reset(ctor);

        if self.root >= 0 {
            let old_root = self.root;
            let new_root = self.insert_node(old_root, index);
            self.root = new_root;
        } else {
            self.root = index;
        }

        Ok(HeapHandle{ index: index })
    }

    pub fn remove(&mut self, h: HeapHandle) {
        let index = h.index;
        let node = self.get_node_md(index);
        let replacement = self.pull_up(node.left_child, node.right_child);

        if node.parent < 0 {
            self.root = replacement;
        } else {
            {
                let parent = self.get_node_md_mut(node.parent);
                if parent.left_child == index {
                    parent.left_child = replacement;
                } else {
                    assert_eq!(parent.right_child, index);
                    parent.right_child = replacement;
                }
            }

            self.decrement_size(node.parent);
        }

        self.free_list.push(index);
    }
}

impl<T> Display for TreeHeap<T> where T: Default + Display + Ord {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let mut nodes = Vec::new();
        let mut children = Vec::new();

        if self.root < 0 {
            return write!(f, "empty heap");
        } else {
            nodes.push(self.root);
        }

        while !nodes.is_empty() {
            for i in nodes {
                if i >= 0 {
                    let node = self.get_node(i);
                    let node_md = node.md.get();

                    try!(write!(f, "{} ", node));
                    children.push(node_md.left_child);
                    children.push(node_md.right_child);
                } else {
                    try!(write!(f, "_ "));
                }
            }

            nodes = children;
            children = Vec::new();

            try!(write!(f, "\n"));
        }

        Ok(())
    }
}
