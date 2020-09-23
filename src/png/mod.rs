use std::collections::VecDeque;
use std::fs::File;
use std::io::prelude::*;
use std::time::Instant;

mod zlib;

#[derive(Debug, Clone)]
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
    SBIT,
    BKGD,
    CHRM,
    HIST,
    TIME,
    ITXT,
    TRNS,

    /* Not defined by the spec */
    UNKNOWN,
}

const fn to_u32(a: [u8; 4]) -> u32 {
    let mut result: u32 = 0;
    result |= (a[0] as u32) << 8 * 3;
    result |= (a[1] as u32) << 8 * 2;
    result |= (a[2] as u32) << 8 * 1;
    result |= (a[3] as u32) << 8 * 0;

    result
}

const PNG_CRC_TABLE: [u64; 256] = make_crc_table();

const fn make_crc_table() -> [u64; 256] {
    let mut result: [u64; 256] = [0; 256];

    let mut i = 0;
    while i < 256 {
        let mut c = i as u64;
        let mut j = 0;
        while j < 8 {
            if (c & 1) != 0 {
                c = 0xedb88320 ^ (c >> 1);
            } else {
                c = c >> 1;
            }
            j += 1;
        }
        result[i] = c;
        i += 1;
    }

    result
}

fn calc_crc(data: &VecDeque<u8>, length: u32) -> u64 {
    let mut result = 0xffffffff as u64;

    for i in 0..length as usize {
        result = PNG_CRC_TABLE[((result ^ data[i] as u64) & 0xff) as usize] ^ (result >> 8);
    }

    result ^ 0xffffffff
}

#[derive(Debug, PartialEq)]
enum ColourType {
    Grayscale,
    TrueColour,
    Indexed,
    GrayscaleAlpha,
    TrueColourAlpha,
    Invalid,
}

#[derive(Debug)]
pub struct Parser {
    width: u32,
    height: u32,
    depth: u8,
    colour_type: ColourType,
    compression: u8,
    filter: u8,
    interlace: u8,
    plte: Vec<(u8, u8, u8, u8)>,
    transparency: (u16, u16, u16),
    has_transparency: bool,
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

fn visit(
    image: &mut Vec<u8>,
    data: &Vec<u8>,
    width: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    num_components: usize,
) {
    for yy in 0..h {
        for xx in 0..w {
            for i in 0..num_components {
                image[(x + xx) * num_components
                    + i
                    + (y + yy) * (width * num_components) as usize] = data[i];
            }
        }
    }
}

impl Parser {
    pub fn new() -> Parser {
        Parser {
            width: 0,
            height: 0,
            depth: 0,
            colour_type: ColourType::Invalid,
            compression: 0,
            filter: 0,
            interlace: 0,
            plte: Vec::new(),
            transparency: (255, 255, 255),
            has_transparency: false,
            compressed_data: VecDeque::new(),
            has_end: false,
            encoded_data: VecDeque::new(),
            decoded_data: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn parse(&mut self, data: Vec<u8>) -> Result<(), String> {
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
        println!("colour_type: {:?}", self.colour_type);
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
            ColourType::Grayscale => 1,
            ColourType::TrueColour => 3,
            ColourType::Indexed => {
                if self.plte.len() == 0 {
                    return Err("Missing PLTE chunk".to_string());
                }

                4
            }
            ColourType::GrayscaleAlpha => 2,
            ColourType::TrueColourAlpha => 4,
            _ => panic!(),
        };

        let now = Instant::now();
        if self.interlace == 1 {
            let mut offset = 0;
            self.data = vec![0; (self.width * self.height * num_components as u32) as usize];
            for pass in 0..7 as usize {
                let mut row = STARTING_ROW[pass];
                let w = ((self.width as i32 - STARTING_COL[pass] as i32
                    + COL_INCREMENT[pass] as i32
                    - 1)
                    / COL_INCREMENT[pass] as i32) as usize;
                let h = ((self.height as i32 - STARTING_ROW[pass] as i32
                    + ROW_INCREMENT[pass] as i32
                    - 1)
                    / ROW_INCREMENT[pass] as i32) as usize;

                if w == 0 || h == 0 {
                    continue;
                }
                let bytes_needed = match self.colour_type {
                    ColourType::Indexed => 1,
                    _ => num_components,
                };
                let data = self.reverse_filter(w, h, bytes_needed, self.depth as usize, offset)?;
                offset += h /*filters*/ +
                          ((w as f32 / 8.0 * self.depth as f32).ceil() as usize) * h * bytes_needed;

                let mut index = 0;
                while row < self.height as usize {
                    let mut col = STARTING_COL[pass];
                    while col < self.width as usize {
                        let mut d = vec![];
                        for i in 0..num_components {
                            d.push(data[index + i]);
                        }
                        visit(
                            &mut self.data,
                            &d,
                            self.width as usize,
                            col,
                            row,
                            min(BLOCK_WIDTH[pass], self.width as usize - col),
                            min(BLOCK_HEIGHT[pass], self.height as usize - row),
                            num_components,
                        );
                        col += COL_INCREMENT[pass];
                        index += num_components;
                    }
                    row += ROW_INCREMENT[pass];
                }
            }
        } else {
            let bytes_needed = match self.colour_type {
                ColourType::Indexed => 1,
                _ => num_components,
            };
            self.data = self.reverse_filter(
                self.width as usize,
                self.height as usize,
                bytes_needed,
                self.depth as usize,
                0,
            )?;
        }

        println!("reverse filter phase: {:?}", now.elapsed());

        if self.has_transparency {
            match self.colour_type {
                ColourType::Grayscale => {
                    let mut new_data = Vec::with_capacity(self.data.len() * 2);
                    for i in 0..(self.data.len()) {
                        new_data.push(self.data[i]);
                        if self.data[i] == self.transparency.0 as u8 {
                            new_data.push(0);
                        } else {
                            new_data.push(255);
                        }
                    }
                    self.data = new_data;
                    self.colour_type = ColourType::GrayscaleAlpha;
                }
                ColourType::TrueColour => {
                    let mut new_data = Vec::with_capacity(self.data.len() / 3 * 4);
                    for i in 0..(self.data.len() / 3) {
                        new_data.push(self.data[i * 3 + 0]);
                        new_data.push(self.data[i * 3 + 1]);
                        new_data.push(self.data[i * 3 + 2]);
                        if self.data[i * 3] == self.transparency.0 as u8
                            && self.data[i * 3 + 1] == self.transparency.1 as u8
                            && self.data[i * 3 + 2] == self.transparency.2 as u8
                        {
                            new_data.push(0);
                        } else {
                            new_data.push(255);
                        }
                    }
                    self.data = new_data;
                    self.colour_type = ColourType::TrueColourAlpha;
                }
                _ => {
                    return Err(format!(
                        "Cannot have transparency for {:?}",
                        self.colour_type
                    ))
                }
            }
        }

        write_img(
            format!("img.ppm"),
            self.width as usize,
            self.height as usize,
            &self.colour_type,
            &self.data,
        );

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

        println!(
            "Processing {} bytes",
            (height /*filters*/ + ((width as f32 / 8.0 * depth as f32).ceil() as usize) * height * channels)
        );

        let mut p = Vec::with_capacity(channels as usize);
        for y in 0..(height) {
            let bytes_to_process = (width as f32 / 8.0 * depth as f32).ceil() as usize;
            let row_index = y * bytes_to_process * channels + y;
            let filter = self.decoded_data[row_index + offset];
            println!("filter: {}, width: {}", filter, bytes_to_process);
            for x in 0..bytes_to_process {
                let xx = row_index + x * channels + 1;
                let a = |offset| -> u32 {
                    match x {
                        0 => 0,
                        _ => {
                            result[(result.len() as u32 - channels as u32 + offset as u32) as usize]
                                as u32
                        }
                    }
                };
                let b = |offset| -> u32 {
                    match y {
                        0 => 0,
                        _ => {
                            result[(result.len() - bytes_to_process * channels + offset) as usize]
                                as u32
                        }
                    }
                };
                let c = |offset| -> u32 {
                    match (x, y) {
                        (0, 0) => 0,
                        (0, _) => 0,
                        (_, 0) => 0,
                        (_, _) => {
                            result[(result.len() as u32
                                - (bytes_to_process * channels) as u32
                                - channels as u32
                                + offset as u32) as usize] as u32
                        }
                    }
                };

                match filter {
                    0 => {
                        for i in 0..channels {
                            let index = (xx + i + offset) as usize;
                            result.push(self.decoded_data[index]);
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

        const DEPTH_SCALE: [u8; 5] = [0, 255, 85, 0, 17];
        match depth {
            1 | 2 | 4 => {
                let mut current_byte = 0;
                for _ in 0..height {
                    let mut num_bits = 0;
                    for _ in 0..width {
                        let x =
                            result[current_byte] >> ((8 - depth) - num_bits) & ((1 << depth) - 1);
                        if self.colour_type == ColourType::Indexed {
                            data.push(self.plte[x as usize].0);
                            data.push(self.plte[x as usize].1);
                            data.push(self.plte[x as usize].2);
                            data.push(self.plte[x as usize].3);
                        } else {
                            data.push(x as u8 * DEPTH_SCALE[depth]);
                        }
                        if num_bits + depth >= 8 {
                            current_byte += 1;
                        }
                        num_bits = (num_bits + depth) % 8;
                    }
                    if num_bits > 0 {
                        current_byte += 1;
                    }
                }
                return Ok(data);
            }
            8 => {
                if self.colour_type == ColourType::Indexed {
                    for i in 0..result.len() {
                        data.push(self.plte[result[i] as usize].0);
                        data.push(self.plte[result[i] as usize].1);
                        data.push(self.plte[result[i] as usize].2);
                        data.push(self.plte[result[i] as usize].3);
                    }
                    return Ok(data);
                }
            }
            _ => return Err("Not implemented".to_string()),
        }

        Ok(result)
    }

    fn parse_png_header(&mut self) -> Result<(), String> {
        let header = [137, 80, 78, 71, 13, 10, 26, 10];

        for byte in header.iter() {
            let b = self.parse_u8()?;
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

        let length = self.parse_u32()?;

        let headers: [(u32, ChunkType); 15] = [
            (to_u32([73, 72, 68, 82]), ChunkType::IHDR),
            (to_u32([80, 76, 84, 69]), ChunkType::PLTE),
            (to_u32([73, 68, 65, 84]), ChunkType::IDAT),
            (to_u32([73, 69, 78, 68]), ChunkType::IEND),
            (to_u32([116, 69, 88, 116]), ChunkType::TEXT),
            (to_u32([112, 72, 89, 115]), ChunkType::PHYS),
            (to_u32([122, 84, 88, 116]), ChunkType::ZTXT),
            (to_u32([103, 65, 77, 65]), ChunkType::GAMA),
            (to_u32([115, 66, 73, 84]), ChunkType::SBIT),
            (to_u32([98, 75, 71, 68]), ChunkType::BKGD),
            (to_u32([99, 72, 82, 77]), ChunkType::CHRM),
            (to_u32([104, 73, 83, 84]), ChunkType::HIST),
            (to_u32([116, 73, 77, 69]), ChunkType::TIME),
            (to_u32([105, 84, 88, 116]), ChunkType::ITXT),
            (to_u32([116, 82, 78, 83]), ChunkType::TRNS),
        ];

        if self.compressed_data.len() < (length as usize + 4) {
            return Err("Not enough data".to_string());
        }
        let crc1 = calc_crc(&self.compressed_data, length + 4);
        let crc2 = self.peek_u32(length + 4)?;

        if crc1 != crc2 as u64 {
            return Err("Corrupted data".to_string());
        }

        let chunk_type = self.parse_u32()?;
        for header in &headers {
            if chunk_type == header.0 {
                return Ok((header.1.clone(), length));
            }
        }

        println!(
            "Unknown chunk header, ignoring: {} {} {} {}",
            chunk_type >> 24 & 0xff,
            chunk_type >> 16 & 0xff,
            chunk_type >> 8 & 0xff,
            chunk_type & 0xff,
        );
        Ok((ChunkType::UNKNOWN, length))
    }

    fn parse_u32(&mut self) -> Result<u32, String> {
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

    fn peek_u32(&self, offset: u32) -> Result<u32, String> {
        let mut result: u32 = 0;
        if self.compressed_data.len() < (offset as usize + 4) {
            return Err("Not enough data".to_string());
        }
        for i in 0..4 {
            let byte = self.compressed_data[offset as usize + i] as u32;
            result |= byte << 8 * (3 - i);
        }

        Ok(result)
    }

    fn parse_u16(&mut self) -> Result<u32, String> {
        let mut result: u32 = 0;
        if self.compressed_data.len() < 2 {
            return Err("Not enough data".to_string());
        }
        for i in 0..2 {
            let byte = self.compressed_data.pop_front().unwrap() as u32;
            result |= byte << 8 * (1 - i);
        }

        Ok(result)
    }

    fn parse_u8(&mut self) -> Result<u8, String> {
        if self.compressed_data.len() < 1 {
            return Err("Not enough data".to_string());
        }

        Ok(self.compressed_data.pop_front().unwrap())
    }

    fn parse_ihdr(&mut self, _: u32) -> Result<(), String> {
        self.width = self.parse_u32()?;
        self.height = self.parse_u32()?;
        self.depth = self.parse_u8()?;
        self.colour_type = match self.parse_u8()? {
            0 => ColourType::Grayscale,
            2 => ColourType::TrueColour,
            3 => ColourType::Indexed,
            4 => ColourType::GrayscaleAlpha,
            6 => ColourType::TrueColourAlpha,
            _ => ColourType::Invalid,
        };
        self.compression = self.parse_u8()?;
        self.filter = self.parse_u8()?;
        self.interlace = self.parse_u8()?;
        match self.interlace {
            0 | 1 => {}
            _ => return Err("Invalid interlace method".to_string()),
        };

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_idat(&mut self, length: u32) -> Result<(), String> {
        if self.compressed_data.len() < length as usize {
            return Err("Not enough data".to_string());
        }
        let new_data = self.compressed_data.split_off(length as usize);
        self.encoded_data.append(&mut self.compressed_data);
        self.compressed_data = new_data;

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_plte(&mut self, length: u32) -> Result<(), String> {
        if length % 3 != 0 {
            return Err("Corrupted data".to_string());
        }

        self.plte = Vec::with_capacity((length / 3) as usize);
        for _ in 0..(length / 3) {
            let r = self.parse_u8()?;
            let g = self.parse_u8()?;
            let b = self.parse_u8()?;
            self.plte.push((r, g, b, 255));
        }
        println!("{:?}", self.plte);

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_ztxt(&mut self, length: u32) -> Result<(), String> {
        let mut size = 0;
        let mut keyword = Vec::new();
        loop {
            let c = self.parse_u8()? as char;
            size += 1;
            if c == '\0' {
                break;
            }
            keyword.push(c);
            if size > 79 {
                return Err("Corrupted PNG zTXt header".to_string());
            }
        }
        let _method = self.parse_u8()?;
        for _ in 0..(length - size - 1) {
            self.parse_u8()?;
        }

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_phys(&mut self, _length: u32) -> Result<(), String> {
        let ppu_x = self.parse_u32()?;
        let ppu_y = self.parse_u32()?;
        let unit = self.parse_u8()?;

        println!("PPU: {}x{} ({})", ppu_x, ppu_y, unit);

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_iend(&mut self, _length: u32) -> Result<(), String> {
        self.has_end = true;
        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_gama(&mut self, _length: u32) -> Result<(), String> {
        let gamma = self.parse_u32()?;
        println!("Gamma: {} {}", gamma, gamma as f32 / 100000.0);
        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_sbit(&mut self, _length: u32) -> Result<(), String> {
        match self.colour_type {
            ColourType::Grayscale => {
                println!("g: {}", self.parse_u8()?);
            }
            ColourType::TrueColour | ColourType::Indexed => {
                println!("r: {}", self.parse_u8()?);
                println!("g: {}", self.parse_u8()?);
                println!("b: {}", self.parse_u8()?);
            }
            ColourType::GrayscaleAlpha => {
                println!("g: {}", self.parse_u8()?);
                println!("a: {}", self.parse_u8()?);
            }
            ColourType::TrueColourAlpha => {
                println!("r: {}", self.parse_u8()?);
                println!("g: {}", self.parse_u8()?);
                println!("b: {}", self.parse_u8()?);
                println!("a: {}", self.parse_u8()?);
            }
            ColourType::Invalid => return Err("Got sBIT before colour type".to_string()),
        }

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_bkgd(&mut self, _length: u32) -> Result<(), String> {
        match self.colour_type {
            ColourType::Grayscale | ColourType::GrayscaleAlpha => {
                println!("bg: {} {}", self.parse_u8()?, self.parse_u8()?);
            }
            ColourType::TrueColour | ColourType::TrueColourAlpha => {
                println!("bg r: {} {}", self.parse_u8()?, self.parse_u8()?);
                println!("bg g: {} {}", self.parse_u8()?, self.parse_u8()?);
                println!("bg b: {} {}", self.parse_u8()?, self.parse_u8()?);
            }
            ColourType::Indexed => {
                println!("bg: {}", self.parse_u8()?);
            }
            ColourType::Invalid => return Err("Got bKGD before colour type".to_string()),
        }

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_chrm(&mut self, _length: u32) -> Result<(), String> {
        let _wpx = self.parse_u32()?;
        let _wpy = self.parse_u32()?;
        let _redx = self.parse_u32()?;
        let _redy = self.parse_u32()?;
        let _greenx = self.parse_u32()?;
        let _greeny = self.parse_u32()?;
        let _bluex = self.parse_u32()?;
        let _bluey = self.parse_u32()?;

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_hist(&mut self, length: u32) -> Result<(), String> {
        let mut hist = Vec::with_capacity((length / 2) as usize);
        for _ in 0..(length / 2) {
            hist.push(self.parse_u16()?);
        }

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_time(&mut self, _length: u32) -> Result<(), String> {
        let _year1 = self.parse_u8()?;
        let _year2 = self.parse_u8()?;
        let _month = self.parse_u8()?;
        let _day = self.parse_u8()?;
        let _hour = self.parse_u8()?;
        let _min = self.parse_u8()?;
        let _sec = self.parse_u8()?;

        println!(
            "Last modification time: {}{}/{}/{} {}:{}:{}",
            _year1, _year2, _month, _day, _hour, _min, _sec
        );

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_trns(&mut self, length: u32) -> Result<(), String> {
        match self.colour_type {
            ColourType::Grayscale => {
                let rgb = (self.parse_u8()? as u16) << 8 | self.parse_u8()? as u16;
                println!("rgb: {}", rgb);
                self.transparency = (rgb, rgb, rgb);
                self.has_transparency = true;
            }
            ColourType::TrueColour => {
                let r = (self.parse_u8()? as u16) << 8 | self.parse_u8()? as u16;
                let g = (self.parse_u8()? as u16) << 8 | self.parse_u8()? as u16;
                let b = (self.parse_u8()? as u16) << 8 | self.parse_u8()? as u16;
                println!("rgb: {}, {}, {}", r, g, b);
                self.transparency = (r, g, b);
                self.has_transparency = true;
            }
            ColourType::Indexed => {
                if self.plte.len() == 0 {
                    return Err("Expected PLTE before TRNS chunk".to_string());
                }
                for i in 0..length as usize {
                    let a = self.parse_u8()?;
                    self.plte[i].3 = a;
                }
                println!("size: {}", self.plte.len());
            }
            ColourType::Invalid => {
                return Err("Expected IHDR before TRNS chunk".to_string());
            }
            _ => {
                return Err(format!("Not valid for ColourType: {:?}", self.colour_type));
            }
        }

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_str(&mut self) -> Result<(String, usize), String> {
        let mut size = 0;
        let mut result = Vec::new();
        loop {
            let c = self.parse_u8()? as char;
            size += 1;
            if c == '\0' {
                break;
            }
            result.push(c);
        }

        Ok((result.into_iter().collect(), size))
    }

    fn parse_itxt(&mut self, length: u32) -> Result<(), String> {
        let mut total_size = 0;
        let (_keyword, size) = self.parse_str()?;
        total_size += size;
        let _compr_flag = self.parse_u8()?;
        let _compr_method = self.parse_u8()?;
        let (_lang, size) = self.parse_str()?;
        total_size += size;
        let (_translated_keyword, size) = self.parse_str()?;
        total_size += size;
        total_size += 2; // compression bytes

        let bytes_left = length as i32 - total_size as i32;
        if bytes_left < 0 {
            return Err("Expected a length > 0 for text string".to_string());
        }
        let mut text_str = Vec::with_capacity(bytes_left as usize);
        for _ in 0..bytes_left {
            let c = self.parse_u8()? as char;
            text_str.push(c);
        }

        let _text_str: String = text_str.into_iter().collect();

        //println!("{}, {}, {}", _keyword, _lang, _text_str);

        let _crc = self.parse_u32()?;
        Ok(())
    }

    fn parse_text(&mut self, length: u32) -> Result<(), String> {
        let (_keyword, size) = self.parse_str()?;
        let bytes_left = length as usize - size;
        if bytes_left == 0 {
            return Err("Expected a length > 0 for text string".to_string());
        }
        let mut text_str = Vec::new();
        for _ in 0..bytes_left {
            let c = self.parse_u8()? as char;
            text_str.push(c);
        }

        let _text_str: String = text_str.into_iter().collect();
        //println!("{}: {}", keyword, text_str);
        let _crc = self.parse_u32()?;
        //println!("crc: {}", crc);

        Ok(())
    }

    fn parse_chunk(&mut self) -> Result<(), String> {
        let chunk_type = self.get_chunk_type();
        if chunk_type.is_err() {
            return Err("Failed to read a valid PNG chunk header".to_string());
        }

        let (chunk_type, length) = chunk_type.unwrap();
        println!("{:?}", chunk_type);

        match chunk_type {
            ChunkType::IHDR => self.parse_ihdr(length),
            ChunkType::IDAT => self.parse_idat(length),
            ChunkType::PLTE => self.parse_plte(length),
            ChunkType::IEND => self.parse_iend(length),
            ChunkType::TEXT => self.parse_text(length),
            ChunkType::PHYS => self.parse_phys(length),
            ChunkType::ZTXT => self.parse_ztxt(length),
            ChunkType::GAMA => self.parse_gama(length),
            ChunkType::SBIT => self.parse_sbit(length),
            ChunkType::BKGD => self.parse_bkgd(length),
            ChunkType::CHRM => self.parse_chrm(length),
            ChunkType::HIST => self.parse_hist(length),
            ChunkType::TIME => self.parse_time(length),
            ChunkType::ITXT => self.parse_itxt(length),
            ChunkType::TRNS => self.parse_trns(length),

            ChunkType::UNKNOWN => {
                if self.compressed_data.len() < length as usize {
                    return Err("Not enough data".to_string());
                }
                let new_data = self.compressed_data.split_off(length as usize);
                self.compressed_data = new_data;

                let _crc = self.parse_u32()?;
                Ok(())
            }
        }
    }
}

fn write_img(name: String, w: usize, h: usize, colour: &ColourType, data: &Vec<u8>) {
    let debug = false;
    let mut file = File::create(name).unwrap();
    file.write_all(b"P3 \n").unwrap();
    file.write_all(format!("{} {} \n", w, h).as_bytes())
        .unwrap();
    file.write_all(b"255 \n").unwrap();
    let mut i = 0;
    while i < data.len() {
        match colour {
            ColourType::Grayscale => {
                if debug {
                    file.write_all(format!("{} {} {}\n", data[i], data[i], data[i]).as_bytes())
                        .unwrap();
                } else {
                    file.write_all(format!("{} {} {} 255\n", data[i], data[i], data[i]).as_bytes())
                        .unwrap();
                }
                i += 1;
            }
            ColourType::GrayscaleAlpha => {
                if debug {
                    file.write_all(format!("{} {} {}\n", data[i], data[i], data[i]).as_bytes())
                        .unwrap();
                } else {
                    file.write_all(
                        format!("{} {} {} {}\n", data[i], data[i], data[i], data[i + 1]).as_bytes(),
                    )
                    .unwrap();
                }
                i += 2;
            }
            ColourType::TrueColour => {
                if debug {
                    file.write_all(
                        format!("{} {} {}\n", data[i], data[i + 1], data[i + 2]).as_bytes(),
                    )
                    .unwrap();
                } else {
                    file.write_all(
                        format!("{} {} {} 255\n", data[i], data[i + 1], data[i + 2]).as_bytes(),
                    )
                    .unwrap();
                }
                i += 3;
            }
            ColourType::TrueColourAlpha | ColourType::Indexed => {
                if debug {
                    file.write_all(
                        format!("{} {} {}\n", data[i], data[i + 1], data[i + 2]).as_bytes(),
                    )
                    .unwrap();
                } else {
                    file.write_all(
                        format!(
                            "{} {} {} {}\n",
                            data[i],
                            data[i + 1],
                            data[i + 2],
                            data[i + 3]
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                }
                i += 4;
            }
            _ => panic!(),
        }
    }
}
