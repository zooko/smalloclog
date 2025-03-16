use std::fs::{File};
use std::io::Write;

use bytesize::ByteSize;

fn conv(size: u128) -> String {
    let byte_size = ByteSize::b(size as u64);
    byte_size.to_string()
}

use smalloc;

use core::{
    alloc::{GlobalAlloc, Layout}
};

use std::alloc::System;

use std::sync::atomic::{AtomicBool, Ordering};

static LOCK: AtomicBool = AtomicBool::new(false);

use smalloc::{layout_to_sizeclass,sizeclass_to_slotsize};

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

use std::collections::HashMap;

use thousands::Separable;

pub fn smalloclog_to_stats(sml: &[u8]) -> String {
    const NUM_SCS: usize = smalloc::NUM_SCSU8 as usize;
    
    let mut slabs_now: Vec<usize> = Vec::with_capacity(NUM_SCS);
    slabs_now.resize(NUM_SCS , 0);

    let mut slabs_totallocs: Vec<usize> = Vec::with_capacity(NUM_SCS);
    slabs_totallocs.resize(NUM_SCS , 0);

    let mut slabs_highwater: Vec<usize> = Vec::with_capacity(NUM_SCS);
    slabs_highwater.resize(NUM_SCS, 0);

    let mut oversize_now = 0;
    let mut oversize_totalallocs = 0;
    let mut oversize_highwater = 0;
    
    let mut reallocon2c: HashMap<(usize, usize), u64> = HashMap::with_capacity(100_000_000);
    
    let mut ptr2sc: HashMap<usize, u8> = HashMap::with_capacity(10_000_000);

    let mut i: usize = 0; // index of next byte to read

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

	    //XXX simulate promotion of growers

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
    writeln!(res, "{:>4} {:>10} {:>9} {:>13}", "sc", "size", "highwater", "tot").unwrap();
    writeln!(res, "{:>4} {:>10} {:>9} {:>13}", "--", "----", "---------", "---").unwrap();
    for i in 0..NUM_SCS {
	writeln!(res, "{:>4} {:>10} {:>9} {:>13}", i, conv(sizeclass_to_slotsize(i as u8) as u128), slabs_highwater[i].separate_with_commas(), slabs_totallocs[i].separate_with_commas()).unwrap();
    }

    writeln!(res, ">{:>3} >{:>9} {:>9} {:>13}", NUM_SCS, conv(sizeclass_to_slotsize((NUM_SCS-1) as u8) as u128), oversize_highwater, oversize_totalallocs).unwrap();

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
