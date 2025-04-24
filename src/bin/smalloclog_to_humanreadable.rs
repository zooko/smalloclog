use std::io::{self, BufWriter};

use smalloclog::logger::Logger;
use smalloclog::parser::{Parser, slurp};

fn main() {
    let stdin = io::stdin().lock();
    let stdo = BufWriter::new(std::io::stdout());
    let logger = Logger::new(stdo);
    let parser = Parser::new(logger);

    slurp(stdin, parser);
}
