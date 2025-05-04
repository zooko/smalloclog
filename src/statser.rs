//use std::collections::{HashMap,HashSet};
use rustc_hash::{FxHashMap, FxHashSet};
//use rustc_hash::{FxHashmap as HashMap, FxHashSet as HashSet};

use std::io::Write;

pub struct Statser<W: Write> {
    w: W,

    // How many allocations have been requested out of each of these slabs?
    small_allocs_current: Vec<usize>,
    large_allocs_current: Vec<usize>,
    oversize_allocs_current: usize,

    small_allocs_total: Vec<usize>,
    large_allocs_total: Vec<usize>,
    oversize_allocs_total: usize,

    // What was the most allocations that were live at one time that had to be satisfied by this slab?
    small_allocs_highwater: Vec<usize>,
    large_allocs_highwater: Vec<usize>,
    oversize_allocs_highwater: usize,

    // map ptr to current Allocation
    ptr2alloca: FxHashMap<usize, Allocation>,

    // map current ptr to its so-far life history of reallocs
    ptr2ob: FxHashMap<usize, OpenBook>,

    // set of closed books of realloc histories
    cbs: FxHashSet<ClosedBook>,
}

use smalloc::{
    HUGE_SLABNUM, NUM_LARGE_SLABS, NUM_SLOTS_O, NUM_SMALL_SLABS, SIZE_OF_BIGGEST_SMALL_SLOT,
    SIZE_OF_HUGE_SLOTS, large_slabnum_to_slotsize, num_large_slots, small_slabnum_to_slotsize,
};

impl<W: Write> Statser<W> {
    pub fn new(w: W) -> Self {
        let mut ns = Statser {
            w,
            small_allocs_current: Vec::with_capacity(NUM_SMALL_SLABS),
            large_allocs_current: Vec::with_capacity(NUM_LARGE_SLABS),
            oversize_allocs_current: 0,

            small_allocs_total: Vec::with_capacity(NUM_SMALL_SLABS),
            large_allocs_total: Vec::with_capacity(NUM_LARGE_SLABS),
            oversize_allocs_total: 0,

            small_allocs_highwater: Vec::with_capacity(NUM_SMALL_SLABS),
            large_allocs_highwater: Vec::with_capacity(NUM_LARGE_SLABS),
            oversize_allocs_highwater: 0,

            ptr2alloca: FxHashMap::default(),
            ptr2ob: FxHashMap::default(),
            cbs: FxHashSet::default(),
        };

        // initialize elements of these to 0
        ns.small_allocs_current.resize(NUM_SMALL_SLABS, 0);
        ns.large_allocs_current.resize(NUM_LARGE_SLABS, 0);
        ns.oversize_allocs_current = 0;

        ns.small_allocs_total.resize(NUM_SMALL_SLABS, 0);
        ns.large_allocs_total.resize(NUM_LARGE_SLABS, 0);
        ns.oversize_allocs_total = 0;

        ns.small_allocs_highwater.resize(NUM_SMALL_SLABS, 0);
        ns.large_allocs_highwater.resize(NUM_LARGE_SLABS, 0);
        ns.oversize_allocs_highwater = 0;

        ns
    }

    fn write_stats(&mut self) {
        writeln!(
            self.w,
            "{:>5} {:>8} {:>12} {:>15}",
            "slab#", "size", "high", "tot"
        )
        .unwrap();
        writeln!(
            self.w,
            "{:>5} {:>8} {:>12} {:>15}",
            "-----", "----", "----", "---"
        )
        .unwrap();
        writeln!(self.w, "small").unwrap();
        for i in 0..NUM_SMALL_SLABS {
            writeln!(
                self.w,
                "{:>5} {:>8} {:>12} {:>15}",
                i,
                conv(small_slabnum_to_slotsize(i)),
                self.small_allocs_highwater[i].separate_with_commas(),
                self.small_allocs_total[i].separate_with_commas()
            )
            .unwrap();
        }

        writeln!(self.w, "large").unwrap();
        for i in 0..NUM_LARGE_SLABS {
            writeln!(
                self.w,
                "{:>5} {:>8} {:>12} {:>15}",
                i,
                conv(large_slabnum_to_slotsize(i)),
                self.large_allocs_highwater[i].separate_with_commas(),
                self.large_allocs_total[i].separate_with_commas()
            )
            .unwrap();
        }

        writeln!(self.w, "oversize").unwrap();
        writeln!(
            self.w,
            "   {:>2} >{:>7} {:>12} {:>15}",
            "ov",
            conv(large_slabnum_to_slotsize(HUGE_SLABNUM)),
            self.oversize_allocs_highwater.separate_with_commas(),
            self.oversize_allocs_total.separate_with_commas()
        )
        .unwrap();

        let mut tot_bytes_worst = 0;
        let mut tot_bytes_smalloc = 0;
        let mut tot_bytes_saved = 0;
        writeln!(
            self.w,
            "{:>13} {:>16} {:>14} {:>15} {:>5} {:<14}",
            "num", "worst case", "smalloc case", "saved", "%", "realloc hist"
        )
        .unwrap();
        writeln!(
            self.w,
            "{:>13} {:>16} {:>14} {:>15} {:>5} {:<14}",
            "---", "----------", "------------", "-----", "-", "------------"
        )
        .unwrap();
        let mut cbs: Vec<ClosedBook> = self.cbs.drain().collect();
        cbs.sort_unstable_by(|a, b| b.cmp(a));
        for cb in cbs {
            let mcw = cb.movecostworstperlife * cb.numlifes;
            let mcs = cb.movecostsmalloc;
            let saved = mcw as i64 - mcs as i64;
            let percsaved = (saved as f64 / mcw as f64) * 100.0;
            writeln!(
                self.w,
                "{:>13} {:>16} {:>14} {:>15} {:>5.0}% {:<14}",
                cb.numlifes.separate_with_commas(),
                mcw.separate_with_commas(),
                mcs.separate_with_commas(),
                saved.separate_with_commas(),
                percsaved,
                cb.key
            )
            .ok();
            tot_bytes_worst += mcw;
            tot_bytes_smalloc += mcs;
            tot_bytes_saved += saved;
        }

        let totalpercsaved = (tot_bytes_saved as f64 / tot_bytes_worst as f64) * 100.0;
        writeln!(self.w, "tot bytes moved worst-case: {:>16}, tot bytes moved smalloc: {:>14}, saved from move: {:>14}, percentage saved: {:>3.0}%", tot_bytes_worst.separate_with_commas(), tot_bytes_smalloc.separate_with_commas(), tot_bytes_saved.separate_with_commas(), totalpercsaved).ok();
    }
}

use std::alloc::Layout;
fn layout_to_alloca(layout: Layout) -> Allocation {
    let size = layout.size();
    assert!(size > 0);
    let alignment = layout.align();
    assert!(alignment > 0);
    assert!(
        (alignment & (alignment - 1)) == 0,
        "alignment must be a power of two"
    );
    assert!(alignment <= 4096); // We don't guarantee larger alignments than 4096

    // Round up size to the nearest multiple of alignment in order to get a slot that is aligned on that size.
    let alignedsize: usize = ((size - 1) | (alignment - 1)) + 1;

    if alignedsize <= SIZE_OF_BIGGEST_SMALL_SLOT {
        let mut smallslabnum = NUM_SMALL_SLABS - 1;
        while smallslabnum > 0 && small_slabnum_to_slotsize(smallslabnum - 1) >= alignedsize {
            smallslabnum -= 1;
        }
        assert!(smallslabnum < NUM_SMALL_SLABS);
        assert!(small_slabnum_to_slotsize(smallslabnum) >= alignedsize);
        assert!(if smallslabnum > 0 {
            small_slabnum_to_slotsize(smallslabnum - 1) < alignedsize
        } else {
            true
        });

        Allocation {
            sizeclass: SizeClass::Small,
            slabnum: smallslabnum,
        }
    } else if alignedsize <= SIZE_OF_HUGE_SLOTS {
        let mut largeslabnum = 0;
        while large_slabnum_to_slotsize(largeslabnum) < alignedsize {
            largeslabnum += 1;
        }
        assert!(largeslabnum < NUM_LARGE_SLABS);

        Allocation {
            sizeclass: SizeClass::Large,
            slabnum: largeslabnum,
        }
    } else {
        Allocation {
            sizeclass: SizeClass::Oversize,
            slabnum: 0,
        }
    }
}

use crate::parser::{Entry, EntryConsumerTrait};

impl<W: Write> EntryConsumerTrait for Statser<W> {
    fn consume_entry(&mut self, e: &Entry) {
        match e {
            Entry::Alloc {
                reqsiz,
                reqalign,
                resptr,
            } => {
                let lo = Layout::from_size_align(*reqsiz, *reqalign).unwrap();
                let alloca = layout_to_alloca(lo);

                assert!(!self.ptr2alloca.contains_key(resptr));
                self.ptr2alloca.insert(*resptr, alloca);
                assert!(!self.ptr2ob.contains_key(resptr));

                let ch: ReallocHistoryChapter = ReallocHistoryChapter {
                    newsiz: *reqsiz,
                    align: *reqalign,
                    newalloca: alloca,
                };

                let ob: OpenBook = OpenBook { chs: vec![ch] };

                self.ptr2ob.insert(*resptr, ob);

                match alloca.sizeclass {
                    SizeClass::Small => {
                        let sn = alloca.slabnum;
                        assert!(self.small_allocs_current[sn] < NUM_SLOTS_O);
                        self.small_allocs_total[sn] += 1;
                        self.small_allocs_current[sn] += 1;
                        if self.small_allocs_current[sn] > self.small_allocs_highwater[sn] {
                            self.small_allocs_highwater[sn] = self.small_allocs_current[sn];
                        }
                    }
                    SizeClass::Large => {
                        let sn = alloca.slabnum;
                        assert!(self.large_allocs_current[sn] < num_large_slots(sn));
                        self.large_allocs_total[sn] += 1;
                        self.large_allocs_current[sn] += 1;
                        if self.large_allocs_current[sn] > self.large_allocs_highwater[sn] {
                            self.large_allocs_highwater[sn] = self.large_allocs_current[sn];
                        }
                    }
                    SizeClass::Oversize => {
                        self.oversize_allocs_total += 1;
                        self.oversize_allocs_current += 1;
                        if self.oversize_allocs_current > self.oversize_allocs_highwater {
                            self.oversize_allocs_highwater = self.oversize_allocs_current;
                        }
                    }
                }
            }

            Entry::Free { oldptr } => {
                assert!(self.ptr2alloca.contains_key(oldptr));
                assert!(self.ptr2ob.contains_key(oldptr));

                let alloca = self.ptr2alloca[oldptr];
                match alloca.sizeclass {
                    SizeClass::Small => {
                        let sn = alloca.slabnum;
                        assert!(self.small_allocs_current[sn] > 0);
                        self.small_allocs_current[sn] -= 1;
                    }
                    SizeClass::Large => {
                        let sn = alloca.slabnum;
                        assert!(self.large_allocs_current[sn] > 0);
                        self.large_allocs_current[sn] -= 1;
                    }
                    SizeClass::Oversize => {
                        assert!(self.oversize_allocs_current > 0);
                        self.oversize_allocs_current -= 1;
                    }
                }

                self.ptr2alloca.remove(oldptr);

                // Now we need to collect realloc-history statistics for later reporting before we free this.
                let ob = self.ptr2ob.remove(oldptr).unwrap();
                let cb = ClosedBook::new(ob);

                // But you know what? If the worst-case cost of moved bytes in this life story was 0, then nobody cares so optimize it out...
                if cb.movecostworstperlife == 0 {
                    return;
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

            Entry::Realloc {
                prevptr,
                prevsiz,
                reqalign,
                newsiz,
                resptr,
            } => {
                assert!(*prevsiz > 0);
                assert!(*reqalign > 0);
                assert!(
                    (*reqalign & (*reqalign - 1)) == 0,
                    "alignment must be a power of two"
                );
                assert!(*reqalign <= 4096); // We don't guarantee larger alignments than 4096
                assert!(*newsiz > 0);
                assert!(self.ptr2alloca.contains_key(prevptr));
                assert!(self.ptr2ob.contains_key(prevptr));

                let prevalloca = self.ptr2alloca[prevptr];
                self.ptr2alloca.remove(prevptr);

                let newlo = Layout::from_size_align(*newsiz, *reqalign).unwrap();
                let tempalloca = layout_to_alloca(newlo);

                let newalloca = match prevalloca.sizeclass {
                    SizeClass::Small => match tempalloca.sizeclass {
                        SizeClass::Small => {
                            if tempalloca.slabnum > prevalloca.slabnum {
                                assert!(self.small_allocs_current[prevalloca.slabnum] > 0);
                                self.small_allocs_current[prevalloca.slabnum] -= 1;
                                assert!(self.large_allocs_current[0] < num_large_slots(0));
                                self.large_allocs_current[0] += 1;
                                if self.large_allocs_current[0] > self.large_allocs_highwater[0] {
                                    self.large_allocs_highwater[0] = self.large_allocs_current[0];
                                }
                                Allocation {
                                    sizeclass: SizeClass::Large,
                                    slabnum: 0,
                                }
                            } else {
                                prevalloca
                            }
                        }
                        SizeClass::Large => {
                            assert!(self.small_allocs_current[prevalloca.slabnum] > 0);
                            self.small_allocs_current[prevalloca.slabnum] -= 1;
                            if tempalloca.slabnum == 0 {
                                assert!(self.large_allocs_current[0] < num_large_slots(0));
                                self.large_allocs_current[0] += 1;
                                if self.large_allocs_current[0] > self.large_allocs_highwater[0] {
                                    self.large_allocs_highwater[0] = self.large_allocs_current[0];
                                }
                                tempalloca
                            } else {
                                assert!(
                                    self.large_allocs_current[HUGE_SLABNUM]
                                        < num_large_slots(HUGE_SLABNUM)
                                );
                                self.large_allocs_current[HUGE_SLABNUM] += 1;
                                if self.large_allocs_current[HUGE_SLABNUM]
                                    > self.large_allocs_highwater[HUGE_SLABNUM]
                                {
                                    self.large_allocs_highwater[HUGE_SLABNUM] =
                                        self.large_allocs_current[HUGE_SLABNUM];
                                }
                                Allocation {
                                    sizeclass: SizeClass::Large,
                                    slabnum: HUGE_SLABNUM,
                                }
                            }
                        }
                        SizeClass::Oversize => tempalloca,
                    },
                    SizeClass::Large => match tempalloca.sizeclass {
                        SizeClass::Small => prevalloca,
                        SizeClass::Large => {
                            if tempalloca.slabnum > prevalloca.slabnum {
                                assert!(self.large_allocs_current[prevalloca.slabnum] > 0);
                                self.large_allocs_current[prevalloca.slabnum] -= 1;
                                assert!(
                                    self.large_allocs_current[HUGE_SLABNUM]
                                        < num_large_slots(HUGE_SLABNUM)
                                );
                                self.large_allocs_current[HUGE_SLABNUM] += 1;
                                if self.large_allocs_current[HUGE_SLABNUM]
                                    > self.large_allocs_highwater[HUGE_SLABNUM]
                                {
                                    self.large_allocs_highwater[HUGE_SLABNUM] =
                                        self.large_allocs_current[HUGE_SLABNUM];
                                }
                                Allocation {
                                    sizeclass: SizeClass::Large,
                                    slabnum: HUGE_SLABNUM,
                                }
                            } else {
                                prevalloca
                            }
                        }
                        SizeClass::Oversize => tempalloca,
                    },
                    SizeClass::Oversize => prevalloca,
                };

                // Okay now insert this into our map from (new) pointer to newalloca.
                assert!(!self.ptr2alloca.contains_key(resptr));
                self.ptr2alloca.insert(*resptr, newalloca);

                // Move this allocation's reallocation history to the new ptr in our map.
                let mut ob = self.ptr2ob.remove(prevptr).unwrap();

                if newsiz > prevsiz {
                    ob.chs.push(ReallocHistoryChapter {
                        newsiz: *newsiz,
                        align: *reqalign,
                        newalloca,
                    });
                }

                self.ptr2ob.insert(*resptr, ob);
            }
        }
    }

    fn done(&mut self) {
        self.write_stats();

        writeln!(self.w, "done:-}}!").ok();
    }
}

struct ReallocHistoryChapter {
    newsiz: usize,
    align: usize,
    newalloca: Allocation,
}

struct OpenBook {
    chs: Vec<ReallocHistoryChapter>,
}

struct ClosedBook {
    numlifes: usize, // Number of lifes which follow this exact realloc progression
    movecostworstperlife: usize,

    movecostsmalloc: usize,
    key: String,
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
            if ch.newalloca != prevch.newalloca {
                movecostsmalloc += prevch.newsiz;
            }
            prevch = ch;
        }

        ClosedBook {
            numlifes: 1,
            movecostworstperlife,
            movecostsmalloc,
            key,
        }
    }
}

use std::hash::{Hash, Hasher};
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

impl Eq for ClosedBook {}

use std::cmp::{Ord, Ordering, PartialOrd};
impl PartialOrd for ClosedBook {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ClosedBook {
    fn cmp(&self, other: &Self) -> Ordering {
        let stwc = self.movecostworstperlife * self.numlifes;
        let otwc = other.movecostworstperlife * other.numlifes;
        stwc.cmp(&otwc)
    }
}

#[derive(PartialEq, Copy, Clone)]
enum SizeClass {
    Small,
    Large,
    Oversize,
}

use std::cmp::PartialEq;

#[derive(PartialEq, Copy, Clone)]
struct Allocation {
    sizeclass: SizeClass,
    slabnum: usize,
}

use bytesize::ByteSize;
fn conv(size: usize) -> String {
    let byte_size = ByteSize::b(size as u64);
    byte_size.to_string()
}
use thousands::Separable;
