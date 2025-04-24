#![allow(clippy::needless_range_loop)] // I like using needless range loops more than I like using enumerate.

// XXX TODO: extend smalloclog to detect CPU number and track precisely the spread of allocations and other events across CPUs...

#[derive(Debug)]
pub enum Entry {
    Alloc {
        reqsiz: usize,
        reqalign: usize,
        resptr: usize,
    },
    Free {
        oldptr: usize,
    },
    Realloc {
        prevptr: usize,
        prevsiz: usize,
        reqalign: usize,
        newsiz: usize,
        resptr: usize,
    },
}

pub trait EntryConsumerTrait {
    fn consume_entry(&mut self, e: &Entry);
    fn done(&mut self);
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
    chunk_size_realloc: usize,
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
            chunk_size_realloc: 0,
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
            "This version of smalloclog can read only version 3 smalloclog files."
        );
        i += 1;
        self.consumedheader = true;
        self.sou = bs[i] as usize; // source usize
        self.chunk_size_alloc = 1 + 3 * self.sou;
        self.chunk_size_free = 1 + self.sou;
        self.chunk_size_realloc = 1 + 5 * self.sou;
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

                        let reqsiz = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;
                        let reqalign = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;
                        let resptr = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;

                        retentry = Some(Entry::Alloc {
                            reqsiz,
                            reqalign,
                            resptr,
                        });
                    }
                }
                b'd' => {
                    if bs.len() >= self.chunk_size_free {
                        i += 1;

                        let oldptr = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;

                        retentry = Some(Entry::Free { oldptr });
                    }
                }
                b'r' => {
                    if bs.len() >= self.chunk_size_realloc {
                        i += 1;

                        let prevptr = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;
                        let prevsiz = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;
                        let reqalign = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;
                        let newsiz = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;
                        let resptr = usize::from_le_bytes(bs[i..i + sou].try_into().unwrap());
                        i += sou;

                        retentry = Some(Entry::Realloc {
                            prevptr,
                            prevsiz,
                            reqalign,
                            newsiz,
                            resptr,
                        });
                    }
                }
                _ => {
                    let debugbuf = &bs[i..i + 60];
                    panic!(
                        "Found something unexpected in smalloclog. i: {}, bs[i..i+60]: {:?}",
                        i, debugbuf
                    );
                }
            }
        }

        (retentry, i)
    }

    /// Returns the number of bytes successfully consumed.
    pub fn try_to_consume_bytes(&mut self, bs: &[u8]) -> usize {
        let mut ourbs = bs; // Our slice (reference to bs)
        let mut retval: usize = 0; // track how many bytes we consumed to return it when we're done.

        if !self.consumedheader {
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

use std::io::BufRead;

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
