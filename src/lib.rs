//! A byte oriented key-value store exposed through a C ABI.
//!
//! The C contract lives in `include/kvstore.h`. Every exported function
//! catches panics and reports them as `Status::Panic`, so no unwind ever
//! crosses into the caller's frames.
//!
//! The store is only ever accessed through shared references. Mutation goes
//! through a `RefCell` which turns reentrant mutation from a visitor callback
//! (e.g. `kv_store_set` called while `kv_store_for_each` is iterating) into a
//! `Status::Busy` instead of aliased `&mut` undefined behaviour.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::{ptr, slice};

pub struct Store {
    map: RefCell<BTreeMap<Vec<u8>, Vec<u8>>>,
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    NotFound = 1,
    NullArg = 2,
    Busy = 3,
    Panic = 4,
}

/// A Rust-owned byte buffer handed to the caller. Release with `kv_bytes_free`.
#[repr(C)]
pub struct Bytes {
    ptr: *mut u8,
    len: usize,
}

/// Called once per entry. The key and value pointers are borrowed from the
/// store and are valid only for the duration of the call.
pub type Visit = unsafe extern "C" fn(
    user_data: *mut c_void,
    key: *const u8,
    key_len: usize,
    value: *const u8,
    value_len: usize,
);

fn guard(f: impl FnOnce() -> Status) -> Status {
    // A panic can only come from allocation or a bug in this
    // file; the map is never left half updated by the operations below.
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(Status::Panic)
}

/// # Safety
/// `store` must be null or a live pointer returned by `kv_store_new`.
unsafe fn store_ref<'a>(store: *const Store) -> Option<&'a Store> {
    unsafe { store.as_ref() }
}

/// # Safety
/// If `len > 0`, `ptr` must be valid for reads of `len` bytes for `'a`.
unsafe fn byte_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        Some(&[])
    } else if ptr.is_null() {
        None
    } else {
        Some(unsafe { slice::from_raw_parts(ptr, len) })
    }
}

/// Returns null only if a panic occurred during construction.
#[unsafe(no_mangle)]
pub extern "C" fn kv_store_new() -> *mut Store {
    catch_unwind(|| {
        Box::into_raw(Box::new(Store {
            map: RefCell::new(BTreeMap::new()),
        }))
    })
    .unwrap_or(ptr::null_mut())
}

/// Refuses with `Busy` while the store is being iterated which keeps a
/// visitor from freeing the store out from under `kv_store_for_each`.
///
/// # Safety
/// `store` must be null or a live pointer returned by `kv_store_new`.
/// After `Ok`, the pointer is dangling and must not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kv_store_free(store: *mut Store) -> Status {
    guard(|| {
        let Some(s) = (unsafe { store_ref(store) }) else {
            return Status::NullArg;
        };
        if s.map.try_borrow_mut().is_err() {
            return Status::Busy;
        }
        drop(unsafe { Box::from_raw(store) });
        Status::Ok
    })
}

/// # Safety
/// `store` must be null or live. `key`/`value` must be valid for reads of
/// their lengths. Both buffers are copied; the caller keeps ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kv_store_set(
    store: *const Store,
    key: *const u8,
    key_len: usize,
    value: *const u8,
    value_len: usize,
) -> Status {
    guard(|| {
        let (Some(s), Some(k), Some(v)) = (unsafe {
            (
                store_ref(store),
                byte_slice(key, key_len),
                byte_slice(value, value_len),
            )
        }) else {
            return Status::NullArg;
        };
        let Ok(mut map) = s.map.try_borrow_mut() else {
            return Status::Busy;
        };
        map.insert(k.to_vec(), v.to_vec());
        Status::Ok
    })
}

/// On `Ok`, `*out` receives a copy of the value that the caller must release
/// with `kv_bytes_free`. On any other status `*out` is left untouched.
///
/// # Safety
/// `store` must be null or live, `key` valid for `key_len` bytes and `out`
/// null or valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kv_store_get(
    store: *const Store,
    key: *const u8,
    key_len: usize,
    out: *mut Bytes,
) -> Status {
    guard(|| {
        let (Some(s), Some(k)) = (unsafe { (store_ref(store), byte_slice(key, key_len)) }) else {
            return Status::NullArg;
        };
        if out.is_null() {
            return Status::NullArg;
        }
        // A shared borrow: lookups are allowed from inside a visitor.
        let map = s.map.borrow();
        let Some(v) = map.get(k) else {
            return Status::NotFound;
        };
        let boxed: Box<[u8]> = v.as_slice().into();
        let len = boxed.len();
        let ptr = Box::into_raw(boxed) as *mut u8;
        unsafe { out.write(Bytes { ptr, len }) };
        Status::Ok
    })
}

/// # Safety
/// `store` must be null or live, `key` valid for `key_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kv_store_remove(
    store: *const Store,
    key: *const u8,
    key_len: usize,
) -> Status {
    guard(|| {
        let (Some(s), Some(k)) = (unsafe { (store_ref(store), byte_slice(key, key_len)) }) else {
            return Status::NullArg;
        };
        let Ok(mut map) = s.map.try_borrow_mut() else {
            return Status::Busy;
        };
        match map.remove(k) {
            Some(_) => Status::Ok,
            None => Status::NotFound,
        }
    })
}

/// Returns 0 for a null store.
///
/// # Safety
/// `store` must be null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kv_store_len(store: *const Store) -> usize {
    catch_unwind(AssertUnwindSafe(|| {
        unsafe { store_ref(store) }.map_or(0, |s| s.map.borrow().len())
    }))
    .unwrap_or(0)
}

/// Visits entries in ascending key order. While iterating, `kv_store_set`,
/// `kv_store_remove` and `kv_store_free` on the same store return `Busy`;
/// `kv_store_get` and `kv_store_len` work normally.
///
/// # Safety
/// `store` must be null or live. `visit` must be safe to call with
/// `user_data` and must not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kv_store_for_each(
    store: *const Store,
    visit: Option<Visit>,
    user_data: *mut c_void,
) -> Status {
    guard(|| {
        let (Some(s), Some(visit)) = (unsafe { store_ref(store) }, visit) else {
            return Status::NullArg;
        };
        let map = s.map.borrow();
        for (k, v) in map.iter() {
            unsafe { visit(user_data, k.as_ptr(), k.len(), v.as_ptr(), v.len()) };
        }
        Status::Ok
    })
}

/// Releases a buffer returned by `kv_store_get`. A null `ptr` is a noop.
///
/// # Safety
/// `bytes` must come from `kv_store_get` and not have been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kv_bytes_free(bytes: Bytes) {
    if bytes.ptr.is_null() {
        return;
    }
    let raw = ptr::slice_from_raw_parts_mut(bytes.ptr, bytes.len);
    drop(unsafe { Box::from_raw(raw) });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(s: *const Store, k: &[u8], v: &[u8]) -> Status {
        unsafe { kv_store_set(s, k.as_ptr(), k.len(), v.as_ptr(), v.len()) }
    }

    fn get(s: *const Store, k: &[u8]) -> Result<Vec<u8>, Status> {
        let mut out = Bytes {
            ptr: ptr::null_mut(),
            len: 0,
        };
        match unsafe { kv_store_get(s, k.as_ptr(), k.len(), &mut out) } {
            Status::Ok => {
                let v = unsafe { slice::from_raw_parts(out.ptr, out.len) }.to_vec();
                unsafe { kv_bytes_free(out) };
                Ok(v)
            }
            status => Err(status),
        }
    }

    #[test]
    fn round_trip_and_overwrite() {
        let s = kv_store_new();
        assert_eq!(set(s, b"k", b"one"), Status::Ok);
        assert_eq!(set(s, b"k", b"two"), Status::Ok);
        assert_eq!(get(s, b"k"), Ok(b"two".to_vec()));
        assert_eq!(get(s, b"missing"), Err(Status::NotFound));
        assert_eq!(unsafe { kv_store_len(s) }, 1);
        assert_eq!(unsafe { kv_store_remove(s, b"k".as_ptr(), 1) }, Status::Ok);
        assert_eq!(
            unsafe { kv_store_remove(s, b"k".as_ptr(), 1) },
            Status::NotFound
        );
        assert_eq!(unsafe { kv_store_free(s) }, Status::Ok);
    }

    #[test]
    fn empty_and_binary_data() {
        let s = kv_store_new();
        assert_eq!(
            unsafe { kv_store_set(s, ptr::null(), 0, ptr::null(), 0) },
            Status::Ok
        );
        assert_eq!(get(s, b""), Ok(vec![]));
        assert_eq!(set(s, b"a\0b\xff", b"\0\0"), Status::Ok);
        assert_eq!(get(s, b"a\0b\xff"), Ok(b"\0\0".to_vec()));
        assert_eq!(get(s, b"a"), Err(Status::NotFound));
        unsafe { kv_store_free(s) };
    }

    #[test]
    fn null_arguments() {
        let s = kv_store_new();
        assert_eq!(set(ptr::null(), b"k", b"v"), Status::NullArg);
        assert_eq!(
            unsafe { kv_store_set(s, ptr::null(), 3, b"v".as_ptr(), 1) },
            Status::NullArg
        );
        assert_eq!(
            unsafe { kv_store_get(s, b"k".as_ptr(), 1, ptr::null_mut()) },
            Status::NullArg
        );
        assert_eq!(
            unsafe { kv_store_for_each(s, None, ptr::null_mut()) },
            Status::NullArg
        );
        assert_eq!(unsafe { kv_store_free(ptr::null_mut()) }, Status::NullArg);
        assert_eq!(unsafe { kv_store_len(ptr::null()) }, 0);
        unsafe { kv_store_free(s) };
    }

    struct Reentry {
        store: *mut Store,
        seen: Vec<(Vec<u8>, Vec<u8>)>,
        set: Vec<Status>,
        remove: Vec<Status>,
        free: Vec<Status>,
        get: Vec<Result<Vec<u8>, Status>>,
    }

    unsafe extern "C" fn reenter(
        user_data: *mut c_void,
        k: *const u8,
        kl: usize,
        v: *const u8,
        vl: usize,
    ) {
        let r = unsafe { &mut *(user_data as *mut Reentry) };
        let (k, v) = unsafe { (slice::from_raw_parts(k, kl), slice::from_raw_parts(v, vl)) };
        r.seen.push((k.to_vec(), v.to_vec()));
        r.set.push(set(r.store, b"new", b"x"));
        r.remove
            .push(unsafe { kv_store_remove(r.store, k.as_ptr(), k.len()) });
        r.free.push(unsafe { kv_store_free(r.store) });
        r.get.push(get(r.store, k));
    }

    #[test]
    fn reentry_from_visitor_is_refused_not_ub() {
        let s = kv_store_new();
        set(s, b"b", b"2");
        set(s, b"a", b"1");
        let mut r = Reentry {
            store: s,
            seen: vec![],
            set: vec![],
            remove: vec![],
            free: vec![],
            get: vec![],
        };
        let status =
            unsafe { kv_store_for_each(s, Some(reenter), &mut r as *mut _ as *mut c_void) };
        assert_eq!(status, Status::Ok);
        assert_eq!(
            r.seen,
            vec![
                (b"a".to_vec(), b"1".to_vec()),
                (b"b".to_vec(), b"2".to_vec())
            ]
        );
        assert_eq!(r.set, vec![Status::Busy, Status::Busy]);
        assert_eq!(r.remove, vec![Status::Busy, Status::Busy]);
        assert_eq!(r.free, vec![Status::Busy, Status::Busy]);
        assert_eq!(r.get, vec![Ok(b"1".to_vec()), Ok(b"2".to_vec())]);
        assert_eq!(set(s, b"new", b"x"), Status::Ok);
        assert_eq!(unsafe { kv_store_free(s) }, Status::Ok);
    }

    #[test]
    fn guard_turns_panic_into_status() {
        assert_eq!(guard(|| panic!("boom")), Status::Panic);
    }
}
