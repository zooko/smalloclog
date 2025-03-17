use std::fs::{File};
use std::io::Write;

use core::{
    alloc::{GlobalAlloc, Layout}
};

use std::alloc::System;

use std::sync::atomic::{AtomicBool, Ordering};

static LOCK: AtomicBool = AtomicBool::new(false);

pub struct SmallocLog { }


use std::primitive::usize;
const U_U8: u8 = (std::primitive::usize::BITS / 8) as u8; // number of bytes for a usize
const U: usize = U_U8 as usize;
static mut MY_FILE_OPT: Option<File> = None;
#[allow(static_mut_refs)] // This function is called only from within a lock (a dumb compare-and-exchange spinlock). So we can use non-atomic test and set on MY_FILE_OPT.
fn create_my_file() -> () {
    // If MY_FILE_OPT doesn't already have a File in it, create a File, write the smalloclog header to it, and store it in MY_FILE_OPT.

    unsafe {
	if MY_FILE_OPT.is_none() {
	    let mut new_file = File::create("smalloclog.log").unwrap();

	    assert!(U <= 32); // looking forward to 256-bit CPUs archs. But not 512-bit CPU archs.

	    let header: [u8; 2] = [
		b'3', // version of smalloclog file
		U_U8
	    ];
	    new_file.write_all(&header).unwrap();

	    MY_FILE_OPT = Some(new_file);
	};
    }
}

//XXX#[inline(always)]
//XXXfn log_layout(layout: core::alloc::Layout) -> Result<(), io::Error> {
//XXX    write(&layout.size().to_le_bytes())?;
//XXX    write(&layout.align().to_le_bytes())?;
//XXX    Ok(())
//XXX}

//XXXX add detection of CPU number
unsafe impl GlobalAlloc for SmallocLog {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
	// Spin until this thread gets the exclusive ownership of LOCK:
	loop {
	    let result = LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed);
	    match result {
		Ok(_x) => { break; },
		Err(_x) =>  { }
	    }
	}

	let p = unsafe { System.alloc(layout) };

	create_my_file();
	let mut entry: [u8; 1+3*U] = [0; 1+3*U];
	let e: &mut [u8] = &mut entry;

	let mut i = 0;
	e[i] = b'a'; i += 1; // alloc
	e[i..i+U].copy_from_slice(&layout.size().to_le_bytes()); i += U;
	e[i..i+U].copy_from_slice(&layout.align().to_le_bytes()); i += U;
	e[i..i+U].copy_from_slice(&p.addr().to_le_bytes());

	#[allow(static_mut_refs)] // This function is called only from within a lock (a dumb compare-and-exchange spinlock). So we can use non-atomic test and set on MY_FILE_OPT.
	unsafe {
	    assert!(! MY_FILE_OPT.is_none(), "It was created by create_MY_FILE above.");
	    MY_FILE_OPT.as_mut().expect("").write_all(&entry).unwrap();
	}

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);

	p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
	// Spin until this thread gets the exclusive ownership of LOCK:
	loop {
	    let result = LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed);
	    match result {
		Ok(_x) => { break; },
		Err(_x) =>  { }
	    }
	}

	let mut entry: [u8; 1+1*U] = [0; 1+1*U];
	let e: &mut [u8] = &mut entry;
	let mut i = 0;
	e[i] = b'd'; i += 1; // dealloc
	e[i..i+U].copy_from_slice(&ptr.addr().to_le_bytes());

	#[allow(static_mut_refs)] // This function is called only from within a lock (a dumb compare-and-exchange spinlock). So we can use non-atomic test and set on MY_FILE_OPT.
	unsafe {
	    assert!(! MY_FILE_OPT.is_none(), "MY_FILE is created in alloc(), which must be called before any call to dealloc().");
	    MY_FILE_OPT.as_mut().expect("").write_all(&entry).unwrap();
	}

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);

	unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
	// Spin until this thread gets the exclusive ownership of LOCK:
	loop {
	    let result = LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed);
	    match result {
		Ok(_x) => { break; },
		Err(_x) =>  { }
	    }
	}

	let newptr = unsafe { System.realloc(ptr, layout, new_size) };

	let mut entry: [u8; 1+5*U] = [0; 1+5*U];
	let e: &mut [u8] = &mut entry;
	let mut i = 0;
	e[i] = b'r'; i += 1; // realloc
	e[i..i+U].copy_from_slice(&ptr.addr().to_le_bytes()); i += U;
	e[i..i+U].copy_from_slice(&layout.size().to_le_bytes()); i += U;
	e[i..i+U].copy_from_slice(&layout.align().to_le_bytes()); i += U;
	e[i..i+U].copy_from_slice(&new_size.to_le_bytes()); i += U;
	e[i..i+U].copy_from_slice(&newptr.addr().to_le_bytes());

	#[allow(static_mut_refs)] // This function is called only from within a lock (a dumb compare-and-exchange spinlock). So we can use non-atomic test and set on MY_FILE_OPT.
	unsafe {
	    assert!(! MY_FILE_OPT.is_none(), "MY_FILE is created in alloc(), which must be called before any call to realloc().");
	    MY_FILE_OPT.as_mut().expect("").write_all(&entry).unwrap();
	}

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);

	newptr
    }
}

use std::fmt::Write as FmtWrite;

pub fn smalloclog_to_human_readable(sml: &[u8]) -> String {
    let mut i: usize = 0; // index of next byte to read
    let mut res: String = String::new();

    assert!(sml[i] == b'3', "This version of smalloclog can read only version 3 smallocloc files.");
    i += 1;
    let sou: usize = sml[i] as usize; // source usize
    i += 1;
    assert!(sou <= 32); // looking forward to CPU arches with 256-bit pointers
    
    while i < sml.len() {
	if sml[i] == b'a' {
	    i += 1;
	    let siz: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    let ali: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    writeln!(res, "alloc({}, {}) -> 0x{:x}", siz, ali, ptr).ok();
	} else if sml[i] == b'd' {
	    i += 1;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    writeln!(res, "dealloc(0x{:x})", ptr).ok();
	} else if sml[i] == b'r' {
	    i += 1;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    let siz: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    let ali: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    let newsiz: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    let newptr: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;
	    writeln!(res, "realloc(0x{:x}, {}, {}, {}) -> 0x{:x}", ptr, siz, ali, newsiz, newptr).ok();
	} else {
	    panic!("Found something unexpected in smalloclog. sml[{}]: {}, first part: {}, remainder: 0x{:?}", i,  sml[i], res, &sml[i..]);
	}
    }

    res
}
