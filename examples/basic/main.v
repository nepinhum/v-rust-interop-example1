import kvstore

fn main() {
	mut store := kvstore.Store.new()
	defer {
		store.free()
	}

	store.set('lang', 'V')!
	store.set('backend', 'Rust')!
	store.set('glue', 'C ABI')!

	if value := store.get('backend') {
		println('backend = ${value}')
	}
	if _ := store.get('gc') {
		println('unexpected')
	} else {
		println('gc      = <none>')
	}

	// Rust drives the loop and calls back into this V closure per entry.
	store.each(fn (key string, value string) {
		println('  ${key}: ${value}')
	})
	println('${store.len()} entries')
}
