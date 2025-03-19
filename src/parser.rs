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
    newsc: usize
}

struct OpenBook {
    chs: Vec<ReallocHistoryChapter>,
}

struct ClosedBook {
    movecostworst: usize,
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
	let mut movecostworst = 0;
	let mut movecostsmalloc = 0;
	for ch in &chs[1..] {
	    movecostworst += ch.newsiz - prevch.newsiz;
	    if ch.newsc != prevch.newsc {
		movecostsmalloc += prevch.newsiz;
	    }
	    prevch = ch;
	}

	ClosedBook {
	    movecostworst,
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
	self.movecostworst.cmp(&other.movecostworst)
    }
}

pub struct Statser<W: Write> {
    w: W,

    // How many allocations have been requested out of each of these slabs (each "slab" having a fixed number of fixed-size "slots").
    slabs_now: Vec<usize>,
    slabs_totallocs: Vec<usize>,
    slabs_highwater: Vec<usize>,

    // map ptr to current SC
    ptr2sc: HashMap<usize, usize>,

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

use smalloc::{sizeclass_to_slots,OVERSIZE_SC,NUM_SCS};
impl<W:Write> Statser<W> {

    pub fn new(w: W) -> Self {
	let mut ns = Statser {
	    w,
	    slabs_now: Vec::with_capacity(NUM_SCS),
	    slabs_totallocs: Vec::with_capacity(NUM_SCS),
	    slabs_highwater: Vec::with_capacity(NUM_SCS),
	    ptr2sc: HashMap::with_capacity(100_000_000),
	    ptr2ob: HashMap::with_capacity(100_000_000),
	    cbs: HashSet::with_capacity(100_000_000)
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

	let mut tot_bytes_worst = 0;
	let mut tot_bytes_smalloc = 0;
	let mut tot_bytes_saved = 0;
        writeln!(self.w, "{:>14} {:>14} {:>14} {:<14}", "worst case", "smalloc case", "saved", "realloc hist").unwrap();
        writeln!(self.w, "{:>14} {:>14} {:>14} {:<14}", "----------", "------------", "-----", "------------").unwrap();
	let mut cbs: Vec<ClosedBook> = self.cbs.drain().collect();
	cbs.sort_unstable_by(|a, b| b.cmp(a));
	for cb in cbs {
	    let mcw = cb.movecostworst;
	    let mcs = cb.movecostsmalloc;
	    let saved = mcw-mcs;
            writeln!(self.w, "{:>14} {:>14} {:>14} {:<14}", mcw.separate_with_commas(), mcs.separate_with_commas(), saved.separate_with_commas(), cb.key).ok();
	    tot_bytes_worst += mcw;
	    tot_bytes_smalloc += mcs;
	    tot_bytes_saved += saved;
	}

	writeln!(self.w, "tot bytes moved worst-case: {}, tot bytes moved smalloc: {}, saved from move: {}", tot_bytes_worst.separate_with_commas(), tot_bytes_smalloc.separate_with_commas(), tot_bytes_saved.separate_with_commas()).ok();
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
		let prevsc = layout_to_sizeclass(*reqsiz, *reqalign);
		assert!(prevsc < NUM_SCS);

		// Overflow of slabs:
		let sc = self.find_next_size_class_with_open_slot(prevsc);

		assert!(! self.ptr2sc.contains_key(resptr));
		self.ptr2sc.insert(*resptr, sc);
		assert!(! self.ptr2ob.contains_key(resptr));

		let ch: ReallocHistoryChapter = ReallocHistoryChapter {
		    newsiz: *reqsiz,
		    align: *reqalign,
		    newsc: sc
		};

		let ob: OpenBook = OpenBook {
		    chs: vec![ch]
		};

		self.ptr2ob.insert(*resptr, ob);

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
		assert!(self.ptr2ob.contains_key(oldptr));

		let sc = self.ptr2sc[oldptr];
		assert!(sc < NUM_SCS);

		assert!(self.slabs_now[sc] > 0, "slabs[{}]: {}", sc, self.slabs_now[sc]);
		self.slabs_now[sc] -= 1;

		self.ptr2sc.remove(oldptr);

		// Now we need to collect realloc-history statistics for later reporting before we free this.
		let ob = self.ptr2ob.remove(oldptr).unwrap();
		let cb = ClosedBook::new(ob);

		// But you know what? If the worst-case cost of moved bytes in this life story was 0, then nobody cares so optimize it out...
		if cb.movecostworst > 0 {
		    // Add this history into our set, summing it with any existing matching histories.
		    let curcbopt = self.cbs.take(&cb);
		
		    if curcbopt.is_some() {
			let mut curcb = curcbopt.unwrap();

			let oldmovecostworst = curcb.movecostworst; // XXX only for assert below
			let oldmovecostsmalloc = curcb.movecostsmalloc; // XXX only for assert below
			let mut newmovecostworst = curcb.movecostworst;
			let mut newmovecostsmalloc = curcb.movecostsmalloc;

			newmovecostworst += cb.movecostworst;
			newmovecostsmalloc += cb.movecostsmalloc;

			curcb.movecostworst = newmovecostworst;
			curcb.movecostsmalloc = newmovecostsmalloc;

			self.cbs.insert(curcb);
			
			assert!(self.cbs.get(&cb).unwrap().movecostworst > oldmovecostworst, "omcw: {}, cur mcw: {}", oldmovecostworst, self.cbs.get(&cb).unwrap().movecostworst);
			assert!(self.cbs.get(&cb).unwrap().movecostsmalloc >= oldmovecostsmalloc);
			
			// XXX check that this updates the copy inside the hashmap... right?!Have to do this by-reference ??
		    } else {
			self.cbs.insert(cb);
		    }
		}
	    }

	    Entry::Realloc { prevptr, prevsiz, reqalign, newsiz, resptr } => {
		assert!(self.ptr2sc.contains_key(prevptr));
		assert!(self.ptr2ob.contains_key(prevptr));

		let prevsc = self.ptr2sc[prevptr];
		assert!(prevsc < NUM_SCS);
		assert!(layout_to_sizeclass(*prevsiz, *reqalign) <= prevsc); // The SC we had this ptr in (in ptr2sc) was big enough to hold the prevsiz&alignment.
		self.ptr2sc.remove(prevptr);

		self.slabs_now[prevsc] -= 1;

		let mut newsc = layout_to_sizeclass(*newsiz, *reqalign);
		assert!(newsc < NUM_SCS);

		// Now we simulate two things: 1. Promotion of growers. 2. Overflow of slabs, By "simulate" I mean that while the actual underlying allocator is going to do whatever it does with this request for realloc(), we're here going to choose a sizeclass to simulate that the new pointer would be in in smalloc.

		// (Note: we're assuming here the "worst-case scenario", where one CPU did all of these allocations. In practice we may get a little relief of the congestion of these slabs from the allocations being spread out over multiple CPUs, but we don't want to count on that necessarily, so let's look at the worst-case scenario first...)

		// 1. Promote growers. Any re-allocation which exceeds its slot gets bumped, not to the next sizeclass that is big enough, but also to the next sizeclass group. The groups are: A. Things that can pack into a 64-byte cache line, B. Things that can pack into a 4096-byte memory page, and C. The huge slots slab. Grower promotion can promote an allocation to group B or group C.
		//XXXif newsc > prevsc {
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

		// Move this allocation's reallocation history to the new ptr in our map.
		let mut ob = self.ptr2ob.remove(prevptr).unwrap();
		// And append this event to its reallocation history, but only if it is a realloc to larger.
		if newsiz > prevsiz {
		    
		    ob.chs.push(ReallocHistoryChapter {
			newsiz: *newsiz, align: *reqalign, newsc
		    });
		}
		self.ptr2ob.insert(*resptr, ob);

		if prevsc != newsc {
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

