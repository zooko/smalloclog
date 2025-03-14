use std::fs::{File};
use std::io::Write;
use std::io;

use std::primitive::usize;

static mut MY_FILE_OPT: Option<File> = None;

#[allow(static_mut_refs)]
fn write(bs: &[u8]) -> Result<(), io::Error> {
    // This function is called only from within a lock (a dumb compare-and-exchange spinlock). So we can use non-atomic test and set on MY_FILE_OPT.
    unsafe {
	if let None = MY_FILE_OPT.as_mut() {
	    let mut new_file = File::create("smalloclog.log")?;
	    new_file.write_all(b"i")?;

	    assert!(std::primitive::usize::BITS< 256);
	    let bytes_in_usize: u8 = (std::primitive::usize::BITS / 8) as u8;

	    new_file.write_all(&[bytes_in_usize])?;

	    MY_FILE_OPT = Some(new_file);
	};

	MY_FILE_OPT.as_mut().expect("x").write_all(bs)?;
	Ok(())
    }
}

use core::{
    alloc::{GlobalAlloc, Layout}
};

use std::alloc::System;

use std::sync::atomic::{AtomicBool, Ordering};

static LOCK: AtomicBool = AtomicBool::new(false);

use smalloc::layout_to_sizeclass;

pub struct SmallocLog { }

#[inline(always)]
fn log_layout(layout: core::alloc::Layout) -> Result<(), io::Error> {
    write(&layout.size().to_le_bytes())?;
    write(&layout.align().to_le_bytes())?;
    Ok(())
}

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

	write(b"a").unwrap();
	log_layout(layout).unwrap();
	let p = unsafe { System.alloc(layout) };
	write(&p.addr().to_le_bytes()).unwrap();

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

	write(b"d").unwrap();
	write(&ptr.addr().to_le_bytes()).unwrap();

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

	write(b"r").unwrap();
	write(&ptr.addr().to_le_bytes()).unwrap();
	log_layout(layout).unwrap();
	write(&new_size.to_le_bytes()).unwrap();
	let newptr = unsafe { System.realloc(ptr, layout, new_size) };
	write(&newptr.addr().to_le_bytes()).unwrap();

	// ok we're done, release the lock
	LOCK.store(false, Ordering::Release);

	newptr
    }
}

use std::fmt::Write as FmtWrite;

pub fn smalloclog_to_human_readable(sml: &[u8]) -> String {
    let mut i: usize = 0; // index of next byte to read
    let mut res: String = String::new();

    assert!(sml[i] == b'i', "something unexpected in smalloclog");
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

use std::collections::HashMap;

use thousands::Separable;

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
    
    let mut realloc_grow = 0;
    let mut realloc_shrink = 0;
    let mut realloc_same = 0;

    let mut reallocon2c: HashMap<(usize, usize), u64> = HashMap::with_capacity(100_000_000);
    
    let mut ptr2sc: HashMap<usize, u8> = HashMap::with_capacity(10_000_000);

    let mut i: usize = 0; // index of next byte to read

    assert!(sml[i] == b'i', "something unexpected in smalloclog");
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
	    let ptr: usize = usize::from_le_bytes(sml[i..i+sou].try_into().unwrap());
	    i += sou;

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

	    if newsc > origsc {
		realloc_grow += 1
	    } else if newsc < origsc {
		realloc_shrink += 1
	    } else {
		realloc_same += 1
	    }

	    let realloconkey = (siz, newsiz); // XXX note: neglecting alignment here, because it probably won't matter much to the results, and because I can't think of a nice way to take into account while still merging same-sized reallocs...
	    *reallocon2c.entry(realloconkey).or_insert(0) += 1;
	    
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
	writeln!(res, "sc: {:>2}, highwater-count: {:>10}, tot: {:>10}", i, slabs_highwater[i].separate_with_commas(), slabs_totallocs[i].separate_with_commas()).unwrap();
    }

    writeln!(res, "oversize: highwater-count: {:>10}, tot: {:>10}", oversize_highwater.separate_with_commas(), oversize_totalallocs.separate_with_commas()).unwrap();
    writeln!(res, "realloc grow: {:>6}, shrink: {:>10}, same: {:>10}", realloc_grow.separate_with_commas(), realloc_shrink.separate_with_commas(), realloc_same.separate_with_commas()).unwrap();

    let mut reallocon2c_entries: Vec<(&(usize, usize), &u64)> = reallocon2c.iter().collect();
    reallocon2c_entries.sort_by(|a, b| a.1.cmp(b.1));

    let mut bytes_moved = 0;
    let mut bytes_saved_from_move = 0;
    for (sizs, count) in reallocon2c_entries {
        write!(res, "{:>10} * {:>10} ({:>2}) -> {:>10} ({:>2})", count.separate_with_commas(), sizs.0.separate_with_commas(), layout_to_sizeclass(sizs.0, 1), sizs.1.separate_with_commas(), layout_to_sizeclass(sizs.1, 1)).ok();
	if layout_to_sizeclass(sizs.1, 1) > layout_to_sizeclass(sizs.0, 1) {
	    let bm: u128 = (*count as u128) * (sizs.0 as u128);
	    writeln!(res, " + {:>12}", bm.separate_with_commas()).ok();
	    bytes_moved += bm;
	} else {
	    let bs: u128 = (*count as u128) * (sizs.0 as u128);
	    writeln!(res, " _ {:>12}", bs.separate_with_commas()).ok();
	    bytes_saved_from_move += bs;
	}
    }

    writeln!(res, "bytes moved: {}, bytes saved from move: {}", bytes_moved.separate_with_commas(), bytes_saved_from_move.separate_with_commas()).ok();
    res
}

