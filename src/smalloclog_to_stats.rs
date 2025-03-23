use std::io::{self, BufWriter};

mod parser;
use parser::{Statser, Parser, slurp};

fn main() {
    let stdin = io::stdin().lock();
    let stdo = BufWriter::new(std::io::stdout());
    let statser = Statser::new(stdo);
    let parser = Parser::new(statser);

    // This returns only once stdin is exhausted.
    slurp(stdin, parser);
}

