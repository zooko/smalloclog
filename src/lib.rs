use core::{
    alloc::{GlobalAlloc, Layout}
};

use std::alloc::System;

mod simplelogger;

use std::sync::atomic::{AtomicBool, Ordering};

static LOCK: AtomicBool = AtomicBool::new(false);

pub struct SmallocLog { }

//XXXX To read this file correctly is going to be impossible without making an assumption about the usize on the source machine...
#[inline(always)]
fn log_layout(layout: core::alloc::Layout) {
    simplelogger::write(&layout.size().to_le_bytes());
    simplelogger::write(&layout.align().to_le_bytes());
}

//XXXX To read this file correctly is impossible without making an assumption about the usize on the source machine...
const SO_U_SRC: usize = 8;
const SO_P_SRC: usize = 8;

unsafe impl GlobalAlloc for SmallocLog {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
	// Spin until this thread gets the exclusive ownership of LOCK:
	loop {
	    let result = LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed);
	    match result {
		Ok(_x) => { /* XXX */ break; },
		Err(_x) =>  { /* XXX */ }
	    }
	}

	//XXXwhile LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).unwrap() { }

	simplelogger::write(b"a");
	log_layout(layout);
	let p = unsafe { System.alloc(layout) };
	simplelogger::write(&p.addr().to_le_bytes());

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);

	p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
	// Spin until this thread gets the exclusive ownership of LOCK:
	loop {
	    let result = LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed);
	    match result {
		Ok(_x) => { /* XXX */break; },
		Err(_x) =>  { /* XXX */ }
	    }
	}

	simplelogger::write(b"d");
	simplelogger::write(&ptr.addr().to_le_bytes());

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);

	unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
	// Spin until this thread gets the exclusive ownership of LOCK:
	loop {
	    let result = LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed);
	    match result {
		Ok(_x) => { /* XXX */ break; },
		Err(_x) =>  { /* XXX */ }
	    }
	}

	simplelogger::write(b"r");
	simplelogger::write(&ptr.addr().to_le_bytes());
	log_layout(layout);
	simplelogger::write(&new_size.to_le_bytes());
	let newptr = unsafe { System.realloc(ptr, layout, new_size) };
	simplelogger::write(&newptr.addr().to_le_bytes());

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);

	newptr
    }
}

use std::fmt::Write;
pub fn smalloclog_to_human_readable(sml: &[u8]) -> String {
    let mut i: usize = 0; // index of next byte to read
    let mut res: String = String::new();

    while i < sml.len() {
	if sml[i] == b'a' {
	    i += 1;
	    let siz: usize = usize::from_le_bytes(sml[i..i+SO_U_SRC].try_into().unwrap());
	    i += SO_U_SRC;
	    let ali: usize = usize::from_le_bytes(sml[i..i+SO_U_SRC].try_into().unwrap());
	    i += SO_U_SRC;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+SO_P_SRC].try_into().unwrap());
	    i += SO_P_SRC;
	    writeln!(res, "alloc({}, {}) -> 0x{:x}", siz, ali, ptr).ok();
	} else if sml[i] == b'd' {
	    i += 1;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+SO_P_SRC].try_into().unwrap());
	    i += SO_P_SRC;
	    writeln!(res, "dealloc(0x{:x})", ptr).ok();
	} else if sml[i] == b'r' {
	    i += 1;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+SO_P_SRC].try_into().unwrap());
	    i += SO_P_SRC;
	    let siz: usize = usize::from_le_bytes(sml[i..i+SO_U_SRC].try_into().unwrap());
	    i += SO_U_SRC;
	    let ali: usize = usize::from_le_bytes(sml[i..i+SO_U_SRC].try_into().unwrap());
	    i += SO_U_SRC;
	    let newsiz: usize = usize::from_le_bytes(sml[i..i+SO_U_SRC].try_into().unwrap());
	    i += SO_U_SRC;
	    let newptr: usize = usize::from_le_bytes(sml[i..i+SO_P_SRC].try_into().unwrap());
	    i += SO_P_SRC;
	    writeln!(res, "realloc(0x{:x}, {}, {}, {}) -> 0x{:x}", ptr, siz, ali, newsiz, newptr).ok();
	} else {
	    panic!("Found something unexpected in smalloclog. sml[{}]: {}, first part: {}, remainder: 0x{:?}", i,  sml[i], res, &sml[i..]);
	}
    }

    res
}

#[cfg(test)]
mod tests {

    #[test]
    fn read_file_1() {
    //XXX    let file_path = "test_file.smalloclog";
//XXX        let expected_contents = b"";

    }

}

