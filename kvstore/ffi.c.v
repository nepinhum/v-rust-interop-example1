module kvstore

// The static archive is produced by cargo build --release at the repo root.
// A Rust staticlib bundles std; these are the native libs it needs, from
// cargo rustc --release -- --print native-static-libs. Both the archive name
// and that list are target specific and only GNU/Linux has been verified.
$if !linux {
	$compile_error('kvstore: only GNU/Linux is supported; see README')
}

#flag -I @DIR/../include
#flag linux @DIR/../target/release/libkvstore.a
#flag linux -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc
#include "kvstore.h"

@[typedef]
struct C.kv_store {}

@[typedef]
struct C.kv_bytes {
	ptr &u8
	len usize
}

// Mirrors kv_status. Declaring the C functions as returning this enum lets V
// match on the result directly. Both sides are int-sized C enums.
enum Status {
	ok        = 0
	not_found = 1
	null_arg  = 2
	busy      = 3
	panic     = 4
}

type VisitFn = fn (user_data voidptr, key &u8, key_len usize, value &u8, value_len usize)

fn C.kv_store_new() &C.kv_store
fn C.kv_store_free(store &C.kv_store) Status
fn C.kv_store_set(store &C.kv_store, key &u8, key_len usize, value &u8, value_len usize) Status
fn C.kv_store_get(store &C.kv_store, key &u8, key_len usize, out &C.kv_bytes) Status
fn C.kv_store_remove(store &C.kv_store, key &u8, key_len usize) Status
fn C.kv_store_len(store &C.kv_store) usize
fn C.kv_store_for_each(store &C.kv_store, visit VisitFn, user_data voidptr) Status
fn C.kv_bytes_free(bytes C.kv_bytes)
