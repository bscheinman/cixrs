use std::cell::Cell;
use std::cmp::{Ord, Ordering};
use std::collections::HashSet;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::vec::Vec;

// I would prefer to use Option<u32> or something similar here but I don't
// think the compiler would be able to perform null pointer optimization on
// it because in theory it could hold any value that can fit in 32 bits
type HeapPtr = i32;

#[derive(Clone, Copy, Debug)]
struct HeapNodeMd {
    parent:         HeapPtr,
    left_child:     HeapPtr,
    right_child:    HeapPtr,
    size:           u32
}

impl Default for HeapNodeMd {
    fn default() -> Self {
        HeapNodeMd {
            parent: -1,
            left_child: -1,
            right_child: -1,
            size: 1
        }
    }
}

#[derive(Debug)]
struct HeapNode<T> where T: Copy + Default {
    value:  T,
    md:     Cell<HeapNodeMd>
}

pub trait Comparer<T> {
    fn compare(x: &T, y: &T) -> Ordering;
}

pub struct DefaultComparer<T> {
    phantom: PhantomData<T>
}

impl<T> Comparer<T> for DefaultComparer<T> where T: Ord {
    fn compare(x: &T, y: &T) -> Ordering {
        x.cmp(y)
    }
}

#[derive(Debug)]
pub struct TreeHeap<T, TCmp> where T: Copy + Default, TCmp: Comparer<T> {
    root: HeapPtr,
    pool: Vec<HeapNode<T>>,
    free_list: Vec<HeapPtr>,
    phantom: PhantomData<TCmp>
}

pub struct TreeHeapOrd<T> where T: Copy + Default + Ord {
    phantom: PhantomData<T>
}

impl<T> TreeHeapOrd<T> where T: Copy + Default + Ord {
    pub fn new(capacity: usize) -> TreeHeap<T, DefaultComparer<T>> {
        TreeHeap::new(capacity)
    }
}

#[derive(Clone, Copy)]
pub struct HeapHandle {
    index: HeapPtr
}

impl HeapNodeMd {
    fn new() -> HeapNodeMd {
        Self::default()
    }

    fn reset(&mut self) {
        self.parent = -1;
        self.left_child = -1;
        self.right_child = -1;
        self.size = 1;
    }
}

impl<T> HeapNode<T> where T: Copy + Default {
    fn reset(&mut self) {
        self.md.get_mut().reset();
    }

    fn new() -> HeapNode<T> {
        HeapNode {
            value:  T::default(),
            md:     Cell::new(HeapNodeMd::new())
        }
    }
}

impl<T> Display for HeapNode<T> where T: Copy + Default + Display {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl<T, TCmp> TreeHeap<T, TCmp> where T: Copy + Default, TCmp: Comparer<T> {
    pub fn new(capacity: usize) -> TreeHeap<T, TCmp> {
        let mut heap = TreeHeap {
            root: -1,
            pool: Vec::with_capacity(capacity),
            free_list: (0..(capacity as i32)).rev().collect(),
            phantom: PhantomData
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

    pub fn capacity(&self) -> usize {
        self.pool.capacity()
    }

    pub fn peek(&self) -> Option<HeapHandle> {
        Self::as_option(self.root).map(|x| { HeapHandle { index: x } })
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

    fn update_size(&mut self, index: HeapPtr) {
        let mut node = self.get_node_md(index);

        node.size = 1;
        if node.left_child >= 0 {
             node.size += self.get_node_md_mut(node.left_child).size;
        }

        if node.right_child >= 0 {
            node.size += self.get_node_md_mut(node.right_child).size;
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

    fn pull_up(&mut self, left: HeapPtr, right: HeapPtr) -> HeapPtr {
        assert!(left != right || (left < 0 && right < 0));

        // If either child is null then we can simply pull up the other child
        // and take its entire subtree with it.  There is no need to
        // recursively restructure that whole subtree as well.  Because we
        // aren't moving around any other nodes we don't need to bother with any
        // of the below bookkeeping.
        if left < 0 {
            return right;
        }

        if right < 0 {
            return left;
        }

        // The child that gets pulled up will become the parent of the other
        // child so we need to choose the child with the greater value to
        // maintain the heap invariant.
        let go_left = {
            let left_node = self.get_node(left);
            let right_node = self.get_node(right);
            match TCmp::compare(&left_node.value, &right_node.value) {
                Ordering::Greater => true,
                _ => false
            }
        };

        // Pull up the appropriate node by making it the parent of both the
        // opposite child node and its recursively reorganized subtree.
        // We have to update the parent of the opposite child node but not the
        // subtree because whichever node is pulled up from that will already
        // have this node as its parent.
        if go_left {
            let mut left_md = self.get_node_md(left);
            left_md.left_child = self.pull_up(left_md.left_child,
                                              left_md.right_child);
            left_md.right_child = right;
            self.set_node_md(left, left_md);
            self.update_size(left);
            self.get_node_md_mut(right).parent = left;
            left
        } else {
            let mut right_md = self.get_node_md(right);
            right_md.right_child = self.pull_up(right_md.left_child,
                                                right_md.right_child);
            right_md.left_child = left;
            self.set_node_md(right, right_md);
            self.update_size(right);
            self.get_node_md_mut(left).parent = right;
            right
        }
    }

    fn insert_node(&mut self, head: HeapPtr, new: HeapPtr) -> HeapPtr {
        assert!(head >= 0 && new >= 0);
        
        // Between the existing head of this subtree and the new node to insert,
        // the one with the greater value will become the new head and the other
        // will be recusrively pushed down the tree
        let (parent_index, descend_index) = {
            let head_node = self.get_node(head);
            let new_node = self.get_node(new);

            match TCmp::compare(&head_node.value, &new_node.value) {
                Ordering::Less => (new, head),
                _ => (head, new)
            }
        };

        let head_node = self.get_node_md(head);

        // Whichever node ends up as the new head of this subtree will have
        // size equal to the size of the old subtree plus one
        // This, along with the one assignments below, are the only places
        // where we need to update node size during insertion; the node that
        // ends up being pushed down the tree will eventually either become the
        // head of a lower subtree, in which case this assignment will take
        // place in the corresponding recursive call, or it will become a leaf
        // node, in which case it will be assigned a size of one.
        self.get_node_md_mut(parent_index).size = head_node.size + 1;

        // If either child is null then we can just make the descending node a
        // child of the new parent and stop there.
        if head_node.left_child < 0 {
            if head_node.right_child >= 0 {
                self.get_node_md_mut(head_node.right_child).parent =
                    parent_index;
            }
            {
                let parent_node = self.get_node_md_mut(parent_index);
                parent_node.right_child = head_node.right_child;
                parent_node.left_child = descend_index;
            }
            {
                // The descending node is now a leaf node.
                let descend_node = self.get_node_md_mut(descend_index);
                descend_node.parent = parent_index;
                descend_node.left_child = -1;
                descend_node.right_child = -1;
                descend_node.size = 1;
            }
            return parent_index;
        }

        if head_node.right_child < 0 {
            self.get_node_md_mut(head_node.left_child).parent = parent_index;
            {
                let parent_node = self.get_node_md_mut(parent_index);
                parent_node.left_child = head_node.left_child;
                parent_node.right_child = descend_index;
            }
            {
                let descend_node = self.get_node_md_mut(descend_index);
                descend_node.parent = parent_index;
                descend_node.left_child = -1;
                descend_node.right_child = -1;
                descend_node.size = 1;
            }
            return parent_index;
        }

        let left_size = self.get_node_md_mut(head_node.left_child).size;
        let right_size = self.get_node_md_mut(head_node.right_child).size;
        let go_left = left_size <= right_size;

        // Insert into the smaller subtree to keep the tree relatively balanced
        let child_index = if go_left {
            head_node.left_child
        } else {
            head_node.right_child
        };

        // Recursively insert the descending node into the appropriate subtree
        let new_child = self.insert_node(child_index, descend_index);
        let mut parent_node = self.get_node_md(parent_index);
        if go_left {
            parent_node.left_child = new_child;
            parent_node.right_child = head_node.right_child;
        } else {
            parent_node.left_child = head_node.left_child;
            parent_node.right_child = new_child;
        }

        self.get_node_md_mut(parent_node.left_child).parent = parent_index;
        self.get_node_md_mut(parent_node.right_child).parent = parent_index;

        parent_node.parent = head_node.parent;
        self.set_node_md(parent_index, parent_node);

        parent_index
    }

    pub fn pop(&mut self) -> T {
        assert!(self.root >= 0);
        let head_index = self.root;
        let head = self.get_node_md(head_index);
        self.root = self.pull_up(head.left_child,
                                 head.right_child);
        self.get_node(head_index).value
    }

    fn insert_impl(&mut self, index: HeapPtr) {
        if self.root >= 0 {
            let old_root = self.root;
            let new_root = self.insert_node(old_root, index);
            self.root = new_root;
        } else {
            self.root = index;
        }
    }

    pub fn insert(&mut self, val: T) -> Result<HeapHandle, &'static str> {
        // XXX: add option to grow list if necessary or make future-aware to
        // add when possible, but for now just fail
        let index = match self.free_list.pop() {
            Some(i) => i,
            None => { return Err("heap full"); }
        };

        {
            let node = self.get_node_mut(index);
            node.reset();
            node.value = val;
        }

        self.insert_impl(index);

        Ok(HeapHandle{ index: index })
    }

    fn remove_impl(&mut self, h: HeapHandle) {
        let index = h.index;
        let node = self.get_node_md(index);
        let replacement = self.pull_up(node.left_child, node.right_child);

        if node.parent < 0 {
            self.root = replacement;
            if replacement >= 0 {
                self.get_node_md_mut(replacement).parent = -1;
            }
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

            if replacement >= 0 {
                self.get_node_md_mut(replacement).parent = node.parent;
            }

            self.decrement_size(node.parent);
        }

        // XXX: rebalance after removals?
    }

    pub fn remove(&mut self, h: HeapHandle) {
        self.remove_impl(h);
        self.free_list.push(h.index);
    }

    // XXX: For now just remove and readd the node; in the future it might be
    // worth exploring an approach involving moving the node up or down the tree
    // as necessary
    pub fn update<F>(&mut self, h: HeapHandle, f: F) where F: Fn(&mut T) {
        let index = h.index;
        f(&mut self.get_node_mut(index).value);

        // XXX: Check whether node's ordering changed and leave it in place if
        // possible
        self.remove_impl(h);
        self.get_node_mut(index).reset();
        self.insert_impl(index);
    }

    fn validate_node(&self, i: HeapPtr, visited: &mut HashSet<HeapPtr>) {
        if i < 0 {
            return;
        }

        // Make sure this is actually a tree and that there aren't multiple
        // nodes pointing to the same children
        assert!(!visited.contains(&i));
        visited.insert(i);

        let mut child_size = 0;
        let node = self.get_node(i);
        let md = node.md.get();

        // Ensure heap invariant (each node's value is greater than those of its
        // children)
        if md.left_child >= 0 {
            let left_node = self.get_node(md.left_child);
            let left_md = left_node.md.get();
            assert_ne!(TCmp::compare(&left_node.value, &node.value),
                Ordering::Greater);
            assert_eq!(left_md.parent, i);
            child_size += left_md.size;
        };

        if md.right_child >= 0 {
            let right_node = self.get_node(md.right_child);
            let right_md = right_node.md.get();
            assert_ne!(TCmp::compare(&right_node.value, &node.value),
                Ordering::Greater);

            // XXX: Ideally we would ensure that subtrees are balanced with
            // respect to each other but until we rebalance after removals this
            // will not be guaranteed
            //assert!(((child_size as i32) - (right_md.size as i32)).abs()
                //<= 1);

            assert_eq!(right_md.parent, i);
            child_size += right_md.size;
        };

        assert_eq!(md.size, child_size + 1);

        // Recursively ensure that child subtrees are valid as well
        self.validate_node(md.left_child, visited);
        self.validate_node(md.right_child, visited);
    }

    pub fn validate(&self) {
        let mut visited = HashSet::new();
        self.validate_node(self.root, &mut visited);
    }
}

impl<T, TCmp> Display for TreeHeap<T, TCmp>
        where T: Copy + Default + Display, TCmp: Comparer<T> {
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

#[derive(Clone, Copy, Default)]
struct HeapIteratorNode<T> where T: Copy + Default {
    value: T,
    md: HeapNodeMd
}

struct HeapIteratorComparator<T, TCmp>
        where T: Copy + Default, TCmp: Comparer<T> {
    phantom_t: PhantomData<T>,
    phantom_tcmp: PhantomData<TCmp>
}

impl<T, TCmp> Comparer<HeapIteratorNode<T>> for HeapIteratorComparator<T, TCmp>
        where T: Copy + Default, TCmp: Comparer<T> {
    fn compare(x: &HeapIteratorNode<T>, y: &HeapIteratorNode<T>) -> Ordering {
        TCmp::compare(&x.value, &y.value)
    }
}

pub struct HeapIterator<'a, T, TCmp>
        where T: 'a + Copy + Default, TCmp: 'a + Comparer<T> {
    heap: &'a TreeHeap<T, TCmp>,
    candidates: TreeHeap<HeapIteratorNode<T>, HeapIteratorComparator<T, TCmp>>
}

impl<'a, T, TCmp> HeapIterator<'a, T, TCmp>
        where T: 'a + Copy + Default, TCmp: 'a + Comparer<T> {
    pub fn new(heap: &'a TreeHeap<T, TCmp>) -> Self {
        let mut result = HeapIterator {
            heap: heap,
            candidates: TreeHeap::new(heap.capacity())
        };

        if let Some(n) = heap.peek() {
            result.add_candidate(heap.get_node(n.index));
        }

        result
    }

    fn add_candidate(&mut self, node: &HeapNode<T>) {
        self.candidates.insert(HeapIteratorNode {
            value: node.value,
            md: node.md.get()
        });
    }

    pub fn next(&mut self) -> Option<T> {
        if self.candidates.is_empty() {
            return None;
        }

        let top = self.candidates.pop();

        if top.md.left_child >= 0 {
            self.add_candidate(self.heap.get_node(top.md.left_child));
        }

        if top.md.right_child >= 0 {
            self.add_candidate(self.heap.get_node(top.md.right_child));
        }

        Some(top.value)
    }
}
