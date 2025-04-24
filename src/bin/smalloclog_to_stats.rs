use std::io::{self, BufWriter};

use smalloclog::parser::{Parser, slurp};
use smalloclog::statser::Statser;

fn main() {
    let stdin = io::stdin().lock();
    let stdo = BufWriter::new(std::io::stdout());
    let statser = Statser::new(stdo);
    let parser = Parser::new(statser);

    // This returns only once stdin is exhausted.
    slurp(stdin, parser);
}
