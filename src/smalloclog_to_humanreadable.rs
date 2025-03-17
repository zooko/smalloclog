use std::io::{self, BufWriter};

mod parser;
use parser::{Logger, Parser, slurp};

fn main() {
    let stdin = io::stdin().lock();
    let stdo = BufWriter::new(std::io::stdout());
    let logger = Logger::new(stdo);
    let parser = Parser::new(logger);

    slurp(stdin, parser);
}

