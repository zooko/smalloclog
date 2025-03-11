use std::fs::File;

use std::io::Write;

static mut MY_FILE_OPT: Option<File> = None;

//XXXXXX gotta add thread-safety!

pub fn write(fname: &str, bs: &[u8]) {
    unsafe {
	#[allow(static_mut_refs)]
	let f = MY_FILE_OPT.get_or_insert_with(||
					   File::create(fname).unwrap());
	f.write_all(bs).unwrap();
    }
}
