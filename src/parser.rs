use std::io::Write;

#[derive(Debug)]
pub enum Entry {
    Alloc { reqsiz: usize, reqalign: usize, resptr: usize },
    Free { oldptr: usize },
    Realloc { oldptr: usize, oldsiz: usize, reqalign: usize, newsiz: usize, resptr: usize }
}

pub trait EntryConsumerTrait {
    fn consume_entry(&mut self, e: &Entry);
    fn done(&mut self);
}

pub struct Logger<W: Write> {
    w: W
}

impl<W:Write> Logger<W> {
    pub fn new(w: W) -> Self {
        Logger { w }
    }
}

impl<W: Write> EntryConsumerTrait for Logger<W> {
    fn consume_entry(&mut self, e: &Entry) {
	match e {
	    Entry::Alloc { reqsiz, reqalign, resptr } =>
		writeln!(self.w, "alloc({}, {}) -> 0x{:x}", reqsiz, reqalign, resptr).ok(),
	    Entry::Free { oldptr } =>
		writeln!(self.w, "dealloc(0x{:x})", oldptr).ok(),
	    Entry::Realloc { oldptr, oldsiz, reqalign, newsiz, resptr } =>
		writeln!(self.w, "realloc(0x{:x}, {}, {}, {}) -> 0x{:x}", oldptr, oldsiz, reqalign, newsiz, resptr).ok()
	}.unwrap()
    }
    fn done(&mut self) {
	writeln!(self.w, "done:-)!");
    }
}

use std::collections::HashMap;

// XXX my first experiment with explicit lifetimes... My intent here is that Statser and the w write object will both be allocated in main(), so I just want to convince Rust that Statser cannot outlive w...
pub struct Statser<W: Write> {
    w: W,
    slabs_now: Vec<usize>,
    slabs_totallocs: Vec<usize>,
    slabs_highwater: Vec<usize>,
    oversize_now: usize,
    oversize_totalallocs: usize,
    oversize_highwater: usize,
    reallocon2c: HashMap<(usize, usize), u64>,
    ptr2sc: HashMap<usize, u8>
}

use bytesize::ByteSize;
fn conv(size: u128) -> String {
    let byte_size = ByteSize::b(size as u64);
    byte_size.to_string()
}
use thousands::Separable;

const NUM_SCS: usize = smalloc::NUM_SCSU8 as usize;
impl<W:Write> Statser<W> {

    pub fn new(w: W) -> Self {
	let mut ns = Statser {
	    w,
	    slabs_now: Vec::with_capacity(NUM_SCS),
	    slabs_totallocs: Vec::with_capacity(NUM_SCS),
	    slabs_highwater: Vec::with_capacity(NUM_SCS),
	    oversize_now: 0,
	    oversize_totalallocs: 0,
	    oversize_highwater: 0,
	    reallocon2c: HashMap::with_capacity(100_000_000),
	    ptr2sc: HashMap::with_capacity(100_000_000)
	};

	ns.slabs_now.resize(NUM_SCS, 0); // initialize elements to 0
	ns.slabs_totallocs.resize(NUM_SCS, 0); // initialize elements to 0
	ns.slabs_highwater.resize(NUM_SCS, 0); // initialize elements to 0

	ns
    }

    fn write_stats(&mut self) {
	writeln!(self.w, "{:>4} {:>10} {:>9} {:>13}", "sc", "size", "highwater", "tot").unwrap();
	writeln!(self.w, "{:>4} {:>10} {:>9} {:>13}", "--", "----", "---------", "---").unwrap();
	for i in 0..NUM_SCS {
	    writeln!(self.w, "{:>4} {:>10} {:>9} {:>13}", i, conv(sizeclass_to_slotsize(i as u8) as u128), self.slabs_highwater[i].separate_with_commas(), self.slabs_totallocs[i].separate_with_commas()).unwrap();
	}

	writeln!(self.w, ">{:>3} >{:>9} {:>9} {:>13}", NUM_SCS, conv(sizeclass_to_slotsize((NUM_SCS-1) as u8) as u128), self.oversize_highwater, self.oversize_totalallocs).unwrap();

	let mut reallocon2c_entries: Vec<(&(usize, usize), &u64)> = self.reallocon2c.iter().collect();
	reallocon2c_entries.sort_by(|a, b| a.1.cmp(b.1));

	let mut bytes_moved = 0;
	let mut bytes_saved_from_move = 0;
	for (sizs, count) in reallocon2c_entries {
            write!(self.w, "{:>10} * {:>10} ({:>2}) -> {:>10} ({:>2})", count.separate_with_commas(), sizs.0.separate_with_commas(), layout_to_sizeclass(sizs.0, 1), sizs.1.separate_with_commas(), layout_to_sizeclass(sizs.1, 1)).ok();
	    if layout_to_sizeclass(sizs.1, 1) > layout_to_sizeclass(sizs.0, 1) {
		let bm: u128 = (*count as u128) * (sizs.0 as u128);
		writeln!(self.w, " + {:>12}", bm.separate_with_commas()).ok();
		bytes_moved += bm;
	    } else {
		let bs: u128 = (*count as u128) * (sizs.0 as u128);
		writeln!(self.w, " _ {:>12}", bs.separate_with_commas()).ok();
		bytes_saved_from_move += bs;
	    }
	}

	writeln!(self.w, "bytes moved: {}, bytes saved from move: {}", bytes_moved.separate_with_commas(), bytes_saved_from_move.separate_with_commas()).ok();
    }
}

use smalloc::{layout_to_sizeclass, sizeclass_to_slotsize};
impl<'a, W: Write> EntryConsumerTrait for Statser<W> {
    fn consume_entry(&mut self, e: &Entry) {
	match e {
	    Entry::Alloc { reqsiz, reqalign, resptr } => {
		let sc = layout_to_sizeclass(*reqsiz, *reqalign);
		let scu = sc as usize;

		assert!(! self.ptr2sc.contains_key(resptr));
		self.ptr2sc.insert(*resptr, sc);

		if scu >= NUM_SCS {
		    self.oversize_totalallocs += 1;
		    self.oversize_now += 1;
		    self.oversize_highwater = self.oversize_highwater.max(self.oversize_now);
		} else {
		    self.slabs_totallocs[scu] += 1;
		    self.slabs_now[scu] += 1;
		    self.slabs_highwater[scu] = self.slabs_highwater[scu].max(self.slabs_now[scu]);
		}
	    }
	    Entry::Free { oldptr } => {
		assert!(self.ptr2sc.contains_key(oldptr));
		
		let sc = self.ptr2sc[oldptr];
		let scu = sc as usize;

		if scu >= NUM_SCS {
		    self.oversize_now -= 1;
		} else {
		    self.slabs_now[scu] -= 1;
		}

		self.ptr2sc.remove(oldptr);
	    }
	    Entry::Realloc { oldptr, oldsiz, reqalign, newsiz, resptr } => {
		assert!(self.ptr2sc.contains_key(oldptr));

		let origscu = self.ptr2sc[oldptr] as usize;
		self.ptr2sc.remove(&oldptr);

		if origscu > NUM_SCS {
		    self.oversize_now -= 1;
		} else {
		    self.slabs_now[origscu] -= 1;
		}

		let newsc = layout_to_sizeclass(*newsiz, *reqalign);
		let newscu = newsc as usize;

		assert!(! self.ptr2sc.contains_key(&resptr));
		self.ptr2sc.insert(*resptr, newsc);

		//XXX simulate promotion of growers

		let realloconkey = (*oldsiz, *newsiz); // note: neglecting alignment here, because it probably won't matter much to the results, and because I can't think of a nice way to take into account while still merging same-sized reallocs...
		*self.reallocon2c.entry(realloconkey).or_insert(0) += 1;

		if newscu > NUM_SCS {
		    if origscu != newscu {
			self.oversize_totalallocs += 1;
		    }

		    self.oversize_now += 1;
		    self.oversize_highwater = self.oversize_highwater.max(self.oversize_now);
		} else {
		    if origscu != newscu {
			self.slabs_totallocs[newscu] += 1;
		    }

		    self.slabs_now[newscu] += 1;
		    self.slabs_highwater[newscu] = self.slabs_highwater[newscu].max(self.slabs_now[newscu]);
		}
	    }
	}
    }
    fn done(&mut self) {
	self.write_stats();
	writeln!(self.w, "done:-}}!");
    }
}

pub struct Parser<T: EntryConsumerTrait> {
    entryconsumer: T,
    consumedheader: bool,
    sou: usize,

    // How many bytes do we need to read to decode each of these 4 things:
    chunk_size_header: usize,
    chunk_size_alloc: usize,
    chunk_size_free: usize,
    chunk_size_realloc: usize
}

use std::cmp::max;
impl<T: EntryConsumerTrait> Parser<T> {
    pub fn new(entryconsumer: T) -> Self {
        Parser {
	    entryconsumer,
	    consumedheader: false,
	    sou: 0,
	    chunk_size_header: 2,
	    chunk_size_alloc: 0,
	    chunk_size_free: 0,
	    chunk_size_realloc: 0
	}
    }

    /// Returns the biggest chunk of bytes that this parser might need to be able to make progress. (This is necessary for slurp() to do minimal copying while guaranteeing progress.)
    #[inline(always)]
    fn biggest_chunk_needed(&self) -> usize {
	return max(max(max(self.chunk_size_header, self.chunk_size_alloc), self.chunk_size_free), self.chunk_size_realloc);
    }
    
    #[inline(always)]
    /// Returns the number of bytes successfully consumed. If the
    /// return value is non-zero then the header was successfully
    /// consumed and the self.sou value was populated.
    fn try_to_consume_header_bytes(&mut self, bs: &[u8]) -> usize {
	let mut i: usize = 0;
	if bs.len() < self.chunk_size_header {
	    return 0;
	}

	assert!(bs[i] == b'3', "This version of smalloclog can read only version 3 smalloclog files.");
	i += 1;
	self.consumedheader = true;
	self.sou = bs[i] as usize; // source usize
	self.chunk_size_alloc = 1 + 3*self.sou;
	self.chunk_size_free = 1 + 1*self.sou;
	self.chunk_size_realloc = 1 + 5*self.sou;
	i += 1;

	assert!(i == self.chunk_size_header);

	return self.chunk_size_header;
    }

    /// Returns a tuple of (Option<Entry>, number of bytes successfully consumed).
    fn try_to_parse_next_entry(&self, bs: &[u8]) -> (Option<Entry>, usize) {
	let mut retentry: Option<Entry> = None;
	let mut i: usize = 0; // consumed bytes
	let sou = self.sou; // to save a few chars of reading
	
	if bs.len() >= 1 {
	    match bs[i] {
		b'a' => {
		    if bs.len() >= self.chunk_size_alloc {
			i += 1;
			
			let reqsiz = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let reqalign = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let resptr = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			
			retentry = Some(Entry::Alloc { reqsiz, reqalign, resptr });
		    }
		}
		b'd' => {
		    if bs.len() >= self.chunk_size_free {
			i += 1;

			let oldptr = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			
			retentry = Some(Entry::Free { oldptr });
		    }
		}
		b'r' => {
		    if bs.len() >= self.chunk_size_realloc {
			i += 1;

			let oldptr = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let oldsiz = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let reqalign = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let newsiz = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let resptr = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			
			retentry = Some(Entry::Realloc { oldptr, oldsiz, reqalign, newsiz, resptr });
		    }
		}
		_ => {
		    let debugbuf = &bs[i..i+60];
		    panic!("Found something unexpected in smalloclog. i: {}, bs[i..i+60]: {:?}", i, debugbuf);
		}
	    }
	}
	return (retentry, i);
    }
    
    /// Returns the number of bytes successfully consumed.
    pub fn try_to_consume_bytes(&mut self, bs: &[u8]) -> usize {
	let mut ourbs = bs; // Our slice (reference to bs)
	let mut retval: usize = 0; // track how many bytes we consumed to return it when we're done.

	if ! self.consumedheader {
	    //eprintln!("xxx about to ttchb(), bs.len(): {}", ourbs.len());

	    let hbs = self.try_to_consume_header_bytes(ourbs);
	    if hbs == 0 {
		return 0;
	    }

	    retval += hbs;

	    // Slice from the first un-consumed byte onwards.
	    ourbs = &ourbs[retval..]
	}
	
	loop {
	    //eprintln!("xxx about to ttpne(), bs.len(): {}", ourbs.len());
	    let (e, j) = self.try_to_parse_next_entry(ourbs);
	    //eprintln!("xxx just ttpne(), j: {}", j);

	    if j == 0 {
		return retval;
	    }

	    ourbs = &ourbs[j..];
	    retval += j;

	    self.entryconsumer.consume_entry(&e.unwrap());
	}
    }

    pub fn done(&mut self) {
	self.entryconsumer.done();
    }
}

use std::io::{BufRead};
use std::io;

const BUFSIZ: usize = 2usize.pow(20); // XXX todo experiment with different buf sizes

/// This function doesn't return until `r` returns 0 from a call to read(). Which hopefully won't happen until we're done, ie the end of the file has been reached if `r` is a file, or the pipe has been closed if `r` is a pipe.
pub fn slurp<R: BufRead, T: EntryConsumerTrait>(mut r: R, mut p: Parser<T>) {
    let mut buffer: [u8; BUFSIZ] = [0; BUFSIZ];
    let mut bytesfilled: usize = 0;

    loop {
	//eprintln!("xxx about to read(), bytesfilled: {}", bytesfilled);
	let bytesread = r.read(&mut buffer[bytesfilled..]).unwrap();
	//eprintln!("xxx just read(), bytesread: {}", bytesread);
	if bytesread == 0 {
	    //eprintln!("xxx about to done()");
	    p.done();
	    return;
	}

	bytesfilled += bytesread;

	//eprintln!("xxx about to ttcb(), bytesfilled: {}", bytesfilled);
	let processed = p.try_to_consume_bytes(&buffer[..bytesfilled]);
	//eprintln!("xxx just ttcb(), processed: {}", processed);

	assert!(processed <= bytesfilled);

	// Copy any leftover bytes from the end to the beginning.
	buffer.copy_within(processed..bytesfilled, 0);

	bytesfilled -= processed;
    }
}

