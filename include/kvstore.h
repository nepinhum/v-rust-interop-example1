// C contract for the Rust key-value store in src/lib.rs.
// It's hand written; keep in sync with the #[repr(C)] types and exports there.
#ifndef KVSTORE_H
#define KVSTORE_H

#include <stddef.h>
#include <stdint.h>

typedef struct kv_store kv_store;

typedef enum {
	KV_OK = 0,
	KV_NOT_FOUND = 1,
	KV_NULL_ARG = 2,
	KV_BUSY = 3, // mutation or free attempted during kv_store_for_each
	KV_PANIC = 4, // Rust panicked; the panic was caught at the boundary
} kv_status;

// Owned by Rust. Release with kv_bytes_free, never with free().
typedef struct {
	uint8_t *ptr;
	size_t len;
} kv_bytes;

// key/value are borrowed and valid only during the call.
typedef void (*kv_visit)(void *user_data, const uint8_t *key, size_t key_len,
	const uint8_t *value, size_t value_len);

// Buffers passed in are copied; the caller keeps ownership. A zero length
// accepts any pointer, including NULL.
kv_store *kv_store_new(void);
kv_status kv_store_free(kv_store *store);
kv_status kv_store_set(const kv_store *store, const uint8_t *key, size_t key_len,
	const uint8_t *value, size_t value_len);
kv_status kv_store_get(const kv_store *store, const uint8_t *key, size_t key_len,
	kv_bytes *out);
kv_status kv_store_remove(const kv_store *store, const uint8_t *key, size_t key_len);
size_t kv_store_len(const kv_store *store);
kv_status kv_store_for_each(const kv_store *store, kv_visit visit, void *user_data);
void kv_bytes_free(kv_bytes bytes);

#endif
