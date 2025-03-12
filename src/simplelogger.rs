use std::fs::File;

use std::io::Write;

use std::sync::atomic::{AtomicBool, Ordering};



static LOCK: AtomicBool = AtomicBool::new(false);

static mut MY_FILE_OPT: Option<File> = None;

pub fn write(fname: &str, bs: &[u8]) {
    unsafe {
	// Spin until this thread gets the exclusive ownership of LOCK:
	while LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).unwrap() { }

	#[allow(static_mut_refs)]
	let f = MY_FILE_OPT.get_or_insert_with(||
					   File::create(fname).unwrap());
	f.write_all(bs).unwrap();

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);
    }
}
