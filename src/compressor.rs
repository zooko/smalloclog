use smalloc::MAX_ALIGNMENT;
use std::io::Write;

const MSU: usize = 8; // max source usize

const CAC_CAPACITY: usize = 2usize.pow(24);

use rustc_hash::FxHashMap;

struct CoolWordCompressor<const N: usize> {
    idx2word: [u64; N],
    word2idx: FxHashMap<u64, usize>,
    nexti: usize,
    filled: bool, // the first time we fill up idx2word, this gets set to true
}

impl<const N: usize> CoolWordCompressor<N> {
    pub fn new() -> Self {
        Self {
            idx2word: [0; N],
            word2idx: FxHashMap::default(),
            nexti: 0,
            filled: false,
        }
    }

    const fn wrapping_incr(x: usize) -> usize {
        assert!(x <= N);
        (x + 1) % N
    }

    const fn wrapping_decr(x: usize) -> usize {
        assert!(x <= N);
        (x + N - 1) % N
    }

    /// Returns x minus y, wrapping if necessary.
    const fn wrapping_sub(x: usize, y: usize) -> usize {
        assert!(x <= N);
        assert!(y <= N);
        (x + N - y) % N
    }

    /// Looks up word in the dict, appends it (as the new most-recently-used) if it is absent (and evicts the least-recently-used if the dict is full), or else moves it to the front (most-recently-used) slot if it is present (by swapping it with the current most-recently-used). Then it returns the "distance", which is the distance from the index that this appeared in before it was updated to the current most-recently-used index, or None if it didn't previously appear in the dict. Got that? Good.
    fn compress_word(&mut self, word: u64) -> Option<usize> {
        //XXX switch to .entry() to avoid one lookup in the table
        let optoldidx = self.word2idx.get(&word);
        match optoldidx {
            None => {
                // Append this word to the dict as the most-recently-used word, evicting the unlucky one if the dict is full.
                if self.filled {
                    // Remove the unlucky one
                    let unluckyword = self.idx2word[self.nexti];
                    self.word2idx.remove(&unluckyword);
                }
                self.idx2word[self.nexti] = word;
                self.word2idx.insert(word, self.nexti);
                self.nexti = CoolWordCompressor::<N>::wrapping_incr(self.nexti);
                if self.nexti == 0 {
                    self.filled = true;
                }

                None
            }
            Some(&oldidx) => {
                let mrui = CoolWordCompressor::<N>::wrapping_decr(self.nexti); // most-recently-used index is 1 less than next index

                // Fetch the current (about to be previous) most-recently-used word
                let cur_mru_word = self.idx2word[mrui];

                // Put the word that just got refreshed into the MRU slot:
                self.idx2word[mrui] = word;
                self.word2idx.insert(word, mrui);

                // Put the previous (was current) most-recently-used word into the slot vacated by this word:
                self.idx2word[oldidx] = cur_mru_word;
                self.word2idx.insert(cur_mru_word, oldidx);

                Some(CoolWordCompressor::<N>::wrapping_sub(mrui, oldidx))
            }
        }
    }

    /// Looks up word indicated by lookback in the dict.
    /// "lookback" is how many indexes backwards from the current most-recently-used index to fetch the word from.
    fn decompress_word(&mut self, lookback: usize) -> u64 {
        let idx = CoolWordCompressor::<N>::wrapping_sub(self.nexti, lookback + 1);
        let word = self.idx2word[idx];

        assert!(
            self.word2idx.contains_key(&word),
            "idx: {}, mri: {}, n2a: {:?} (len {}), a2n: {:?}, a: {:?}",
            idx,
            self.nexti,
            self.idx2word,
            self.idx2word.len(),
            self.word2idx,
            word
        );

        word
    }
}

/// All valid alignments can be compressed into one byte (actually
/// just the least-significant four bits -- an integer 0-12 -- but close
/// enough).
pub fn statelessly_compress_alignment(alignment: usize) -> u8 {
    assert!(alignment > 0);
    assert!(
        (alignment & (alignment - 1)) == 0,
        "alignment must be a power of two"
    );
    assert!(alignment <= MAX_ALIGNMENT); // We don't guarantee larger alignments than 4096
    alignment.ilog2() as u8
}

pub fn statelessly_decompress_alignment(compressed_alignment: u8) -> usize {
    2usize.pow(compressed_alignment as u32)
}

use isize;
pub fn statelessly_compress_size(size: usize) -> Vec<u8> {
    assert!(size > 0);
    assert!(size <= isize::MAX as usize);

    // All valid sizes require up to 9 bytes (although they'll often compress down to fewer)
    let mut res: Vec<u8> = Vec::with_capacity(9);
    let mut residual = size;
    while residual >= 128 {
        res.push(((residual % 128) | 0b10000000) as u8);
        //eprintln!("c buf[{}]: {}", i, buf[i]);
        residual /= 128;
    }
    res.push(residual as u8);

    res
}

pub fn statelessly_decompress_size(compressed_size: &[u8]) -> usize {
    assert!(compressed_size.len() <= 9);
    let leng = compressed_size.len();

    let mut result: usize = 0;
    let mut i: usize = 0;
    while i < leng {
        //eprintln!("d cs[{}]: {}", i, compressed_size[i]);
        result += ((compressed_size[i] % 128) as usize) * (128_usize.pow(i as u32));
        //eprintln!("d result: {}", result);
        i += 1;
    }

    result
}

pub struct Compressor<W: Write> {
    consumedheader: bool,
    sou: usize,

    // How many bytes do we need to read to decode each of these 4 things:
    chunk_size_header: usize,
    chunk_size_alloc: usize,
    chunk_size_free: usize,
    chunk_size_realloc: usize,

    w: W,
    cwc: CoolWordCompressor<CAC_CAPACITY>,
}

impl<W: Write> Compressor<W> {
    pub fn new(w: W) -> Self {
        Self {
            consumedheader: false,
            sou: 0,
            chunk_size_header: 2,
            chunk_size_alloc: 0,
            chunk_size_free: 0,
            chunk_size_realloc: 0,
            w,
            cwc: CoolWordCompressor::new(),
        }
    }

    /// Returns the number of bytes successfully consumed. If the
    /// return value is non-zero then the header was successfully
    /// consumed and the self.sou value was populated.
    fn try_to_consume_header_bytes(&mut self, bs: &[u8]) -> usize {
        let mut i: usize = 0;
        if bs.len() < self.chunk_size_header {
            return 0;
        }

        assert!(
            bs[i] == b'3',
            "This version of smalloclog-compressor can read only version 3 smalloclog files."
        );
        i += 1;
        self.consumedheader = true;
        self.sou = bs[i] as usize; // source usize
        assert!(self.sou <= MSU);

        self.chunk_size_alloc = 1 + 3 * self.sou;
        self.chunk_size_free = 1 + self.sou;
        self.chunk_size_realloc = 1 + 5 * self.sou;

        i += 1;

        assert!(i == self.chunk_size_header);

        self.chunk_size_header
    }

    //     /// Returns number of bytes successfully consumed.
    //     fn try_to_parse_and_compress_next_entry(&self, bs: &[u8]) -> usize {
    // 	let mut retentry: Entry;
    // 	let mut i: usize = 0; // consumed bytes
    // 	let sou = self.sou; // to save a few chars of reading this source code

    // 	if !bs.is_empty() {
    // 	    match bs[i] {
    // 		b'a' => {
    // 		    if bs.len() >= self.chunk_size_alloc {
    // 			i += 1;

    //                         let opti = self.cwc.compress_word(&bs[i..i+sou]);
    //                         match opti {
    //                             None => {
    //                                 self.w.write_all(&bs[i..i+sou]);
    //                             }
    //                             Some(i) => {
    // xxx
    //                             }
    //                         }
    //                         let reqsiz: [u8; MSU];
    //                         reqsiz.copy_from_slice(&);

    //                         // lookup reqsize in the dict
    //                         match self.word2num.get(reqsiz) {
    //                             None => {
    //                                 // insert reqsiz into the dict
    //                             }
    //                             Some(k) => {
    //                             }
    //                         }
    //                         let k = self.word2num

    // xxx
    // 			i += sou;

    //                         let reqalign: [u8; MSU];
    //                         reqalign.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;
    //                         let resptr: [u8; MSU];
    //                         resptr.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;

    //                         self.w.write_all(b"a");
    // 			retentry = Entry::Alloc { reqsiz, reqalign, resptr };

    //                         // 0b00 means this is an allocation event
    //                         //xxxself.w.write_all(b[0]).unwrap();
    // 		    }
    // 		}
    // 		b'd' => {
    // 		    if bs.len() >= self.chunk_size_free {
    // 			i += 1;

    // 			let oldptr: [u8; MSU];
    //                         oldptr.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;

    // 			retentry = Entry::Free { oldptr };
    // 		    }
    // 		}
    // 		b'r' => {
    // 		    if bs.len() >= self.chunk_size_realloc {
    // 			i += 1;

    // 			let prevptr: [u8; MSU];
    //                         prevptr.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;
    // 			let prevsiz: [u8; MSU];
    //                         prevsiz.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;
    // 			let reqalign: [u8; MSU];
    //                         reqalign.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;
    // 			let newsiz: [u8; MSU];
    //                         newsiz.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;
    // 			let resptr: [u8; MSU];
    //                         resptr.copy_from_slice(&bs[i..i+sou]);
    // 			i += sou;

    // 			retentry = Entry::Realloc { prevptr, prevsiz, reqalign, newsiz, resptr };
    // 		    }
    // 		}
    // 		_ => {
    // 		    let debugbuf = &bs[i..i+60];
    // 		    panic!("Found something unexpected in smalloclog. i: {}, bs[i..i+60]: {:?}", i, debugbuf);
    // 		}
    // 	    }
    // 	}

    // 	(retentry, i)
    //     }

    //    pub fn xxx
    //     pub fn try_to_consume_bytes(&mut self, bs: &[u8]) -> usize {
    // 	let mut ourbs = bs; // Our slice (reference to bs)
    // 	let mut retval: usize = 0; // track how many bytes we consumed to return it when we're done.

    // 	if ! self.consumedheader {

    // 	    let hbs = self.try_to_consume_header_bytes(ourbs);
    // 	    if hbs == 0 {
    // 		return 0;
    // 	    }

    // 	    retval += hbs;

    // 	    // Slice from the first un-consumed byte onwards.
    // 	    ourbs = &ourbs[retval..]
    // 	}

    // 	loop {
    // 	    let (e, j) = self.try_to_parse_next_entry(ourbs);

    // 	    if j == 0 {
    // 		return retval;
    // 	    }

    // 	    ourbs = &ourbs[j..];
    // 	    retval += j;

    // 	    self.consume_entry(&e);
    // 	}
    //     }
    // }
}
use std::io::BufRead;

const BUFSIZ: usize = 2usize.pow(20);

/// This function doesn't return until `r` returns 0 from a call to read(). Which hopefully won't happen until we're done, ie the end of the file has been reached if `r` is a file, or the pipe has been closed if `r` is a pipe.
pub fn slurp<R: BufRead, W: Write>(mut _r: R, mut _c: Compressor<W>) {
    // let mut buffer: [u8; BUFSIZ] = [0; BUFSIZ];
    // let mut bytesfilled: usize = 0;

    // loop {
    //     let bytesread = r.read(&mut buffer[bytesfilled..]).unwrap();
    //     if bytesread == 0 {
    //         c.done();
    //         return;
    //     }

    //     bytesfilled += bytesread;

    //     let processed = c.try_to_consume_bytes(&buffer[..bytesfilled]);

    //     assert!(processed <= bytesfilled);

    //     // Copy any leftover bytes from the end to the beginning.
    //     buffer.copy_within(processed..bytesfilled, 0);

    //     bytesfilled -= processed;
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    //XXXuse rand::RngCore;
    //XXXuse rand::SeedableRng;
    //XXXuse rand::rngs::StdRng;

    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    const CAC_CAPACITY_FOR_TESTING: usize = 3;

    #[test]
    fn test_cac() {
        let mut r = SmallRng::seed_from_u64(0);

        let mut cac = CoolWordCompressor::<CAC_CAPACITY_FOR_TESTING>::new();

        assert_eq!(cac.nexti, 0);

        // insert one item! whee!
        let a = r.random::<u64>();

        let opti = cac.compress_word(a);
        assert!(opti.is_none());
        assert_eq!(cac.nexti, 1);

        // what happens if you compress the same item again?
        let opti2 = cac.compress_word(a);
        assert!(opti2.is_some());
        assert_eq!(opti2.unwrap(), 0);
        assert_eq!(cac.nexti, 1);

        let deceda = cac.decompress_word(0);
        assert_eq!(deceda, a);
        assert_eq!(cac.nexti, 1);

        // a new word (which is different from the first one)
        let mut a2 = r.random::<u64>();
        while a2 == a {
            a2 = r.random::<u64>();
        }

        let opti2 = cac.compress_word(a2);
        assert!(opti2.is_none());

        assert_eq!(cac.nexti, 2);
        let deceda2 = cac.decompress_word(0);
        assert_eq!(a2, deceda2);
        assert_eq!(cac.nexti, 2);

        let deceda_again = cac.decompress_word(1);
        assert_eq!(deceda_again, deceda);
        assert_eq!(cac.nexti, 2);

        // a new word (which is different from the first two)
        let mut a3 = r.random::<u64>();
        while a3 == a || a3 == a2 {
            a3 = r.random::<u64>();
        }

        let opti3 = cac.compress_word(a3);
        assert_eq!(cac.nexti, 0); // wrapped because 3 >= CAC_CAPACITY_FOR_TESTING
        assert!(opti3.is_none());

        // Okay now all three of them should be in here, in this order:
        assert_eq!(
            a3,
            cac.decompress_word(0),
            "cac.ni: {}, a3: {:?}, cac.idx2word: {:?}, cac.word2idx: {:?}",
            cac.nexti,
            a3,
            cac.idx2word,
            cac.word2idx
        );
        assert_eq!(a2, cac.decompress_word(1));
        assert_eq!(a, cac.decompress_word(2));

        // a new word (which is different from the first three)
        let mut a4 = r.random::<u64>();
        while a4 == a || a4 == a2 || a4 == a3 {
            a4 = r.random::<u64>();
        }

        let opti4 = cac.compress_word(a4);
        assert!(opti4.is_none());
        assert_eq!(cac.nexti, 1); // wrapped because 3 >= CAC_CAPACITY_FOR_TESTING

        // Now all three of the most recent ones should be in here, in this order:
        assert_eq!(a4, cac.decompress_word(0));
        assert_eq!(a3, cac.decompress_word(1));
        assert_eq!(a2, cac.decompress_word(2));

        // Since we added a fourth, then the first one -- `a` -- got evicted...
        assert!(!cac.word2idx.contains_key(&a));
        assert_eq!(cac.idx2word.len(), CAC_CAPACITY_FOR_TESTING);

        // Now if we re-add a3, it will become the most recent (lookback 0), swapping with a4 (which will become lookback 1)
        let opti5 = cac.compress_word(a3);
        assert!(opti5.is_some());
        assert_eq!(cac.nexti, 1); // wrapped because 3 >= CAC_CAPACITY_FOR_TESTING
        // The lookback that a3 *had* before we promoted it to the front was 1:
        assert_eq!(opti5.unwrap(), 1);
    }

    #[test]
    fn test_roundtrip_stateless_alignment() {
        for i in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
            let compeda = statelessly_compress_alignment(i);
            let decompeda = statelessly_decompress_alignment(compeda);
            assert_eq!(i, decompeda);
        }
    }

    #[test]
    fn test_roundtrip_stateless_size() {
        for i in 1..2usize.pow(10) {
            eprintln!("t i: {}", i);
            let compeds = statelessly_compress_size(i);
            let decompeds = statelessly_decompress_size(&compeds);
            assert_eq!(i, decompeds, "compeds: {:?}", compeds);
        }

        for expo in 2..9 {
            for i in 2usize.pow(expo * 7)..2usize.pow(expo * 7) + 259 {
                let compeds = statelessly_compress_size(i);
                let decompeds = statelessly_decompress_size(&compeds);
                assert_eq!(i, decompeds);
            }
        }

        for i in 2usize.pow(63) - 512..2usize.pow(63) {
            let compeds = statelessly_compress_size(i);
            let decompeds = statelessly_decompress_size(&compeds);
            assert_eq!(i, decompeds);
        }
    }
}
