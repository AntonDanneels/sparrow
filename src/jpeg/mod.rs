use std::collections::VecDeque;

enum ParsingMode {
    Baseline,

    ExtendedHuffman,
    ProgressiveHuffman,
    LosslessHuffman,

    ExtendedArithmetic,
    ProgressiveArithmetic,
    LosslessArithmetic,

    Invalid,
}

#[derive(Debug, Clone)]
struct Component {
    identifier: u8,
    sampling_hor: u8,
    sampling_ver: u8,
    quant_table: usize,
    dc_table: usize,
    ac_table: usize,
}

impl Component {
    fn new(identifier: u8, sampling_hor: u8, sampling_ver: u8, quant_table: usize) -> Component {
        Component {
            identifier: identifier,
            sampling_hor: sampling_hor,
            sampling_ver: sampling_ver,
            quant_table: quant_table,
            dc_table: 0,
            ac_table: 0,
        }
    }
}

pub struct Parser {
    // File data: JPEG chunks
    compressed_data: VecDeque<u8>,
    has_end: bool,

    dequant_table:[[u16; 64]; 4], 

                         /*size, code, val*/
    //hufftables: Vec<Vec<(u16, u16, u16)>>,
    dc_hufftables: Vec<HuffmanTree>,
    ac_hufftables: Vec<HuffmanTree>,

    parsing_mode: ParsingMode,

    depth: u8,
    width: usize,
    height: usize,
    num_components: u32,

    components: Vec<Component>,
}

#[derive(Debug, Clone)]
enum ChunkType {
    SOF0,

    DHT,

    SOS,
    DQT,

    APP0,
    APP1,
    APP2,
    APP_UNKNOWN,

    UNKNOWN,
}

const fn to_u16(a: [u8; 2]) -> u16 {
    let mut result: u16 = 0;
    result |= (a[0] as u16) << 8 * 1;
    result |= (a[1] as u16) << 8 * 0;

    result
}

const DEZIGZAG:[usize; 64] = [
    0,  1,  8, 16,  9,  2,  3, 10,
   17, 24, 32, 25, 18, 11,  4,  5,
   12, 19, 26, 33, 40, 48, 41, 34,
   27, 20, 13,  6,  7, 14, 21, 28,
   35, 42, 49, 56, 57, 50, 43, 36,
   29, 22, 15, 23, 30, 37, 44, 51,
   58, 59, 52, 45, 38, 31, 39, 46,
   53, 60, 61, 54, 47, 55, 62, 63,
];

impl Parser {
    pub fn new() -> Parser {
        Parser {
            compressed_data: VecDeque::new(),
            has_end: false,

            dequant_table: [[0; 64]; 4],

            dc_hufftables: Vec::new(), 
            ac_hufftables: Vec::new(), 
            parsing_mode: ParsingMode::Invalid,

            depth: 0,
            width: 0,
            height: 0,
            num_components: 0,

            components: Vec::new(),
        }
    }

    fn parse_u8(&mut self) -> Result<u8, String> {
        if self.compressed_data.len() < 1 {
            return Err("Not enough data".to_string());
        }

        Ok(self.compressed_data.pop_front().unwrap())
    }

    fn parse_u16(&mut self) -> Result<u16, String> {
        let mut result: u16 = 0;
        if self.compressed_data.len() < 2 {
            return Err("Not enough data".to_string());
        }
        for i in 0..2 {
            let byte = self.compressed_data.pop_front().unwrap() as u16;
            result |= byte << 8 * (1 - i);
        }

        Ok(result)
    }

    fn get_chunk_type(&mut self) -> Result<ChunkType, String> {
        let markers: [(u16, ChunkType); 19] = [
            (to_u16([0xff, 0xE0]), ChunkType::APP0),
            (to_u16([0xff, 0xE1]), ChunkType::APP1),
            (to_u16([0xff, 0xE2]), ChunkType::APP2),

            (to_u16([0xff, 0xE3]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xE4]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xE5]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xE6]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xE7]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xE8]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xE9]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xEA]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xEB]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xEC]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xED]), ChunkType::APP_UNKNOWN),
            (to_u16([0xff, 0xEF]), ChunkType::APP_UNKNOWN),

            (to_u16([0xff, 0xC4]), ChunkType::DHT),
            (to_u16([0xff, 0xDB]), ChunkType::DQT),
            (to_u16([0xff, 0xDA]), ChunkType::SOS),
            (to_u16([0xff, 0xC0]), ChunkType::SOF0),
        ];

        let data = self.parse_u16()?;
        println!("Marker: {:#x}", data);
        for marker in markers.iter() {
            if marker.0 == data {
                println!("Marker: {:?}", marker.1);
                return Ok(marker.1.clone());
            }
        }
    
        Err(format!("Not implemented"))
    }

    fn parse_chunk(&mut self) -> Result<(), String> {
        match self.get_chunk_type()? {
            ChunkType::APP0 => self.parse_app0(),
            ChunkType::APP1 => self.parse_app1(),
            ChunkType::APP2 => self.parse_app2(),
            ChunkType::APP_UNKNOWN => self.parse_app_unknown(),
            ChunkType::DQT => self.parse_dqt(),
            ChunkType::DHT => self.parse_dht(),
            ChunkType::SOS => self.parse_sos(),
            ChunkType::SOF0 => self.parse_sof0(),
            ChunkType::UNKNOWN => Err("Don't know what to do".to_string()),
            _ => Err("Don't know what to do".to_string())
        }
    }

    fn is_jpeg(&mut self) -> Result<(), String> {
        let soi_header = [0xff, 0xD8];

        for byte in soi_header.iter() {
            let b = self.parse_u8()?;
            if b != *byte as u8 {
                return Err(format!("Not a JPEG: {}", b));
            }
        }

        Ok(())
    }

    fn parse_app0(&mut self) -> Result<(), String> {
        let length = self.parse_u16()?;
        println!("Length: {}", length);

        let jfif_marker = [0x4A, 0x46, 0x49, 0x46, 0x00];
        for i in 0..5 {
            let b = self.parse_u8()?;
            if b != jfif_marker[i] {
                return Err("Not a JFIF".to_string());
            }
        }
        println!("Version: {:#x}.{:#x}", self.parse_u8()?, self.parse_u8()?);
        println!("Units: {}", self.parse_u8()?);
        println!("Xdense: {}, Ydense: {}", self.parse_u16()?, self.parse_u16()?);
        let x_thumb = self.parse_u8()?;
        let y_thumb = self.parse_u8()?;
        println!("Xthumb: {}, Ythumb: {}", x_thumb, y_thumb);

        for _ in 0..(x_thumb * y_thumb * 3) {
            let _ = self.parse_u8()?;
        }

        Ok(())
    }

    fn parse_app1(&mut self) -> Result<(), String> {
        let length = self.parse_u16()?;
        println!("Length: {}", length);
        for _ in 0..(length - 2) {
            let _ = self.parse_u8()?;
        }

        Ok(())
    }

    fn parse_app2(&mut self) -> Result<(), String> {
        let length = self.parse_u16()?;
        println!("Length: {}", length);
        for _ in 0..(length - 2) {
            let _ = self.parse_u8()?;
        }

        Ok(())
    }

    fn parse_app_unknown(&mut self) -> Result<(), String> {
        let length = self.parse_u16()?;
        println!("Length: {}", length);
        for _ in 0..(length - 2) {
            let _ = self.parse_u8()?;
        }

        Ok(())
    }

    fn parse_dqt(&mut self) -> Result<(), String> {
        let mut length = self.parse_u16()? - 2;
        while length > 0 {
            let v = self.parse_u8()?;
            let pq = v >> 4;
            let tq = v & 0b1111;
            if tq > 4 {
                return Err(format!("Badly formed JPEG"));
            }
            println!("\tGot DQT table: {}", tq);
            for i in 0..64 {
                let qk = match pq {
                    0 => self.parse_u8()? as u16,
                    1 => self.parse_u16()?,
                    _ => return Err(format!("Badly formed JPEG"))
                };
                if i == 1 || i == 2 {
                    println!("DQT: {}", qk);
                }
                self.dequant_table[tq as usize][DEZIGZAG[i]] = qk;
            }
            length -= match pq {
                0 => 65,
                1 => 129,
                _ => length
            }; 
        }
    
        Ok(())
    }

    fn parse_sof0(&mut self) -> Result<(), String> {
        let mut length = self.parse_u16()?;
        let p = self.parse_u8()?; 
        let y = self.parse_u16()?; 
        let x = self.parse_u16()?; 
        let nf = self.parse_u8()?; 

        println!("Length: {}", length);
        println!("{}x{}@{}", x, y, p);
        println!("Components: {}", nf);

        self.width = x as usize;
        self.height = y as usize;
        self.depth = p;
        self.num_components = nf as u32;
        self.parsing_mode = ParsingMode::Baseline;

        length -= 2 + 1 + 2 + 2 + 1;
        while length > 0 {
            let ci = self.parse_u8()?; 
            let h = self.parse_u8()?; 
            let hi = h >> 4;
            let vi = h & 0b1111;
            let tqi = self.parse_u8()?; 

            let c = Component::new(ci, hi, vi, tqi as usize);
            self.components.push(c);

            println!("\tci: {}", ci);
            println!("\thi: {}, vi: {}", hi, vi);
            println!("\ttqi: {}", tqi);
            println!("\t--------------------");

            length -= 1 + 1 + 1;
        }

        Ok(())
    }

    fn gen_huffman_table(&self, sizes: Vec<u8>) -> (Vec<usize>, Vec<u32>) {
        let mut i = 1;
        let mut j = 1;
        let mut k = 0;

        let mut huffsizes = vec![0; 256];
        while i <= 16 {
            while j <= sizes[i - 1] {
                huffsizes[k] = i;
                k += 1;
                j += 1;
            }
            i += 1;
            j = 1;
        }
        huffsizes[k] = 0;

        let mut k = 0;
        let mut code = 0;
        let mut si = huffsizes[0];

        let mut huffcodes = vec![0; 256];
        loop {
            loop {
                huffcodes[k] = code;
                code += 1;
                k += 1;
                if huffsizes[k] != si {
                    break;
                }
            }
            if huffsizes[k] == 0 {
                break;
            }
            while huffsizes[k] != si {
                code <<= 1;
                si += 1;
            }
        }
        (huffsizes, huffcodes)
    }

    fn parse_dht(&mut self) -> Result<(), String> {
        let mut length = self.parse_u16()? - 2;
        println!("Length: {}", length);

        while length > 0 {
            let t = self.parse_u8()?;
            let tc = t >> 4; /* table class (0=DC, 1=AC)*/
            let th = t & 0b1111; /* destination */

            println!("\ttc: {}, th: {}", tc, th);

            let mut size = 0;
            let mut sizes = vec![0; 16];
            for i in 0..16 {
                let li = self.parse_u8()?;
                size += li as u16;
                //println!("\t\tli: {}", li);
                sizes[i] = li;
            }

            let (sizes, code) = self.gen_huffman_table(sizes);
            println!("Size: {:?}", size);
            let mut tree = HuffmanTree::new();
            for i in 0..size as usize {
                let vlj = self.parse_u8()?;
                tree.insert(code[i] as u16, sizes[i] as u16, vlj as u32);
            }

            if th > 4 {
                return Err("Corrupted data (DHT)".to_string());
            }

            match tc {
                0 => {
                    if th as usize >= self.dc_hufftables.len() {
                        self.dc_hufftables.resize_with(4, || { HuffmanTree::new() });
                    }
                    self.dc_hufftables[th as usize] = tree;
                }
                1 => {
                    if th as usize >= self.ac_hufftables.len() {
                        self.ac_hufftables.resize_with(4, || { HuffmanTree::new() });
                    }
                    self.ac_hufftables[th as usize] = tree;
                }
                _ => return Err("Corrupted data (DHT)".to_string())
            }

            length -= 17 + size;
        }

        Ok(())
    }

    fn parse_sos(&mut self) -> Result<(), String> {
        let mut length = self.parse_u16()? - 2;
        println!("Length: {}", length);

        let ns = self.parse_u8()?;
        println!("Ns: {}", ns);

        let mut comp_order = Vec::new();
        let mut index = 0;
        for _ in 0..ns {
            let cs = self.parse_u8()?; /* component selector */
            let t = self.parse_u8()?;
            let td = t >> 4; /*DC table*/
            let ta = t & 0b1111; /*AC table*/

            for component in &mut self.components {
                if component.identifier == cs {
                    component.dc_table = td as usize;
                    component.ac_table = ta as usize;

                    comp_order.push(index);
                    index += 1;
                }
            }

            println!("{}: {} {}", cs, td, ta);
        }
        let ss = self.parse_u8()?;
        let se = self.parse_u8()?;
        let a = self.parse_u8()?;
        let ah = a >> 4;
        let al = a & 0b1111;

        match self.parsing_mode {
            ParsingMode::Baseline => {},
            ParsingMode::Invalid => {
                return Err("Corrupted image".to_string());
            }
            _ => {
                return Err("Not implemented".to_string());
            }
        }

        println!("ss: {}, se: {}, ah: {}, al: {}", ss, se, ah, al);

        println!("{:?}", self.components[comp_order[0]]);

        /*
        println!("{:#0b}", self.parse_u8()?);
        println!("{:#0b}", self.parse_u8()?);
        println!("{:#0b}", self.parse_u8()?);
        */
        
        //FIXME: remove clones
        let mut data = self.compressed_data.clone(); 
        let mut bitbuffer = BitBuffer::new(&mut data);
        let component = self.components[comp_order[0]].clone();
        self.parse_block(&mut bitbuffer, &component);

        Ok(())
    }

    fn extend_receive(buffer: &mut BitBuffer, val: u32) -> i16 {
        let mut result = buffer.get_n_bits(val) as i16;
        let vt = 1 << (val - 1); 
        if result < vt {
            result += (-1 << val) + 1;
        }

        result
    }

    fn parse_block(&mut self, buffer: &mut BitBuffer, component: &Component) {
        //Parse DC coeff
        
        let val = self.dc_hufftables[component.dc_table].find(buffer).unwrap();
        let result = Parser::extend_receive(buffer, val);
        println!("Receive+extend: {:?}", result);
    }

    pub fn parse(&mut self, data: Vec<u8>) -> Result<(), String> {
        self.compressed_data = data.into_iter().collect();

        self.is_jpeg()?;
        while !self.has_end {
            match self.parse_chunk() {
                Ok(_) => {}
                Err(e) => {
                    println!("Error while parsing JPEG: {}", e);
                    panic!();
                }
            }
            println!("-------------------------------------------");
        }
    
        Err(format!("Not impl"))
    }
}

struct BitBuffer<'a> {
    buffer: u32,
    num_bits: u32,
    data: &'a mut VecDeque<u8>,
}

impl<'a> BitBuffer<'a> {
    fn new(data: &mut VecDeque<u8>) -> BitBuffer {
        BitBuffer {
            buffer: 0,
            num_bits: 0,
            data: data,
        }
    }

    fn fill(&mut self) {
        while self.num_bits < 24 {
            if self.data.len() == 0 {
                break;
            }
            self.buffer |= (self.data.pop_front().unwrap().reverse_bits() as u32) << self.num_bits;
            self.num_bits += 8;
        }
    }

    fn reset(&mut self) {
        while self.num_bits > 0 {
            self.data.push_front((self.buffer & 0xff) as u8);
            self.buffer >>= 8;
            self.num_bits -= 8;
        }
    }

    fn get_n_bits(&mut self, n: u32) -> u16 {
        if self.num_bits < 24 {
            self.fill();
        }
        //let result = self.buffer & ((1 << n) - 1);
        let mut result = 0;
        for i in 0..n {
            result |= (self.buffer >> i & 0b1) << (n - 1 - i);
        }
        self.buffer >>= n;
        self.num_bits -= n;

        return result as u16;
    }
}

#[derive(Debug)]
struct HuffmanTree {
    nodes: Vec<HuffmanNode>,
}

#[derive(Debug)]
struct HuffmanNode {
    left: Option<usize>,
    right: Option<usize>,
    val: Option<u32>,
}

impl HuffmanNode {
    fn new() -> HuffmanNode {
        HuffmanNode {
            left: None,
            right: None,
            val: None,
        }
    }
}

impl HuffmanTree {
    fn new() -> HuffmanTree {
        let mut nodes = Vec::new();
        nodes.push(HuffmanNode::new());
        HuffmanTree { nodes: nodes }
    }

    fn new_node(&mut self) -> usize {
        let result = self.nodes.len();
        self.nodes.push(HuffmanNode::new());

        result
    }

    fn find(&self, buffer: &mut BitBuffer) -> Option<u32> {
        let mut current_node = 0;

        let mut i = 0;
        if buffer.num_bits < 24 {
            buffer.fill();
        }
        loop {
            let c = (buffer.buffer >> i) & 0b1;
            i += 1;

            if c > 0 {
                if self.nodes[current_node].right.is_some() {
                    let idx = self.nodes[current_node].right.unwrap();
                    if self.nodes[idx].val.is_some() {
                        buffer.buffer >>= i;
                        buffer.num_bits -= i;
                        return self.nodes[idx].val;
                    }
                    current_node = idx;
                } else {
                    return None;
                }
            } else {
                if self.nodes[current_node].left.is_some() {
                    let idx = self.nodes[current_node].left.unwrap();
                    if self.nodes[idx].val.is_some() {
                        buffer.buffer >>= i;
                        buffer.num_bits -= i;
                        return self.nodes[idx].val;
                    }
                    current_node = idx;
                } else {
                    return None;
                }
            }
        }
    }

    fn insert(&mut self, code: u16, bit_length: u16, val: u32) -> bool {
        let mut c = code;
        let mut current_node = 0;
        for _ in 0..bit_length {
            let bit = c & 0b1;
            c >>= 1;

            if bit > 0 {
                if self.nodes[current_node].right.is_none() {
                    self.nodes[current_node].right = Some(self.new_node());
                }
                current_node = self.nodes[current_node].right.unwrap();
            } else {
                if self.nodes[current_node].left.is_none() {
                    self.nodes[current_node].left = Some(self.new_node());
                }
                current_node = self.nodes[current_node].left.unwrap();
            }
        }
        if self.nodes[current_node].val.is_some() {
            return false;
        }
        self.nodes[current_node].val = Some(val);

        true
    }
}
