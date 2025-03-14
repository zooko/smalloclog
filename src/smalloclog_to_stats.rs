
use std::io::{self, Read};

use smalloclog::smalloclog_to_stats;

//XXXuse firestorm::profile_fn;

fn get_and_print() -> () {
    let mut buffer = Vec::new();
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    
    if let Err(e) = handle.read_to_end(&mut buffer) {
        eprintln!("Failed to read from stdin: {}", e);
        return;
    }
    
    println!("{}", smalloclog_to_stats(&buffer));
}

fn main() {
    get_and_print();
}
