use std::collections::VecDeque;

fn parse_png_header(data: &mut VecDeque<u8>) -> Result<(), String> {
    let header = [137,80,78,71,13,10,26,10];

    for byte in header.iter() {
        let b = data.pop_front();
        if b.is_none() {
            return Err("Not enough data".to_string());
        }
        let b = b.unwrap();
        if b != *byte as u8 {
            return Err("Not a PNG file".to_string());
        }
    }

    Ok(())
}

#[derive(Clone)]
enum ChunkType {
    IHDR,
    PLTE,
    IDAT,
    IEND
}

const fn to_u32(a: [u8;4]) -> u32 {
    let mut result: u32 = 0;
    result |= (a[0] as u32) << 8 * 3;
    result |= (a[1] as u32) << 8 * 2;
    result |= (a[2] as u32) << 8 * 1;
    result |= (a[3] as u32) << 8 * 0;

    result
}

fn get_chunk_type(data: &mut VecDeque<u8>) -> Result<(ChunkType, u32), String> {
    if data.len() < 8 {
        return Err("Not enough data to determine chunk header".to_string())
    }

    let length = parse_uint(data)?;
    let chunk_type = parse_uint(data)?;

    let headers: [(u32, ChunkType); 4] = [
        (to_u32([73, 72, 68, 82]), ChunkType::IHDR),
        (to_u32([80, 76, 84, 69]), ChunkType::PLTE),
        (to_u32([73, 68, 65, 84]), ChunkType::IDAT),
        (to_u32([73, 69, 78, 68]), ChunkType::IEND),
    ];

    for header in &headers {
        if chunk_type == header.0 {
            return Ok((header.1.clone(), length));
        }
    }

    println!("{} {} {} {}",
        chunk_type >> 24 & 0xff,
        chunk_type >> 16 & 0xff,
        chunk_type >> 8 & 0xff,
        chunk_type & 0xff,
            );
    return Err("Unknown chunk header".to_string())
}

fn parse_uint(data: &mut VecDeque<u8>) -> Result<u32, String> {
    let mut result: u32 = 0;
    if data.len() < 4 {
        return Err("Not enough data".to_string());
    }
    for i in 0..4 {
        let byte = data.pop_front().unwrap() as u32;
        result |= byte << 8 * (3 - i);
    }

    Ok(result)
}

fn parse_byte(data: &mut VecDeque<u8>) -> Result<u8, String> {
    if data.len() < 1 {
        return Err("Not enough data".to_string());
    }

    Ok(data.pop_front().unwrap())
}

fn parse_ihdr(data: &mut VecDeque<u8>, length: u32) -> Result<(), String> {

    println!("Length: {}", length);
    let w = parse_uint(data)?;
    let h = parse_uint(data)?;
    let depth = parse_byte(data)?;
    let colour_type = parse_byte(data)?;
    let compression = parse_byte(data)?;
    let filter = parse_byte(data)?;
    let interlace = parse_byte(data)?;

    println!("{}x{}:{}", w, h, depth);
    println!("colour_type: {}", colour_type);
    println!("compression: {}", compression);
    println!("filter: {}", filter);
    println!("interlace: {}", interlace);

    let crc = parse_uint(data)?;
    println!("crc: {}", crc);

    Ok(())
}

fn parse_idat(data: &mut VecDeque<u8>, length: u32) -> Result<(), String> {

    Err("not implemented".to_string())
}

fn parse_plte(data: &mut VecDeque<u8>, length: u32) -> Result<(), String> {

    Err("not implemented".to_string())
}

fn parse_iend(data: &mut VecDeque<u8>, length: u32) -> Result<(), String> {

    Err("not implemented".to_string())
}

fn parse_chunk(data: &mut VecDeque<u8>) -> Result<(), String> {
    let chunk_type = get_chunk_type(data);
    if chunk_type.is_err() {
        return Err("Failed to read a valid PNG chunk header".to_string());
    }

    let (chunk_type, length) = chunk_type.unwrap();
    match chunk_type {
        ChunkType::IHDR => {
            parse_ihdr(data, length)
        }
        ChunkType::IDAT => {
            parse_idat(data, length)
        }
        ChunkType::PLTE => {
            parse_plte(data, length)
        }
        ChunkType::IEND => {
            parse_iend(data, length)
        }
    }
}

fn main() {
    let f = std::fs::read("res/sparrow.png");

    let f = match f {
        Ok(f) => f,
        Err(e) => panic!(e)
    };

    let mut data: VecDeque<u8> = f.into_iter().collect();

    if let Err(e) = parse_png_header(&mut data) {
        println!("{}", e);
    }

    println!("PNG file!");

    while data.len() > 0 {
        match parse_chunk(&mut data) {
            Ok(_) => { println!("Parsed chunk"); }
            Err(e) => {
                println!("Error while parsing PNG: {}", e);
                panic!();
            }
        }
        println!("Remaining: {}", data.len());
    }
}
