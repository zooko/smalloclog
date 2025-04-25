use std::io::{self};

use smalloclog::compressor::{Compressor, slurp_and_compress};

fn main() {
    let stdin = io::stdin().lock();
    let compressor = Compressor::new(std::io::stdout());

    slurp_and_compress(stdin, compressor);
}
