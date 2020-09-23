use std::time::Instant;

mod png;

fn main() {
    let now = Instant::now();
    let filename = std::env::args().nth(1).expect("Expected a filename");
    let f = std::fs::read(filename);

    let f = match f {
        Ok(f) => f,
        Err(e) => panic!(e),
    };

    let data: Vec<u8> = f.into_iter().collect();
    let mut parser = png::Parser::new();

    match parser.parse(data) {
        Ok(_) => {}
        Err(e) => {
            println!("Failed to parse PNG: {}", e);
            std::process::exit(-1);
        }
    }

    println!("PNG parsing took: {:?}", now.elapsed());
}
