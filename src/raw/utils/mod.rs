mod parker;
pub use parker::Parker;

use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicIsize, AtomicPtr, Ordering};

// Polyfill for the unstable strict-provenance APIs.
#[allow(clippy::missing_safety_doc)]
#[allow(dead_code)] // `strict_provenance` has stabilized on nightly.
pub unsafe trait StrictProvenance<T>: Sized {
    fn addr(self) -> usize;
    fn map_addr(self, f: impl FnOnce(usize) -> usize) -> Self;
    fn unpack(self) -> Tagged<T>
    where
        T: Unpack;
}

// Unpack a tagged pointer.
pub trait Unpack {
    // A mask for the pointer tag bits.
    const MASK: usize;
}

unsafe impl<T> StrictProvenance<T> for *mut T {
    #[inline(always)]
    fn addr(self) -> usize {
        self as usize
    }

    #[inline(always)]
    fn map_addr(self, f: impl FnOnce(usize) -> usize) -> Self {
        f(self.addr()) as Self
    }

    #[inline(always)]
    fn unpack(self) -> Tagged<T>
    where
        T: Unpack,
    {
        Tagged {
            raw: self,
            ptr: self.map_addr(|addr| addr & T::MASK),
        }
    }
}

// An unpacked tagged pointer.
pub struct Tagged<T> {
    // The raw tagged pointer.
    pub raw: *mut T,

    // The untagged pointer.
    pub ptr: *mut T,
}

// Creates a `Tagged` from an untagged pointer.
#[inline]
pub fn untagged<T>(value: *mut T) -> Tagged<T> {
    Tagged {
        raw: value,
        ptr: value,
    }
}

impl<T> Tagged<T>
where
    T: Unpack,
{
    // Returns the tag portion of this pointer.
    #[inline]
    pub fn tag(self) -> usize {
        self.raw.addr() & !T::MASK
    }

    // Maps the tag of this pointer.
    #[inline]
    pub fn map_tag(self, f: impl FnOnce(usize) -> usize) -> Self {
        Tagged {
            raw: self.raw.map_addr(f),
            ptr: self.ptr,
        }
    }
}

impl<T> Copy for Tagged<T> {}

impl<T> Clone for Tagged<T> {
    fn clone(&self) -> Self {
        *self
    }
}

// Polyfill for the unstable `atomic_ptr_strict_provenance` APIs.
pub trait AtomicPtrFetchOps<T> {
    fn fetch_or(&self, value: usize, ordering: Ordering) -> *mut T;
}

impl<T> AtomicPtrFetchOps<T> for AtomicPtr<T> {
    #[inline]
    fn fetch_or(&self, value: usize, ordering: Ordering) -> *mut T {
        #[cfg(not(miri))]
        {
            use std::sync::atomic::AtomicUsize;

            // Safety: `AtomicPtr` and `AtomicUsize` are identical in terms
            // of memory layout. This operation is technically invalid in that
            // it loses provenance, but there is no stable alternative.
            unsafe { &*(self as *const AtomicPtr<T> as *const AtomicUsize) }
                .fetch_or(value, ordering) as *mut T
        }

        // Avoid ptr2int under Miri.
        #[cfg(miri)]
        {
            // Returns the ordering for the read in an RMW operation.
            const fn read_ordering(ordering: Ordering) -> Ordering {
                match ordering {
                    Ordering::SeqCst => Ordering::SeqCst,
                    Ordering::AcqRel => Ordering::Acquire,
                    _ => Ordering::Relaxed,
                }
            }

            self.fetch_update(ordering, read_ordering(ordering), |ptr| {
                Some(ptr.map_addr(|addr| addr | value))
            })
            .unwrap()
        }
    }
}

/// Pads and aligns a value to the length of a cache line.
#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
// Source: https://github.com/crossbeam-rs/crossbeam/blob/master/crossbeam-utils/src/cache_padded.rs#L63.
#[cfg_attr(
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "powerpc64",
    ),
    repr(align(128))
)]
#[cfg_attr(
    any(
        target_arch = "arm",
        target_arch = "mips",
        target_arch = "mips32r6",
        target_arch = "mips64",
        target_arch = "mips64r6",
        target_arch = "riscv64",
    ),
    repr(align(32))
)]
#[cfg_attr(target_arch = "s390x", repr(align(256)))]
#[cfg_attr(
    not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "powerpc64",
        target_arch = "arm",
        target_arch = "mips",
        target_arch = "mips32r6",
        target_arch = "mips64",
        target_arch = "mips64r6",
        target_arch = "riscv64",
        target_arch = "s390x",
    )),
    repr(align(64))
)]
pub struct CachePadded<T> {
    value: T,
}

// A sharded atomic counter.
pub struct Counter(Box<[CachePadded<AtomicIsize>]>);

impl Default for Counter {
    fn default() -> Counter {
        let num_cpus = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let shards = (0..num_cpus.next_power_of_two())
            .map(|_| Default::default())
            .collect();
        Counter(shards)
    }
}

impl Counter {
    // Return the shard for the given thread ID.
    #[inline]
    pub fn get(&self, thread: usize) -> &AtomicIsize {
        &self.0[thread & (self.0.len() - 1)].value
    }

    // Returns the sum of all counter shards.
    #[inline]
    pub fn sum(&self) -> usize {
        self.0
            .iter()
            .map(|x| x.value.load(Ordering::Relaxed))
            .sum::<isize>()
            .try_into()
            // Depending on the order of deletion/insertions this might be negative,
            // so assume the map is empty.
            .unwrap_or(0)
    }
}

// `Box<T>` but aliasable.
pub struct Shared<T>(NonNull<T>);

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Shared<T> {
        Shared(unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(value))) })
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        // Safety: `self.0` was allocated with `Box` and is never shared.
        unsafe { &*self.0.as_ptr() }
    }
}

impl<T> Drop for Shared<T> {
    #[inline]
    fn drop(&mut self) {
        // Safety: `self.0` was allocated with `Box` and is never shared,
        // and we have unique access to `self`.
        let _ = unsafe { Box::from_raw(self.0.as_ptr()) };
    }
}

/// A `seize::Guard` that has been verified to belong to a given map.
pub trait VerifiedGuard: seize::Guard {}

#[repr(transparent)]
pub struct MapGuard<G>(G);

impl<G> MapGuard<G> {
    /// Create a new `MapGuard`.
    ///
    /// # Safety
    ///
    /// The guard must be valid to use with the given map.
    pub unsafe fn new(guard: G) -> MapGuard<G> {
        MapGuard(guard)
    }

    /// Create a new `MapGuard` from a reference.
    ///
    /// # Safety
    ///
    /// The guard must be valid to use with the given map.
    pub unsafe fn from_ref(guard: &G) -> &MapGuard<G> {
        // Safety: `VerifiedGuard` is `repr(transparent)` over `G`.
        unsafe { &*(guard as *const G as *const MapGuard<G>) }
    }
}

impl<G> VerifiedGuard for MapGuard<G> where G: seize::Guard {}

impl<G> seize::Guard for MapGuard<G>
where
    G: seize::Guard,
{
    #[inline]
    fn refresh(&mut self) {
        self.0.refresh();
    }

    #[inline]
    fn flush(&self) {
        self.0.flush();
    }

    #[inline]
    fn protect<T: seize::AsLink>(&self, ptr: &AtomicPtr<T>, ordering: Ordering) -> *mut T {
        self.0.protect(ptr, ordering)
    }

    #[inline]
    unsafe fn defer_retire<T: seize::AsLink>(
        &self,
        ptr: *mut T,
        reclaim: unsafe fn(*mut seize::Link),
    ) {
        unsafe { self.0.defer_retire(ptr, reclaim) };
    }

    #[inline]
    fn thread_id(&self) -> usize {
        self.0.thread_id()
    }

    #[inline]
    fn belongs_to(&self, collector: &seize::Collector) -> bool {
        self.0.belongs_to(collector)
    }

    #[inline]
    fn link(&self, collector: &seize::Collector) -> seize::Link {
        self.0.link(collector)
    }
}
