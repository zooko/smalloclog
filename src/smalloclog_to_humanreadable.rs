
// use std::io::{self, Read};
use std::io::{self, BufWriter};

mod parser;
use parser::{Logger, Parser, slurp};

fn main() {
    let mut stdin = io::stdin().lock();
    let stdout = BufWriter::new(std::io::stdout());
    let stdout_logger = Logger::new(stdout);
    let mut parser = Parser::new(stdout_logger);

    slurp(&mut stdin, &mut parser);
}

