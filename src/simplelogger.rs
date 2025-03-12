use std::fs::File;

use std::io::Write;

static mut MY_FILE_OPT: Option<File> = None;

pub fn write(bs: &[u8]) {
    // This function is called only from within a lock (a dumb compare-and-exchange spinlock) held by smalloclog. So we can use non-atomic test and set on MY_FILE_OPT.
    unsafe {
	#[allow(static_mut_refs)]
	let f = MY_FILE_OPT.get_or_insert_with(||
					       File::create("smalloclog.log").unwrap());
	f.write_all(bs).unwrap();
    }
}
