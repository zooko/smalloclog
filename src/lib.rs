use core::{
    alloc::{GlobalAlloc, Layout}
};

use std::alloc::System;

mod simplelogger;

use std::sync::atomic::{AtomicBool, Ordering};

static LOCK: AtomicBool = AtomicBool::new(false);

use smalloc::layout_to_sizeclass;

//XXXpub(crate) use firestorm::{
//XXX    profile_fn,
//XXX    profile_method,
//XXX    profile_section
//XXX};

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
		Ok(_x) => { break; },
		Err(_x) =>  { }
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
		Ok(_x) => { break; },
		Err(_x) =>  { }
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

use std::collections::HashMap;

pub fn smalloclog_to_stats(sml: &[u8]) -> String {
    const NUM_SCSU8: u8=30;
    const NUM_SCS: usize = NUM_SCSU8 as usize;
    
    let mut slabs_now: Vec<usize> = Vec::with_capacity(NUM_SCS);
    slabs_now.resize(NUM_SCS , 0);

    let mut slabs_totallocs: Vec<usize> = Vec::with_capacity(NUM_SCS);
    slabs_totallocs.resize(NUM_SCS , 0);

    let mut slabs_highwater: Vec<usize> = Vec::with_capacity(NUM_SCS);
    slabs_highwater.resize(NUM_SCS, 0);

    let mut oversize_now = 0;
    let mut oversize_totalallocs = 0;
    let mut oversize_highwater = 0;
    
    let mut ptr2sc: HashMap<usize, u8> = HashMap::with_capacity(10000000);

    let mut i: usize = 0; // index of next byte to read

    while i < sml.len() {
	if sml[i] == b'a' {
	    i += 1;
	    let siz: usize = usize::from_le_bytes(sml[i..i+SO_U_SRC].try_into().unwrap());
	    i += SO_U_SRC;
	    let ali: usize = usize::from_le_bytes(sml[i..i+SO_U_SRC].try_into().unwrap());
	    i += SO_U_SRC;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+SO_P_SRC].try_into().unwrap());
	    i += SO_P_SRC;

	    let sc = layout_to_sizeclass(siz, ali);
	    let scu = sc as usize;

	    assert!(! ptr2sc.contains_key(&ptr));
	    ptr2sc.insert(ptr, sc);

	    if scu >= NUM_SCS {
		oversize_totalallocs += 1;
		oversize_now += 1;
		oversize_highwater = oversize_highwater.max(oversize_now);
	    } else {
		slabs_totallocs[scu] += 1;
		slabs_now[scu] += 1;
		slabs_highwater[scu] = slabs_highwater[scu].max(slabs_now[scu]);
	    }
	} else if sml[i] == b'd' {
	    i += 1;
	    let ptr: usize = usize::from_le_bytes(sml[i..i+SO_P_SRC].try_into().unwrap());
	    i += SO_P_SRC;

	    assert!(ptr2sc.contains_key(&ptr));

	    let sc = ptr2sc[&ptr];
	    let scu = sc as usize;

	    if scu >= NUM_SCS {
		oversize_now -= 1;
	    } else {
		slabs_now[scu] -= 1;
	    }

	    ptr2sc.remove(&ptr);
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

	    assert!(ptr2sc.contains_key(&ptr));

	    let origsc = ptr2sc[&ptr];
	    let origscu = origsc as usize;

	    ptr2sc.remove(&ptr);

	    if origscu > NUM_SCS {
		oversize_now -= 1;
	    } else {
		slabs_now[origscu] -= 1;
	    }

	    let newsc = layout_to_sizeclass(newsiz, ali);
	    let newscu = newsc as usize;

	    assert!(! ptr2sc.contains_key(&newptr));
	    ptr2sc.insert(newptr, newsc);

	    if newscu > NUM_SCS {
		if origscu != newscu {
		    oversize_totalallocs += 1;
		}

		oversize_now += 1;
		oversize_highwater = oversize_highwater.max(oversize_now);
	    } else {
		if origscu != newscu {
		    slabs_totallocs[newscu] += 1;
		}

		slabs_now[newscu] += 1;
		slabs_highwater[newscu] = slabs_highwater[newscu].max(slabs_now[newscu]);
	    }
	} else {
	    panic!("Found something unexpected in smalloclog. sml[{}]: {}, so far: {:?}, remainder: 0x{:?}", i,  sml[i], &slabs_highwater, &sml[i..]);
	}
    }

    let mut res: String = String::new();
    for i in 0..NUM_SCS {
	writeln!(res, "sc: {}, highwater-count: {}, tot: {}", i, slabs_highwater[i], slabs_totallocs[i]).unwrap();
    }
    writeln!(res, "oversize: highwater-count: {}, tot: {}", oversize_highwater, oversize_totalallocs).unwrap();

    res
}

#[cfg(test)]
mod tests {

    #[test]
    fn read_file_1() {
	//Xxx    let file_path = "test_file.smalloclog";
	//XXX        let expected_contents = b"";

    }

}

