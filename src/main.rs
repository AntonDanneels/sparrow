use std::collections::VecDeque;
use std::fs::File;
use std::io::prelude::*;
use std::time::Instant;
mod zlib;

#[derive(Clone)]
enum ChunkType {
    /* Required */
    IHDR,
    PLTE,
    IDAT,
    IEND,
    /* Optional */
    TEXT,
    PHYS,
    ZTXT,
    GAMA,
}

const fn to_u32(a: [u8; 4]) -> u32 {
    let mut result: u32 = 0;
    result |= (a[0] as u32) << 8 * 3;
    result |= (a[1] as u32) << 8 * 2;
    result |= (a[2] as u32) << 8 * 1;
    result |= (a[3] as u32) << 8 * 0;

    result
}

#[derive(Debug)]
struct Parser {
    width: u32,
    height: u32,
    depth: u8,
    colour_type: u8,
    compression: u8,
    filter: u8,
    interlace: u8,
    // File data: PNG chunks
    compressed_data: VecDeque<u8>,

    has_end: bool,
    // encoded zlib data
    encoded_data: VecDeque<u8>,
    // decoded zlib data
    decoded_data: Vec<u8>,
    // reconstructed image
    data: Vec<u8>,
}

impl Parser {
    fn new() -> Parser {
        Parser {
            width: 0,
            height: 0,
            depth: 0,
            colour_type: 0,
            compression: 0,
            filter: 0,
            interlace: 0,
            compressed_data: VecDeque::new(),
            has_end: false,
            encoded_data: VecDeque::new(),
            decoded_data: Vec::new(),
            data: Vec::new(),
        }
    }

    fn parse(&mut self, data: Vec<u8>) -> Result<(), String> {
        self.compressed_data = data.into_iter().collect();

        let now = Instant::now();
        self.parse_png_header()?;
        while !self.has_end {
            match self.parse_chunk() {
                Ok(_) => {}
                Err(e) => {
                    println!("Error while parsing PNG: {}", e);
                    panic!();
                }
            }
        }
        println!("PNG phase: {:?}", now.elapsed());

        println!("Image info: {}x{}@{}", self.width, self.height, self.depth);
        println!("colour_type: {}", self.colour_type);
        println!("compression: {}", self.compression);
        println!("filter: {}", self.filter);
        println!("interlace: {}", self.interlace);
        let now = Instant::now();
        self.decoded_data = zlib::parse(&mut self.encoded_data)?;
        println!("zlib phase: {:?}", now.elapsed());
        println!(
            "Decoded data: {:?}, len: {}",
            self.decoded_data,
            self.decoded_data.len()
        );

        //let mut tmp_data = Vec::with_capacity((self.width * self.height) as usize);
        const STARTING_ROW: [usize; 7] = [0, 0, 4, 0, 2, 0, 1];
        const STARTING_COL: [usize; 7] = [0, 4, 0, 2, 0, 1, 0];
        const ROW_INCREMENT: [usize; 7] = [8, 8, 8, 4, 4, 2, 2];
        const COL_INCREMENT: [usize; 7] = [8, 8, 4, 4, 2, 2, 1];
        const BLOCK_HEIGHT: [usize; 7] = [8, 8, 4, 4, 2, 2, 1];
        const BLOCK_WIDTH: [usize; 7] = [8, 4, 4, 2, 2, 1, 1];

        let min = |a, b| -> usize {
            if a < b {
                a
            } else {
                b
            }
        };

        let num_components: usize = match self.colour_type {
            0 => 1, //Grayscale
            2 => 3, //True color
            _ => panic!(),
        };
        match self.interlace {
            0 => false,
            1 => true,
            _ => return Err("Invalid interlace method".to_string()),
        };

        let visit = |image: &mut Vec<u8>, data: &Vec<u8>, x, y, w, h| {
            //println!("{}: {}, {}, {}, {}", data, x, y, w, h);
            for yy in 0..h {
                for xx in 0..w {
                    for i in 0..num_components {
                        image[(x + xx) * num_components
                            + i
                            + (y + yy) * (self.width * num_components as u32) as usize] = data[i];
                    }
                    //println!("Set: {} to {}", x + xx + (y + yy) * self.width as usize, 255);
                }
            }
        };

        let write_img = |name: String, w: usize, h: usize, data: &Vec<u8>| {
            let mut file = File::create(name).unwrap();
            file.write_all(b"P3 \n").unwrap();
            file.write_all(format!("{} {} \n", w, h).as_bytes())
                .unwrap();
            file.write_all(b"255 \n").unwrap();
            let mut i = 0;
            while i < data.len() {
                match num_components {
                    1 => {
                        file.write_all(
                            format!("{} {} {} \n", data[i], data[i], data[i],).as_bytes(),
                        )
                        .unwrap();
                        i += 1;
                    }
                    3 => {
                        file.write_all(
                            format!("{} {} {} \n", data[i], data[i + 1], data[i + 2],).as_bytes(),
                        )
                        .unwrap();
                        i += 3;
                    }
                    _ => panic!(),
                }
            }
        };
        if self.interlace == 1 {
            let mut offset = 0;
            let mut img = vec![122; (self.width * self.height * num_components as u32) as usize];
            for pass in 0..7 {
                println!(
                    "----------------------PASS: {}---------------------------",
                    pass
                );
                let mut row = STARTING_ROW[pass];
                let tmp_w = self.width as usize / COL_INCREMENT[pass];
                let tmp_h = self.height as usize / ROW_INCREMENT[pass];
                let data = self.reverse_filter(
                    self.width as usize / COL_INCREMENT[pass],
                    self.height as usize / ROW_INCREMENT[pass],
                    num_components,
                    self.depth as usize,
                    offset,
                )?;
                offset += tmp_h
                    + ((tmp_w as f32 / 8.0 * self.depth as f32).ceil() as usize)
                        * tmp_h
                        * num_components;
                let mut index = 0;
                while row < self.height as usize {
                    let mut col = STARTING_COL[pass];
                    while col < self.width as usize {
                        let mut d = vec![];
                        for i in 0..num_components {
                            d.push(data[index + i]);
                        }
                        visit(
                            &mut img,
                            &d,
                            col,
                            row,
                            min(BLOCK_WIDTH[pass], self.width as usize - col),
                            min(BLOCK_HEIGHT[pass], self.height as usize - row),
                        );
                        col += COL_INCREMENT[pass];
                        index += num_components;
                    }
                    row += ROW_INCREMENT[pass];
                }
                write_img(
                    format!("pass_{}.ppm", pass),
                    self.width as usize,
                    self.height as usize,
                    &img,
                );
            }
            return Ok(());
        }

        for i in 1..5 {
            println!("{:#0b}", self.decoded_data[i]);
        }

        //TODO: handle components properly
        let now = Instant::now();
        self.data = self.reverse_filter(
            self.width as usize,
            self.height as usize,
            num_components,
            self.depth as usize,
            0,
        )?;
        println!("reverse filter phase: {:?}", now.elapsed());

        let mut file = File::create("img.ppm").unwrap();
        file.write_all(b"P3 \n").unwrap();
        file.write_all(format!("{} {} \n", self.width, self.height).as_bytes())
            .unwrap();
        file.write_all(b"255 \n").unwrap();
        let mut i = 0;
        while i < self.data.len() {
            file.write_all(
                format!(
                    "{} {} {}\n",
                    self.data[i],
                    self.data[i + 1],
                    self.data[i + 2]
                )
                .as_bytes(),
            )
            .unwrap();
            i += 3;
        }

        Ok(())
    }

    fn reverse_filter(
        &self,
        width: usize,
        height: usize,
        channels: usize,
        depth: usize,
        offset: usize,
    ) -> Result<Vec<u8>, String> {
        let mut result = Vec::with_capacity((self.width * self.height * channels as u32) as usize);

        let mut data = Vec::with_capacity(width * height * channels / 8 * depth);
        println!(
            "Reverse filter: {}x{}@{} ({})",
            width, height, depth, channels
        );
        match depth {
            1 | 2 | 4 => {
                println!(
                    "Processing {} bytes",
                    (height /*filters*/ + ((width as f32 / 8.0 * depth as f32).ceil() as usize)* height * channels)
                );
                for y in 0..height {
                    let row_index = y * ((width as f32 / 8.0 * depth as f32).ceil() as usize) + y;
                    let filter = self.decoded_data[row_index + offset];
                    println!(
                        "filter: {}, width: {}",
                        filter,
                        ((width as f32 / 8.0 * depth as f32).ceil() as usize)
                    );
                    for x in 0..((width as f32 / 8.0 * depth as f32).ceil() as usize) {
                        //println!("{} {:#0b}",self.decoded_data[row_index + x + 1 + offset],
                        //                     self.decoded_data[row_index + x + 1 + offset]);
                        let x = self.decoded_data[row_index + x + 1 + offset];
                        match depth {
                            1 => {
                                for i in 0..std::cmp::min(width, 8) {
                                    data.push((x >> (7 - i) & 0b1) * 255);
                                }
                            }
                            2 => {
                                for i in 0..(std::cmp::min(width, 8 / 2)) {
                                    data.push((x >> (6 - (i * 2)) & 0b11) * 85);
                                }
                            }
                            4 => {
                                for i in 0..(std::cmp::min(width, 8 / 4)) {
                                    data.push((x >> (4 - (i * 4)) & 0b1111) * 17);
                                }
                            }
                            _ => panic!(),
                        }
                    }
                }
                //for i in 0..(height /*filters*/ + width * height * channels / 8) {
                //    data.push(self.decoded_data[i]);
                //}
                //panic!();
                return Ok(data);
            }
            8 => {}
            _ => return Err("Not implemented".to_string()),
        }

        let paeth_predictor = |a, b, c| -> u32 {
            let a = a as i32;
            let b = b as i32;
            let c = c as i32;
            let p = a + b - c;
            let pa = (p - a).abs();
            let pb = (p - b).abs();
            let pc = (p - c).abs();
            if pa <= pb && pa <= pc {
                a as u32
            } else if pb <= pc {
                b as u32
            } else {
                c as u32
            }
        };

        let mut p = Vec::with_capacity(channels as usize);
        for y in 0..(height) {
            let row_index = y * width * channels + y;
            let filter = self.decoded_data[(row_index + offset) as usize];
            for x in 1..(width + 1) {
                let a = |offset| -> u32 {
                    match x {
                        1 => 0,
                        _ => {
                            result[(result.len() as u32 - channels as u32 + offset as u32) as usize]
                                as u32
                        }
                    }
                };
                let b = |offset| -> u32 {
                    match y {
                        0 => 0,
                        _ => result[(result.len() - width * channels + offset) as usize] as u32,
                    }
                };
                let c = |offset| -> u32 {
                    match (x, y) {
                        (1, 0) => 0,
                        (1, _) => 0,
                        (_, 0) => 0,
                        (_, _) => {
                            result[(result.len() as u32
                                - width as u32 * channels as u32
                                - channels as u32
                                + offset as u32) as usize] as u32
                        }
                    }
                };

                let xx = row_index + (x - 1) * channels + 1;
                match filter {
                    0 => {
                        for i in 0..channels {
                            result.push(self.decoded_data[(xx + i + offset) as usize]);
                        }
                    }
                    1 => {
                        for i in 0..channels {
                            p.push(
                                ((self.decoded_data[(xx + i + offset) as usize] as u32 + a(i))
                                    % 256) as u8,
                            );
                        }
                        result.append(&mut p);
                    }
                    2 => {
                        for i in 0..channels {
                            p.push(
                                ((self.decoded_data[(xx + i + offset) as usize] as u32 + b(i))
                                    % 256) as u8,
                            );
                        }
                        result.append(&mut p);
                    }
                    3 => {
                        for i in 0..channels {
                            p.push(
                                ((self.decoded_data[(xx + i + offset) as usize] as u32
                                    + (a(i) + b(i)) / 2)
                                    % 256) as u8,
                            );
                        }
                        result.append(&mut p);
                    }
                    4 => {
                        for i in 0..channels {
                            p.push(
                                ((self.decoded_data[(xx + i + offset) as usize] as u32
                                    + paeth_predictor(a(i), b(i), c(i)))
                                    % 256) as u8,
                            );
                        }
                        result.append(&mut p);
                    }
                    _ => {
                        return Err(format!("Corrupted data: {}", filter));
                    }
                }
            }
        }

        Ok(result)
    }

    fn parse_png_header(&mut self) -> Result<(), String> {
        let header = [137, 80, 78, 71, 13, 10, 26, 10];

        for byte in header.iter() {
            let b = self.parse_byte()?;
            if b != *byte as u8 {
                return Err("Not a PNG file".to_string());
            }
        }

        Ok(())
    }

    fn get_chunk_type(&mut self) -> Result<(ChunkType, u32), String> {
        if self.compressed_data.len() < 8 {
            return Err("Not enough data to determine chunk header".to_string());
        }

        let length = self.parse_uint()?;
        let chunk_type = self.parse_uint()?;

        let headers: [(u32, ChunkType); 8] = [
            (to_u32([73, 72, 68, 82]), ChunkType::IHDR),
            (to_u32([80, 76, 84, 69]), ChunkType::PLTE),
            (to_u32([73, 68, 65, 84]), ChunkType::IDAT),
            (to_u32([73, 69, 78, 68]), ChunkType::IEND),
            (to_u32([116, 69, 88, 116]), ChunkType::TEXT),
            (to_u32([112, 72, 89, 115]), ChunkType::PHYS),
            (to_u32([122, 84, 88, 116]), ChunkType::ZTXT),
            (to_u32([103, 65, 77, 65]), ChunkType::GAMA),
        ];

        for header in &headers {
            if chunk_type == header.0 {
                return Ok((header.1.clone(), length));
            }
        }

        println!(
            "{} {} {} {}",
            chunk_type >> 24 & 0xff,
            chunk_type >> 16 & 0xff,
            chunk_type >> 8 & 0xff,
            chunk_type & 0xff,
        );
        return Err("Unknown chunk header".to_string());
    }

    fn parse_uint(&mut self) -> Result<u32, String> {
        let mut result: u32 = 0;
        if self.compressed_data.len() < 4 {
            return Err("Not enough data".to_string());
        }
        for i in 0..4 {
            let byte = self.compressed_data.pop_front().unwrap() as u32;
            result |= byte << 8 * (3 - i);
        }

        Ok(result)
    }

    fn parse_byte(&mut self) -> Result<u8, String> {
        if self.compressed_data.len() < 1 {
            return Err("Not enough data".to_string());
        }

        Ok(self.compressed_data.pop_front().unwrap())
    }

    fn parse_ihdr(&mut self, _: u32) -> Result<(), String> {
        self.width = self.parse_uint()?;
        self.height = self.parse_uint()?;
        self.depth = self.parse_byte()?;
        self.colour_type = self.parse_byte()?;
        self.compression = self.parse_byte()?;
        self.filter = self.parse_byte()?;
        self.interlace = self.parse_byte()?;

        let _crc = self.parse_uint()?;
        Ok(())
    }

    fn parse_idat(&mut self, length: u32) -> Result<(), String> {
        if self.compressed_data.len() < length as usize {
            return Err("Not enough data".to_string());
        }
        let new_data = self.compressed_data.split_off(length as usize);
        self.encoded_data.append(&mut self.compressed_data);
        self.compressed_data = new_data;

        let _crc = self.parse_uint()?;
        Ok(())
    }

    fn parse_plte(&mut self, length: u32) -> Result<(), String> {
        println!("PLTE");
        Err("not implemented".to_string())
    }

    fn parse_ztxt(&mut self, length: u32) -> Result<(), String> {
        let mut size = 0;
        let mut keyword = Vec::new();
        loop {
            let c = self.parse_byte()? as char;
            size += 1;
            if c == '\0' {
                break;
            }
            keyword.push(c);
            if size > 79 {
                return Err("Corrupted PNG zTXt header".to_string());
            }
        }
        let _method = self.parse_byte()?;
        for _ in 0..(length - size - 1) {
            self.parse_byte()?;
        }

        let _crc = self.parse_uint()?;
        Ok(())
    }

    fn parse_phys(&mut self, length: u32) -> Result<(), String> {
        let ppu_x = self.parse_uint()?;
        let ppu_y = self.parse_uint()?;
        let unit = self.parse_byte()?;

        //println!("PPU: {}x{} ({})", ppu_x, ppu_y, unit);

        let _crc = self.parse_uint()?;
        Ok(())
    }

    fn parse_iend(&mut self, length: u32) -> Result<(), String> {
        self.has_end = true;
        let _crc = self.parse_uint()?;
        Ok(())
    }

    fn parse_gama(&mut self, length: u32) -> Result<(), String> {
        let gamma = self.parse_uint()?;
        let _crc = self.parse_uint()?;
        Ok(())
    }

    fn parse_text(&mut self, length: u32) -> Result<(), String> {
        let mut size = 0;
        let mut keyword = Vec::new();
        loop {
            let c = self.parse_byte()? as char;
            size += 1;
            if c == '\0' {
                break;
            }
            keyword.push(c);
        }
        let bytes_left = length as i32 - size;
        if bytes_left < 0 {
            return Err("Expected a length > 0 for text string".to_string());
        }
        let mut text_str = Vec::new();
        for _ in 0..bytes_left {
            let c = self.parse_byte()? as char;
            text_str.push(c);
        }

        let keyword: String = keyword.into_iter().collect();
        let text_str: String = text_str.into_iter().collect();
        //println!("{}: {}", keyword, text_str);
        let crc = self.parse_uint()?;
        //println!("crc: {}", crc);

        Ok(())
    }

    fn parse_chunk(&mut self) -> Result<(), String> {
        let chunk_type = self.get_chunk_type();
        if chunk_type.is_err() {
            return Err("Failed to read a valid PNG chunk header".to_string());
        }

        let (chunk_type, length) = chunk_type.unwrap();
        match chunk_type {
            ChunkType::IHDR => self.parse_ihdr(length),
            ChunkType::IDAT => self.parse_idat(length),
            ChunkType::PLTE => self.parse_plte(length),
            ChunkType::IEND => self.parse_iend(length),
            ChunkType::TEXT => self.parse_text(length),
            ChunkType::PHYS => self.parse_phys(length),
            ChunkType::ZTXT => self.parse_ztxt(length),
            ChunkType::GAMA => self.parse_gama(length),
        }
    }
}

fn main() {
    let f = std::fs::read("res/sparrow_bg.png");

    let f = match f {
        Ok(f) => f,
        Err(e) => panic!(e),
    };

    let data: Vec<u8> = f.into_iter().collect();
    let mut parser = Parser::new();

    match parser.parse(data) {
        Ok(_) => {
            println!("File parsed!");
        }
        Err(e) => {
            println!("Failed to parse PNG: {}", e);
        }
    }
}
