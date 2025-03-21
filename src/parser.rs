use std::io::Write;

// XXX TODO: extend smalloclog to detect CPU number and track precisely the spread of allocations and other events across CPUs...
		
#[derive(Debug)]
pub enum Entry {
    Alloc { reqsiz: usize, reqalign: usize, resptr: usize },
    Free { oldptr: usize },
    Realloc { prevptr: usize, prevsiz: usize, reqalign: usize, newsiz: usize, resptr: usize }
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
	    Entry::Realloc { prevptr, prevsiz, reqalign, newsiz, resptr } =>
		writeln!(self.w, "realloc(0x{:x}, {}, {}, {}) -> 0x{:x}", prevptr, prevsiz, reqalign, newsiz, resptr).ok()
	}.unwrap()
    }
    fn done(&mut self) {
	writeln!(self.w, "done:-)!").ok();
    }
}

use std::collections::{HashMap,HashSet};

struct ReallocHistoryChapter {
    newsiz: usize,
    align: usize,
    newslabnum: usize
}

struct OpenBook {
    chs: Vec<ReallocHistoryChapter>,
}

struct ClosedBook {
    numlifes: usize, // Number of lifes which follow this exact realloc progression
    movecostworstperlife: usize,

    movecostsmalloc: usize,
    key: String
}

impl ClosedBook {
    pub fn new(openbook: OpenBook) -> Self {
	assert!(!openbook.chs.is_empty());

	let chs = openbook.chs;
	    
	let mut key: String = format!("{}:{}", chs[0].align, chs[0].newsiz);
	for ch in &chs[1..] {
	    key.push_str(format!("->{}", ch.newsiz).as_str());
	}
	
	let mut prevch = &chs[0];
	let mut movecostworstperlife = 0;
	let mut movecostsmalloc = 0;
	for ch in &chs[1..] {
	    movecostworstperlife += prevch.newsiz;
	    if ch.newslabnum != prevch.newslabnum {
		// In smalloc, we have to memcpy the entire contents of the previous slot, not just the occupied bytes! (Because we don't know how many are occupied.
		movecostsmalloc += slabnum_to_slotsize(prevch.newslabnum);
	    }
	    prevch = ch;
	}

	ClosedBook {
	    numlifes: 1,
	    movecostworstperlife,
	    movecostsmalloc,
	    key
	}
    }
}

use std::hash::{Hash,Hasher};
impl Hash for ClosedBook {
    fn hash<H: Hasher>(&self, state: &mut H) {
	self.key.hash(state)
    }
}

impl PartialEq for ClosedBook {
    fn eq(&self, other: &Self) -> bool {
	self.key == other.key
    }
}

impl Eq for ClosedBook { }

use std::cmp::{PartialOrd,Ord,Ordering};
impl PartialOrd for ClosedBook {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ClosedBook {
    fn cmp(&self, other: &Self) -> Ordering {
	let stwc = self.movecostworstperlife*self.numlifes;
	let otwc = other.movecostworstperlife*other.numlifes;
	stwc.cmp(&otwc)
    }
}

pub struct Statser<W: Write> {
    w: W,

    // How many allocations have been requested out of each of these slabs.
    allocs_current: Vec<usize>,
    allocs_total: Vec<usize>,
    allocs_highwater: Vec<usize>,

    // How many times have all the slabs in a whole area been found to be full when we were trying to insert into it.
    slabareaoverflows: Vec<usize>,

    // map ptr to current slab num
    ptr2slabnum: HashMap<usize, usize>,

    // map current ptr to its so-far life history of reallocs
    ptr2ob: HashMap<usize, OpenBook>,

    // set of closed books of realloc histories
    cbs: HashSet<ClosedBook>
	
}

use bytesize::ByteSize;
fn conv(size: usize) -> String {
    let byte_size = ByteSize::b(size as u64);
    byte_size.to_string()
}
use thousands::Separable;

use smalloc::{slabnum_to_numareas, slabnum_to_numslots, slabnum_to_slotsize, OVERSIZE_SLABNUM, NUM_SLABS, MAX_SLABNUM_TO_FIT_INTO_CACHELINE, LARGE_SLOTS_SLABNUM};

fn tot_slots(slabnum: usize) -> usize {
    slabnum_to_numareas(slabnum) * slabnum_to_numslots(slabnum)
}

impl<W:Write> Statser<W> {

    pub fn new(w: W) -> Self {
	let mut ns = Statser {
	    w,
	    allocs_current: Vec::with_capacity(NUM_SLABS),
	    allocs_total: Vec::with_capacity(NUM_SLABS),
	    allocs_highwater: Vec::with_capacity(NUM_SLABS),
	    slabareaoverflows: Vec::with_capacity(NUM_SLABS),
	    ptr2slabnum: HashMap::with_capacity(100_000_000),
	    ptr2ob: HashMap::with_capacity(100_000_000),
	    cbs: HashSet::with_capacity(100_000_000)
	};

	ns.allocs_current.resize(NUM_SLABS, 0); // initialize elements to 0
	ns.allocs_total.resize(NUM_SLABS, 0); // initialize elements to 0
	ns.allocs_highwater.resize(NUM_SLABS, 0); // initialize elements to 0
	ns.slabareaoverflows.resize(NUM_SLABS, 0); // initialize elements to 0

	eprintln!("{:>5} {:>8} {:>11} {:>11} {:>7} {:>7} {:>15}", "slab#", "size", "slots", "highwater", "overs", "overa", "tot");
	eprintln!("{:>5} {:>8} {:>11} {:>11} {:>7} {:>7} {:>15}", "-----", "----", "-----", "---------", "-----", "-----", "---");

	ns
    }

    fn write_stats(&mut self) {
	writeln!(self.w, "{:>5} {:>8} {:>11} {:>11} {:>7} {:>7} {:>15}", "slab#", "size", "slots", "highwater", "overf", "overa", "tot").unwrap();
	writeln!(self.w, "{:>5} {:>8} {:>11} {:>11} {:>7} {:>7} {:>15}", "-----", "----", "-----", "---------", "-----", "-----", "---").unwrap();
	for i in 0..OVERSIZE_SLABNUM {
	    let hw = self.allocs_highwater[i];
	    let slotsperslab = slabnum_to_numslots(i);
	    let overf = hw / slotsperslab;
	    writeln!(self.w, "{:>5} {:>8} {:>11} {:>11} {:>7} {:>7} {:>15}", i, conv(slabnum_to_slotsize(i)), slotsperslab.separate_with_commas(), hw.separate_with_commas(), overf, self.slabareaoverflows[i], self.allocs_total[i].separate_with_commas()).unwrap();
	}

	writeln!(self.w, "  >{:>2} >{:>7} >{:>10} {:>11} ({:>5}) ({:>5}) {:>15}", OVERSIZE_SLABNUM-1, conv(slabnum_to_slotsize(OVERSIZE_SLABNUM-1)), slabnum_to_numslots(OVERSIZE_SLABNUM-1).separate_with_commas(), self.allocs_highwater[OVERSIZE_SLABNUM].separate_with_commas(), "N/A", "N/A", self.allocs_total[OVERSIZE_SLABNUM].separate_with_commas()).unwrap();

	let mut tot_bytes_worst = 0;
	let mut tot_bytes_smalloc = 0;
	let mut tot_bytes_saved = 0;
        writeln!(self.w, "{:>13} {:>14} {:>14} {:>14} {:>5} {:<14}", "num", "worst case", "smalloc case", "saved", "%", "realloc hist").unwrap();
        writeln!(self.w, "{:>13} {:>14} {:>14} {:>14} {:>5} {:<14}", "---", "----------", "------------", "-----", "-", "------------").unwrap();
	let mut cbs: Vec<ClosedBook> = self.cbs.drain().collect();
	cbs.sort_unstable_by(|a, b| b.cmp(a));
	for cb in cbs {
	    let mcw = cb.movecostworstperlife * cb.numlifes;
	    let mcs = cb.movecostsmalloc;
	    let saved = mcw as i64 - mcs as i64;
	    let percsaved = (saved as f64 / mcw as f64) * 100.0;
            writeln!(self.w, "{:>13} {:>14} {:>14} {:>14} {:>5.0}% {:<14}", cb.numlifes.separate_with_commas(), mcw.separate_with_commas(), mcs.separate_with_commas(), saved.separate_with_commas(), percsaved, cb.key).ok();
	    tot_bytes_worst += mcw;
	    tot_bytes_smalloc += mcs;
	    tot_bytes_saved += saved;
	}

	let totalpercsaved = (tot_bytes_saved as f64 / tot_bytes_worst as f64) * 100.0;
	writeln!(self.w, "tot bytes moved worst-case: {:>14}, tot bytes moved smalloc: {:>14}, saved from move: {:>14}, percentage saved: {:>3.0}%", tot_bytes_worst.separate_with_commas(), tot_bytes_smalloc.separate_with_commas(), tot_bytes_saved.separate_with_commas(), totalpercsaved).ok();
    }
}

use std::cmp::max;
use smalloc::{layout_to_slabnum};
impl<W: Write> EntryConsumerTrait for Statser<W> {
    fn consume_entry(&mut self, e: &Entry) {
	match e {
	    Entry::Alloc { reqsiz, reqalign, resptr } => {
		let mut slabnum = layout_to_slabnum(*reqsiz, *reqalign);
		assert!(slabnum < NUM_SLABS);

		// Overflow of slabs:
		// If all slabs of this size in all areas are full...
		assert!(self.allocs_current[slabnum] <= tot_slots(slabnum));
		while self.allocs_current[slabnum] == tot_slots(slabnum) {
		    // ... Then move to a new slabnum
		    self.slabareaoverflows[slabnum] += 1;
		    slabnum += 1;
		    assert!(slabnum < NUM_SLABS, "slabnum: {}, NUM_SLABS: {}, slabs[{}]: {}", slabnum, NUM_SLABS, slabnum-1, self.allocs_current[slabnum-1]); // Note that in smalloc we falsely claim that the "oversize" slab has 7 bytes worth of indexes, when in fact there is no slab there, we're just going to fall back to mmap() for things that big. We just pretend there are 2^64 slots in that slab so that this simulation will never overflow out of that slabnum.
		}
		assert!(self.allocs_current[slabnum] < tot_slots(slabnum));

		assert!(! self.ptr2slabnum.contains_key(resptr));
		self.ptr2slabnum.insert(*resptr, slabnum);
		assert!(! self.ptr2ob.contains_key(resptr));

		let ch: ReallocHistoryChapter = ReallocHistoryChapter {
		    newsiz: *reqsiz,
		    align: *reqalign,
		    newslabnum: slabnum
		};

		let ob: OpenBook = OpenBook {
		    chs: vec![ch]
		};

		self.ptr2ob.insert(*resptr, ob);

		self.allocs_total[slabnum] += 1;
		self.allocs_current[slabnum] += 1;

		if self.allocs_current[slabnum] > self.allocs_highwater[slabnum] {
		    self.allocs_highwater[slabnum] = self.allocs_current[slabnum];
		    let numslotsperslabonearea = slabnum_to_numslots(slabnum);
		    let numslotsperslaballareas = numslotsperslabonearea * slabnum_to_numareas(slabnum);
		    if self.allocs_current[slabnum] % numslotsperslabonearea == 0 {
			eprintln!("{:>5} {:>8} {:>11} {:>11} {:>7} {:>7} {:>15}", slabnum, conv(slabnum_to_slotsize(slabnum)), numslotsperslaballareas.separate_with_commas(), self.allocs_highwater[slabnum].separate_with_commas(), self.allocs_current[slabnum] / numslotsperslabonearea, self.slabareaoverflows[slabnum], self.allocs_total[slabnum].separate_with_commas());
		    }
		}
	    }

	    Entry::Free { oldptr } => {
		assert!(self.ptr2slabnum.contains_key(oldptr));
		assert!(self.ptr2ob.contains_key(oldptr));

		let slabnum = self.ptr2slabnum[oldptr];
		assert!(slabnum < NUM_SLABS);

		assert!(self.allocs_current[slabnum] > 0, "allocs_current[{}]: {}", slabnum, self.allocs_current[slabnum]);
		self.allocs_current[slabnum] -= 1;

		self.ptr2slabnum.remove(oldptr);

		// Now we need to collect realloc-history statistics for later reporting before we free this.
		let ob = self.ptr2ob.remove(oldptr).unwrap();
		let cb = ClosedBook::new(ob);

		// But you know what? If the worst-case cost of moved bytes in this life story was 0, then nobody cares so optimize it out...
		if cb.movecostworstperlife == 0 {
		    return
		}
		
		// Add this history into our set, summing it with any existing matching histories.
		let curcbopt = self.cbs.take(&cb);
		
		if curcbopt.is_some() {
		    let mut curcb = curcbopt.unwrap();

		    curcb.movecostsmalloc += cb.movecostsmalloc;
		    curcb.numlifes += 1;
		    
		    self.cbs.insert(curcb);
		} else {
		    self.cbs.insert(cb);
		}
	    }

	    Entry::Realloc { prevptr, prevsiz, reqalign, newsiz, resptr } => {
		assert!(self.ptr2slabnum.contains_key(prevptr));
		assert!(self.ptr2ob.contains_key(prevptr));

		let prevslabnum = self.ptr2slabnum[prevptr];
		assert!(prevslabnum < NUM_SLABS);
		assert!(layout_to_slabnum(*prevsiz, *reqalign) <= prevslabnum); // The slab num we had this ptr in (in ptr2slabnum) was big enough to hold the prevsiz&alignment.
		self.ptr2slabnum.remove(prevptr);

		self.allocs_current[prevslabnum] -= 1;

		// Now we simulate two things: 1. Promotion of growers. 2. Overflow of slabs, By "simulate" I mean that while the actual underlying allocator is going to do whatever it does with this request for realloc(), we're here going to choose a slabnum to simulate that the new pointer would be in in smalloc.

		let mut newslabnum = max(prevslabnum, layout_to_slabnum(*newsiz, *reqalign));
		assert!(newslabnum < NUM_SLABS);
		assert!(newslabnum >= prevslabnum);

		// 1. Promote growers. Any re-allocation whose new requested size exceeds its slot size gets bumped--not to the next slabnum that is just big enough--but on to slab 14, i.e. the slots just big enough to fit into one (standard) cacheline. If it is too big too fit into 64 bytes, then just promote it directly to the "large" size (<= 6,000,000 bytes).
		if newslabnum > prevslabnum {
		    // Okay this realloc required moving the object to a bigger slab. Therefore this is a "grower".
		    if newslabnum <= MAX_SLABNUM_TO_FIT_INTO_CACHELINE {
			newslabnum = MAX_SLABNUM_TO_FIT_INTO_CACHELINE;
		    } else {
			newslabnum = LARGE_SLOTS_SLABNUM;
		    }
		}

		// 2. Overflow of slabs:
		// If all slabs of this size in all areas are full...
		assert!(self.allocs_current[newslabnum] <= tot_slots(newslabnum));
		while self.allocs_current[newslabnum] == tot_slots(newslabnum) {
		    // ... Then move to a new slabnum
		    self.slabareaoverflows[newslabnum] += 1;
		    newslabnum += 1;
		    assert!(newslabnum < NUM_SLABS, "newslabnum: {}, NUM_SLABS: {}, allocs_current[{}]: {}", newslabnum, NUM_SLABS, newslabnum-1, self.allocs_current[newslabnum-1]); // Note that in smalloc we falsely claim that the "oversize" slab has 7 bytes worth of indexes, when in fact there is no slab there, we're just going to fall back to mmap() for things that big. We just pretend there are 2^64 slots in that slab so that this simulation will never overflow out of that slabnum.
		}
		assert!(self.allocs_current[newslabnum] < tot_slots(newslabnum));

		// Okay now insert this into our map from (new) pointer to newslabnum.
		assert!(! self.ptr2slabnum.contains_key(resptr));
		self.ptr2slabnum.insert(*resptr, newslabnum);

		// Move this allocation's reallocation history to the new ptr in our map.
		let mut ob = self.ptr2ob.remove(prevptr).unwrap();
		// And append this event to its reallocation history, but only if it is a realloc to larger.
		if newsiz > prevsiz {
		    ob.chs.push(ReallocHistoryChapter {
			newsiz: *newsiz, align: *reqalign, newslabnum
		    });
		}
		self.ptr2ob.insert(*resptr, ob);

		if prevslabnum != newslabnum {
		    self.allocs_total[newslabnum] += 1;
		}

		self.allocs_current[newslabnum] += 1;
		if self.allocs_current[newslabnum] > self.allocs_highwater[newslabnum] {
		    self.allocs_highwater[newslabnum] = self.allocs_current[newslabnum];

		    if self.allocs_current[newslabnum] == slabnum_to_numslots(newslabnum) {
			if newslabnum == OVERSIZE_SLABNUM {
			    eprintln!(">{:>3} >{:10} >{:>10} {:>11} {:>15}", OVERSIZE_SLABNUM-1, conv(slabnum_to_slotsize(OVERSIZE_SLABNUM-1)), slabnum_to_numslots(OVERSIZE_SLABNUM-1), self.allocs_highwater[newslabnum].separate_with_commas(), self.allocs_total[newslabnum].separate_with_commas());
			} else {
			    eprintln!("{:>4} {:>11} {:>11} {:>11} {:>15}", newslabnum, conv(slabnum_to_slotsize(newslabnum)), slabnum_to_numslots(newslabnum), self.allocs_highwater[newslabnum].separate_with_commas(), self.allocs_total[newslabnum].separate_with_commas());
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

			let prevptr = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let prevsiz = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let reqalign = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let newsiz = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			let resptr = usize::from_le_bytes(bs[i..i+sou].try_into().unwrap());
			i += sou;
			
			retentry = Some(Entry::Realloc { prevptr, prevsiz, reqalign, newsiz, resptr });
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

