use std::io::{self, BufRead};

mod parser;
// use parser::{Logger,Parser};
use parser::{Parser};

//XXXuse firestorm::profile_fn;

//XXXfn get_and_print() -> () {
//XXX    let stats = Logger::new();
//XXX    let parser = Parser::new(stats);
//XXX
//XXX    let stdin = io::stdin();
//XXX    let mut lock = stdin.lock();   
//XXX
//XXX    loop {
//XXX	let buffer = lock.fill_buf().unwrap();
//XXX
//XXX	let len = buffer.len();
//XXX	if len == 0 {
//XXX	    // Apparently this is how we know that the stdin is closed!?
//XXX	    println!("xxx len == 0");
//XXX	    break;
//XXX	}
//XXX
//XXX	println!("xxx Buffer contains {} bytes: {:?}", len, &buffer[..len]);
//XXX
//XXX	let processed = parser.try_to_consume_bytes(buffer);
//XXX	println!("xxx processed: {}", processed);
//XXX
//XXX	lock.consume(processed);
//XXX    }
//XXX}
//XXX
//XXXfn main() {
//XXX    get_and_print();
//XXX}
