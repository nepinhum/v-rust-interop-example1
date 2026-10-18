module kvstore

// Store is a handle to a key-value store that lives in Rust memory.
//
// V's GC can't see that memory, therefore free() must be called explicitly. Every
// method panics after free(): using a freed handle is a bug in the caller.
//
// A Store must not be copied by value. V has no non-copyable types and a copy
// would keep the raw handle after the original is freed; the nil check below
// only protects the instance free() was called on. Pass &Store around.
@[heap; noinit]
pub struct Store {
mut:
	handle &C.kv_store
}

pub fn Store.new() &Store {
	handle := C.kv_store_new()
	if handle == unsafe { nil } {
		panic('kvstore: kv_store_new failed')
	}
	return &Store{
		handle: handle
	}
}

// free releases the Rust store. Calling it again is a noop. Calling it from
// inside an each() callback panics since the iteration still uses the store.
pub fn (mut s Store) free() {
	if s.handle == unsafe { nil } {
		return
	}
	status := C.kv_store_free(s.handle)
	match status {
		.ok {}
		.busy { panic('kvstore: free() during each()') }
		else { fatal('free', status) }
	}
	s.handle = unsafe { nil }
}

// set stores a copy of key and value. It fails only when called from inside
// an each() callback on the same store.
pub fn (mut s Store) set(key string, value string) ! {
	status := C.kv_store_set(s.live(), key.str, usize(key.len), value.str, usize(value.len))
	match status {
		.ok { return }
		.busy { return error('kvstore: set() during each()') }
		else { fatal('set', status) }
	}
}

// get returns a V-owned copy of the value or none if the key is absent.
pub fn (s &Store) get(key string) ?string {
	mut out := C.kv_bytes{}
	status := C.kv_store_get(s.live(), key.str, usize(key.len), &out)
	match status {
		.ok {}
		.not_found { return none }
		else { fatal('get', status) }
	}
	value := copy_bytes(out.ptr, out.len)
	C.kv_bytes_free(out)
	return value
}

// remove reports whether the key existed. It fails only when called from
// inside an each() callback on the same store.
pub fn (mut s Store) remove(key string) !bool {
	status := C.kv_store_remove(s.live(), key.str, usize(key.len))
	match status {
		.ok { return true }
		.not_found { return false }
		.busy { return error('kvstore: remove() during each()') }
		else { fatal('remove', status) }
	}
	return false
}

pub fn (s &Store) len() int {
	return int(C.kv_store_len(s.live()))
}

struct Visitor {
	f fn (key string, value string) @[required]
}

// visit is the C-ABI trampoline Rust calls back into.
fn visit(user_data voidptr, key &u8, key_len usize, value &u8, value_len usize) {
	v := unsafe { &Visitor(user_data) }
	v.f(copy_bytes(key, key_len), copy_bytes(value, value_len))
}

// each calls 'f' for every entry in ascending key order. Inside 'f', get() and
// len() work; set() and remove() return an error; free() panics.
pub fn (s &Store) each(f fn (key string, value string)) {
	ctx := Visitor{
		f: f
	}
	status := C.kv_store_for_each(s.live(), visit, &ctx)
	if status != .ok {
		fatal('each', status)
	}
}

fn (s &Store) live() &C.kv_store {
	if s.handle == unsafe { nil } {
		panic('kvstore: use of a freed Store')
	}
	return s.handle
}

fn copy_bytes(ptr &u8, len usize) string {
	if len > usize(max_i32) {
		panic('kvstore: ${len} bytes do not fit in a V string')
	}
	return unsafe { tos(ptr, int(len)) }.clone()
}

// null_arg and panic mean a bug on one side of the boundary.
@[noreturn]
fn fatal(op string, status Status) {
	panic('kvstore: ${op} failed with ${status}')
}
