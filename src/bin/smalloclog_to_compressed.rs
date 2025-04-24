use std::io::{self, BufWriter};

use smalloclog::compressor::{Compressor, slurp};

fn main() {
    let stdin = io::stdin().lock();
    let stdo = BufWriter::new(std::io::stdout());
    let compressor = Compressor::new(stdo);

    slurp(stdin, compressor);
}
