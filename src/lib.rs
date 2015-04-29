#![feature(collections)]

use std::sync::atomic::{AtomicUsize, ATOMIC_USIZE_INIT, Ordering};
use std::collections::BitSet;
use std::mem::{forget, swap, transmute, zeroed};

static INSTANCE_ID: AtomicUsize = ATOMIC_USIZE_INIT;

// (instance, index)
pub struct Index(usize, usize);

/// IndexedVec is a vector with a unique approach to indices.
/// Once an item is added to the IndexedVec, a _unique_ index is returned.
/// This index is the only way that that element may be accessed, and it
/// is garunteed by the rust typesystem for there only to be one of these
/// indices at one point in time.
///
/// This means that we can perform operations that would otherwise be unsafe in
/// a perfectly safe maner.  For example, you can grab a mutable reference
/// to an element from an immutable IndexedVec.  This is hugely useful in
/// multithreaded environments.
///
/// Because the Index is garunteed to exist and point to a valid location in
/// the backing array, the implementation of IndexedVec can also do lookups
/// without bounds checking.
pub struct IndexedVec<T> {
    // The backing vector
    mem: Vec<T>,

    // The instance ID.  Used to tell if an index came from this IndexedVec
    instance: usize,

    // A set of positions that are open.  An index can become open
    // if an item is removed from that location in the array.
    open: BitSet,
}

impl <T> IndexedVec<T> {
    /// Creates a new BoundedArray with a given size.
    pub fn new() -> IndexedVec<T> {
        IndexedVec::with_capacity(16)
    }

    pub fn with_capacity(capacity: usize) -> IndexedVec<T> {
        let instance = INSTANCE_ID.fetch_add(1, Ordering::Relaxed);
        IndexedVec {
            mem: Vec::with_capacity(capacity),
            instance: instance,
            open: BitSet::new()
        }
    }

    fn do_push(&mut self, value: T) -> Index {
        let len = self.mem.len();
        self.mem.push(value);
        Index(self.instance, len)
    }

    fn do_fill(&mut self, value: T) -> Result<Index, T> {
        let hole = self.open.iter().nth(0);
        if let Some(h) = hole {
            self.open.remove(&h);
            let arr = &mut self.mem[..];
            let mut val = value;
            unsafe {
                // This is safe because the only way that
                // `h` could get into the open set is by
                // being a valid index and being removed.
                swap(&mut val, arr.get_unchecked_mut(h));

                // This is safe because when `h` got pushed
                // into the open set, the contents were zeroed
                // so this value can not be destrucuted.
                forget(val);
            }
            Ok(Index(self.instance, h))
        } else {
            Err(value)
        }
    }

    fn assert_instance(&self, i: usize) {
        if i != self.instance {
            panic!("get() called with index that wasn't generated by the
                    this BoundedArray.");
        }
    }

    /// Adds an element to the BoundedVec.
    ///
    /// This function prefers to fill up holes in the array
    /// left by removing other items.
    pub fn add(&mut self, value: T) -> Index {
        let value = match self.do_fill(value) {
            Ok(i) => return i,
            Err(v) => v
        };

        let len = self.mem.len();
        if len == self.mem.capacity() {
            self.mem.reserve(len / 3);
        }

        self.do_push(value)
    }

    /// Adds an element to the BoundedVec.
    ///
    /// This function prefers to add elements to the 'end' of the array
    /// before filling holes. It will fill holes if otherwise a resize
    /// would be required.
    pub fn push(&mut self, value: T) -> Index {
        if self.mem.len() != self.mem.capacity() {
            self.do_push(value)
        } else {
            self.add(value)
        }
    }

    /// Returns a reference to an element in the array.
    pub fn get<'a, 'b, 'c: 'a + 'b>(&'a self, index: &'b Index) -> &'c T {
        let &Index(ins, i) = index;
        self.assert_instance(ins);

        let arr: &'a [T] = &self.mem[..];

        unsafe {
            // Safe because we are increasing the lifetime, not decreasing it.
            transmute(
                // Safe because we know that this index is
                // occupied (beacause we generated it).
                arr.get_unchecked(i))
        }
    }

    /// Returns a mutable reference to an element in the array.
    pub fn get_mut<'a, 'b, 'c: 'a + 'b>(&'a self, index: &'b mut Index) -> &'c mut T {
        let &mut Index(ins, i) = index;
        self.assert_instance(ins);

        unsafe {
            // Safe because we are only accessing the location for which
            // we are the only one that can actually access it.
            let arr: &mut [T] = transmute(&self.mem[..]);

            // Safe because we are just using this to increase the lifetime
            // bound from 'b, to 'c, not.
            transmute(
                // Safe because we know that this index is
                // occupied (beacause we generated it).
                arr.get_unchecked_mut(i))
        }
    }

    /// Swaps the element at an index, returning the previous value.
    pub fn swap(&self, index: &mut Index, mut value: T) -> T {
        self.assert_instance(index.0);
        swap(self.get_mut(index), &mut value);
        value
    }

    /// Remove the element stored at Index location, returning it.
    pub fn take(&mut self, index: Index) -> T {
        let Index(ins, i) = index;
        self.assert_instance(ins);

        let mut copy = Index(ins, i);

        let mut out = unsafe { zeroed() };

        {
            let inside = self.get_mut(&mut copy);
            swap(&mut out, inside);
        }

        self.open.insert(i);

        out
    }

    /// Removes the element stored at Index location, dropping it.
    pub fn remove(&mut self, index: Index) {
        self.take(index);
    }
}

impl <T> Drop for IndexedVec<T> {
    fn drop(&mut self) {
        for (i, v) in self.mem.drain().enumerate() {
            if self.open.contains(&i) {
                unsafe{ forget(v); }
            } else {
                drop(v);
            }
        }
    }
}
