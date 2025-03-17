use std::io::Write;

// XXX TODO: extend smalloclog to detect CPU number and track precisely the spread of allocations and other events across CPUs...
		
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
	writeln!(self.w, "done:-)!").ok();
    }
}

use std::collections::HashMap;

pub struct Statser<W: Write> {
    w: W,

    // How many allocations have been requested out of each of these slabs (each "slab" having a fixed number of fixed-size "slots").
    slabs_now: Vec<usize>,
    slabs_totallocs: Vec<usize>,
    slabs_highwater: Vec<usize>,

    // map realloc traces (oldsize, newsize) to counts of how many times that realloc happened
    reallocon2c: HashMap<(usize, usize), usize>,
    // map ptr to current SC
    ptr2sc: HashMap<usize, usize>,
}

use bytesize::ByteSize;
fn conv(size: usize) -> String {
    let byte_size = ByteSize::b(size as u64);
    byte_size.to_string()
}
use thousands::Separable;

use smalloc::{sizeclass_to_slots,OVERSIZE_SC,NUM_SCS};
impl<W:Write> Statser<W> {

    pub fn new(w: W) -> Self {
	let mut ns = Statser {
	    w,
	    slabs_now: Vec::with_capacity(NUM_SCS),
	    slabs_totallocs: Vec::with_capacity(NUM_SCS),
	    slabs_highwater: Vec::with_capacity(NUM_SCS),
	    reallocon2c: HashMap::with_capacity(100_000_000),
	    ptr2sc: HashMap::with_capacity(100_000_000)
	};

	ns.slabs_now.resize(NUM_SCS, 0); // initialize elements to 0
	ns.slabs_totallocs.resize(NUM_SCS, 0); // initialize elements to 0
	ns.slabs_highwater.resize(NUM_SCS, 0); // initialize elements to 0

	eprintln!("{:>4} {:>11} {:>11} {:>15}", "sc", "size", "highwater", "tot");
	eprintln!("{:>4} {:>11} {:>11} {:>15}", "--", "----", "---------", "---");

	ns
    }

    fn write_stats(&mut self) {
	writeln!(self.w, "{:>4} {:>11} {:>11} {:>15}", "sc", "size", "highwater", "tot").unwrap();
	writeln!(self.w, "{:>4} {:>11} {:>11} {:>15}", "--", "----", "---------", "---").unwrap();
	for i in 0..OVERSIZE_SC {
	    writeln!(self.w, "{:>4} {:>11} {:>11} {:>15}", i, conv(sizeclass_to_slotsize(i)), self.slabs_highwater[i].separate_with_commas(), self.slabs_totallocs[i].separate_with_commas()).unwrap();
	}

	writeln!(self.w, ">{:>3} >{:>10} {:>11} {:>15}", OVERSIZE_SC-1, conv(sizeclass_to_slotsize(OVERSIZE_SC-1)), self.slabs_highwater[OVERSIZE_SC].separate_with_commas(), self.slabs_totallocs[OVERSIZE_SC].separate_with_commas()).unwrap();

	let mut reallocon2c_entries: Vec<(&(usize, usize), &usize)> = self.reallocon2c.iter().collect();
	reallocon2c_entries.sort_by(|a, b| a.1.cmp(b.1));

	let mut bytes_moved = 0;
	let mut bytes_saved_from_move = 0;
	for (sizs, count) in reallocon2c_entries {
            write!(self.w, "{:>12} * {:>10} ({:>2}) -> {:>10} ({:>2})", count.separate_with_commas(), sizs.0.separate_with_commas(), layout_to_sizeclass(sizs.0, 1), sizs.1.separate_with_commas(), layout_to_sizeclass(sizs.1, 1)).ok();
	    if layout_to_sizeclass(sizs.1, 1) > layout_to_sizeclass(sizs.0, 1) {
		let bm = *count * sizs.0;
		writeln!(self.w, " + {:>14}", bm.separate_with_commas()).ok();
		bytes_moved += bm;
	    } else {
		let bs = *count * sizs.0;
		writeln!(self.w, " _ {:>14}", bs.separate_with_commas()).ok();

//		xxxx track provenance of growers! How many of them came up from smaller growers. And how many smaller growers then stopped growing!
		    
		bytes_saved_from_move += bs;
	    }
	}

	writeln!(self.w, "bytes moved: {}, bytes saved from move: {}", bytes_moved.separate_with_commas(), bytes_saved_from_move.separate_with_commas()).ok();
    }

    fn find_next_size_class_with_open_slot(&mut self, mut sc: usize) -> usize {
	let mut s = sizeclass_to_slots(sc);
	
	assert!(self.slabs_now[sc] <= s, "{}, {}, {}", sc, self.slabs_now[sc], s); // We can never have more that s slots in a slab.

	// Note that in smalloc we falsely claim that the "oversize" slab has 8 bytes worth of indexes, when in fact there is no slab there, we're just going to fall back to mmap() for things that big. We just pretend there are 2^64 slots in that slab so that this will never overflow out of that sizeclass.
	while self.slabs_now[sc] == s {
	    // This slab is full so we overflow to the next sizeclass.

	    sc += 1;
	    assert!(sc < NUM_SCS); // See "Note" above, on this loop.

	    s = sizeclass_to_slots(sc);

	    assert!(self.slabs_now[sc] <= s, "{}, {}, {}", sc, self.slabs_now[sc], s); // We can never have more that s slots in a slab.
	}
		
	assert!(self.slabs_now[sc] < s);

	sc
    }
}

use smalloc::{layout_to_sizeclass, sizeclass_to_slotsize};
impl<W: Write> EntryConsumerTrait for Statser<W> {
    fn consume_entry(&mut self, e: &Entry) {
	match e {
	    Entry::Alloc { reqsiz, reqalign, resptr } => {
		let origsc = layout_to_sizeclass(*reqsiz, *reqalign);
		assert!(origsc < NUM_SCS);

		// Overflow of slabs:
		let sc = self.find_next_size_class_with_open_slot(origsc);

		assert!(! self.ptr2sc.contains_key(resptr));
		self.ptr2sc.insert(*resptr, sc);

		self.slabs_totallocs[sc] += 1;
		self.slabs_now[sc] += 1;

		if self.slabs_now[sc] > self.slabs_highwater[sc] {
		    self.slabs_highwater[sc] = self.slabs_now[sc];
		    if self.slabs_now[sc] == sizeclass_to_slots(sc) {
			if sc == OVERSIZE_SC {
			    eprintln!(">{:>3} >{:>10} {:>11} {:>15}", OVERSIZE_SC-1, conv(sizeclass_to_slotsize(OVERSIZE_SC-1)), self.slabs_highwater[sc].separate_with_commas(), self.slabs_totallocs[sc].separate_with_commas());
			} else {
			    eprintln!("{:>4} {:>11} {:>11} {:>15}", sc, conv(sizeclass_to_slotsize(sc)), self.slabs_highwater[sc].separate_with_commas(), self.slabs_totallocs[sc].separate_with_commas());
			}
		    }
		}
	    }
	    Entry::Free { oldptr } => {
		assert!(self.ptr2sc.contains_key(oldptr));
		
		let sc = self.ptr2sc[oldptr];
		assert!(sc < NUM_SCS);

		assert!(self.slabs_now[sc] > 0, "slabs[{}]: {}", sc, self.slabs_now[sc]);
		self.slabs_now[sc] -= 1;

		self.ptr2sc.remove(oldptr);
	    }
	    Entry::Realloc { oldptr, oldsiz, reqalign, newsiz, resptr } => {
		assert!(self.ptr2sc.contains_key(oldptr));

		let origsc = self.ptr2sc[oldptr];
		assert!(origsc < NUM_SCS);
		assert!(layout_to_sizeclass(*oldsiz, *reqalign) <= origsc); // The SC we had this ptr in (in ptr2sc) was big enough to hold the oldsiz&alignment.
		self.ptr2sc.remove(oldptr);

		self.slabs_now[origsc] -= 1;

		let mut newsc = layout_to_sizeclass(*newsiz, *reqalign);
		assert!(newsc < NUM_SCS);

		// Now we simulate two things: 1. Promotion of growers. 2. Overflow of slabs, By "simulate" I mean that while the actual underlying allocator is going to do whatever it does with this request for realloc(), we're here going to choose a sizeclass to simulate that the new pointer would be in in smalloc.

		// (Note: we're assuming here the "worst-case scenario", where one CPU did all of these allocations. In practice we may get a little relief of the congestion of these slabs from the allocations being spread out over multiple CPUs, but we don't want to count on that necessarily, so let's look at the worst-case scenario first...)

		// 1. Promote growers. Any re-allocation which exceeds its slot gets bumped, not to the next sizeclass that is big enough, but also to the next sizeclass group. The groups are: A. Things that can pack into a 64-byte cache line, B. Things that can pack into a 4096-byte memory page, and C. The huge slots slab. Grower promotion can promote an allocation to group B or group C.
		//XXXif newsc > origsc {
		//XXX    // Okay this realloc required moving the object to a bigger slab. Therefore this is a "grower".
		//XXX    if newsc <= MAX_SC_TO_FIT_CACHELINE {
		//XXX	newsc = MAX_SC_TO_FIT_CACHELINE + 1;
		//XXX    } else {
		//XXX	newsc = HUGE_SLOTS_SC;
		//XXX    }
		//XXX}

		// 2. Overflow of slabs:
		newsc = self.find_next_size_class_with_open_slot(newsc);

		// Okay now insert this into our "map" -- the combination of ptr2sc and reallocon2c.
		assert!(! self.ptr2sc.contains_key(resptr));
		self.ptr2sc.insert(*resptr, newsc);

		let realloconkey = (*oldsiz, *newsiz); // note: neglecting alignment in realloconkey, because it probably won't matter much to the results, and because I can't think of a nice way to take into account while still merging same-sized reallocs in order to get more useful statistics...// XXX revisit this now that we have the alloc-realloc tracking! :-) :-) :-)
		*self.reallocon2c.entry(realloconkey).or_insert(0) += 1;

		if origsc != newsc {
		    self.slabs_totallocs[newsc] += 1;
		}

		self.slabs_now[newsc] += 1;
		if self.slabs_now[newsc] > self.slabs_highwater[newsc] {
		    self.slabs_highwater[newsc] = self.slabs_now[newsc];

		    if self.slabs_now[newsc] == sizeclass_to_slots(newsc) {
			if newsc == OVERSIZE_SC {
			    eprintln!(">{:>3} >{:>10} {:>11} {:>15}", OVERSIZE_SC-1, conv(sizeclass_to_slotsize(OVERSIZE_SC-1)), self.slabs_highwater[newsc].separate_with_commas(), self.slabs_totallocs[newsc].separate_with_commas());
			} else {
			    eprintln!("{:>4} {:>11} {:>11} {:>15}", newsc, conv(sizeclass_to_slotsize(newsc)), self.slabs_highwater[newsc].separate_with_commas(), self.slabs_totallocs[newsc].separate_with_commas());
			}
		    }
		}
	    }
	}
    }

    fn done(&mut self) {
	self.write_stats();

	writeln!(self.w, "done:-}}!").ok();
    }
}

pub struct Parser<T: EntryConsumerTrait> {
    entryconsumer: T,
    consumedheader: bool,

    // The size of a usize on the source machine (as read from the smalloclog file header):
    sou: usize,

    // How many bytes do we need to read to decode each of these 4 things:
    chunk_size_header: usize,
    chunk_size_alloc: usize,
    chunk_size_free: usize,
    chunk_size_realloc: usize
}

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
	self.chunk_size_free = 1 + self.sou;
	self.chunk_size_realloc = 1 + 5*self.sou;
	i += 1;

	assert!(i == self.chunk_size_header);

	self.chunk_size_header
    }

    /// Returns a tuple of (Option<Entry>, number of bytes successfully consumed).
    fn try_to_parse_next_entry(&self, bs: &[u8]) -> (Option<Entry>, usize) {
	let mut retentry: Option<Entry> = None;
	let mut i: usize = 0; // consumed bytes
	let sou = self.sou; // to save a few chars of reading
	
	if !bs.is_empty() {
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

	(retentry, i)
    }
    
    /// Returns the number of bytes successfully consumed.
    pub fn try_to_consume_bytes(&mut self, bs: &[u8]) -> usize {
	let mut ourbs = bs; // Our slice (reference to bs)
	let mut retval: usize = 0; // track how many bytes we consumed to return it when we're done.

	if ! self.consumedheader {

	    let hbs = self.try_to_consume_header_bytes(ourbs);
	    if hbs == 0 {
		return 0;
	    }

	    retval += hbs;

	    // Slice from the first un-consumed byte onwards.
	    ourbs = &ourbs[retval..]
	}
	
	loop {
	    let (e, j) = self.try_to_parse_next_entry(ourbs);

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

const BUFSIZ: usize = 2usize.pow(20);

/// This function doesn't return until `r` returns 0 from a call to read(). Which hopefully won't happen until we're done, ie the end of the file has been reached if `r` is a file, or the pipe has been closed if `r` is a pipe.
pub fn slurp<R: BufRead, T: EntryConsumerTrait>(mut r: R, mut p: Parser<T>) {
    let mut buffer: [u8; BUFSIZ] = [0; BUFSIZ];
    let mut bytesfilled: usize = 0;

    loop {
	let bytesread = r.read(&mut buffer[bytesfilled..]).unwrap();
	if bytesread == 0 {
	    p.done();
	    return;
	}

	bytesfilled += bytesread;

	let processed = p.try_to_consume_bytes(&buffer[..bytesfilled]);

	assert!(processed <= bytesfilled);

	// Copy any leftover bytes from the end to the beginning.
	buffer.copy_within(processed..bytesfilled, 0);

	bytesfilled -= processed;
    }
}

