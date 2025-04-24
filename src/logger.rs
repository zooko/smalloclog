use std::io::Write;

use crate::parser::{Entry, EntryConsumerTrait};

pub struct Logger<W: Write> {
    w: W,
}

impl<W: Write> Logger<W> {
    pub fn new(w: W) -> Self {
        Self { w }
    }
}

impl<W: Write> EntryConsumerTrait for Logger<W> {
    fn consume_entry(&mut self, e: &Entry) {
        match e {
            Entry::Alloc {
                reqsiz,
                reqalign,
                resptr,
            } => writeln!(self.w, "alloc({}, {}) -> 0x{:x}", reqsiz, reqalign, resptr).ok(),
            Entry::Free { oldptr } => writeln!(self.w, "dealloc(0x{:x})", oldptr).ok(),
            Entry::Realloc {
                prevptr,
                prevsiz,
                reqalign,
                newsiz,
                resptr,
            } => writeln!(
                self.w,
                "realloc(0x{:x}, {}, {}, {}) -> 0x{:x}",
                prevptr, prevsiz, reqalign, newsiz, resptr
            )
            .ok(),
        }
        .unwrap()
    }
    fn done(&mut self) {
        writeln!(self.w, "done:-)!").ok();
    }
}
