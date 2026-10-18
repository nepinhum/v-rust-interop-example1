// Scratch program for poking at the store by hand.
import kvstore

fn main() {
	mut s := kvstore.Store.new()
	defer {
		s.free()
	}

	s.set('a', '1')!
	s.set('b', '2')!
	println(s.get('a') or { 'none' })
	println(s.get('zzz') or { 'none' })
	println(s.remove('a')!)
	println(s.len())

	// Calling back into the store from inside each():
	s.each(fn [mut s] (key string, value string) {
		println('visit ${key}=${value}, get=${s.get(key) or { '?' }}')
		s.set('x', 'y') or { println('  set refused: ${err}') }
	})
}
