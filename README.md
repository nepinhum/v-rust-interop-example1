# Rust and V - interoperability without special language support

A small experiment: a key-value store written in Rust, used from V like any other V type.

There are no bindings generators or compiler plugins here and nothing was patched in either language.
Rust exports a few plain C functions and V calls them the same way it calls any C library.

```v
import kvstore

mut store := kvstore.Store.new()
defer { store.free() }

store.set('lang', 'V')!
println(store.get('lang') or { 'missing' })

store.each(fn (key string, value string) {
	println('${key}: ${value}')
})
```

## Running it

You need Rust, V and gcc. Only tested on GNU/Linux (more on that below).

```sh
cargo build --release   # builds target/release/libkvstore.a
v run examples/basic
v test kvstore/
cargo test
```

You have to rebuild after touching `src/lib.rs`.

`examples/play` is a scratch program if you want to poke at things by hand.

## What's where

```
src/lib.rs          the Rust store and its C functions
include/kvstore.h   the C header both sides agree on (hand written)
kvstore/ffi.c.v     V's view of that header
kvstore/store.v     the actual V API
examples/           basic usage + a playground
```

## How it works

Rust keeps the data. V only ever holds an opaque pointer to it, wrapped in a `Store` struct.

**Strings go across as pointer + length, never as C strings.** V strings can
contain zero bytes and aren't guaranteed to end with one, so Rust just stores
raw bytes.

**Each side frees what it allocated.** When you call `get()`, Rust hands back
a buffer it allocated. V copies it into a normal V string and immediately
gives the buffer back to Rust to free. You never end up with a V string
pointing into Rust memory.

**Rust panics stay in Rust.** Every exported function catches panics and
returns an error code instead of unwinding into V.

**Using a freed store panics instead of crashing.** `free()` clears the
handle and a later call gives you a clear V panic rather than a use-after-free.
Calling `free()` twice is fine.

**Callbacks.** `each()` is the fun part: Rust walks the map and calls back into
V for every entry. What happens if you touch the store from inside that
callback?

- `get()` and `len()` work fine
- `set()` and `remove()` return an error
- `free()` panics

None of these are undefined behaviour. The Rust side tracks this with a `RefCell` and the Rust tests also pass under Miri.

## Gotchas

- **Don't copy a `Store` by value.** V can't forbid it and the copy would
  still hold the old pointer after the original is freed. Pass `&Store`.
- **You have to call `free()` yourself.** V's GC doesn't know about Rust's
  memory.
- **tcc can't link it (at least on Arch).** Rust needs `libgcc_s` and gcc
  keeps it somewhere tcc doesn't look. V notices and falls back to gcc on its
  own but prints a warning. Use `-cc gcc` to skip it.
- **Linux only.** The linker flags come from
  `cargo rustc --release -- --print native-static-libs` and they're
  different on every OS (Windows even names the file `kvstore.lib`). On
  anything else the V build stops with an error up front. Porting would mean
  adding that platform's flags to `kvstore/ffi.c.v`.
- **Not thread safe.** One thread at a time.
