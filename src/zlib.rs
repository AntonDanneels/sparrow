use std::collections::VecDeque;

/* As defined by the DEFLATE spec */
static LENGTH_EXTRA_BITS: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
static LENGTHS_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

static DIST_EXTRA_BITS: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
static DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

static CODE_LENGTH_INDICES: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

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
            self.buffer |= (self.data.pop_front().unwrap() as u32) << self.num_bits;
            self.num_bits += 8;
        }
    }

    fn get_n_bits(&mut self, n: u32) -> u16 {
        if self.num_bits < 24 {
            self.fill();
        }
        let result = self.buffer & ((1 << n) - 1);
        self.buffer >>= n;
        self.num_bits -= n;

        return result as u16;
    }
}

pub fn parse(data: &mut VecDeque<u8>) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err("Failed to read header".to_string());
    }

    let cmf = data.pop_front().unwrap();
    let flg = data.pop_front().unwrap();

    if (cmf as u16 * 256 + flg as u16) % 31 != 0 {
        return Err("Error in bitstream header".to_string());
    }

    let mut buffer = BitBuffer::new(data);

    let c_method = cmf & 0b1111;
    let _c_info = cmf >> 4;

    let _f_check = flg & 0b1111;
    let f_dict = (flg >> 5) & 0b1;
    let _f_level = flg >> 6;

    if f_dict > 0 {
        if data.len() < 4 {
            return Err("Not enough data".to_string());
        }
        panic!("Not implemented");
    }

    if c_method != 8 {
        return Err("Not a DEFLATE stream".to_string());
    }

    let mut output = Vec::with_capacity(buffer.data.len());
    let mut is_final = false;
    while !is_final {
        let b_final = buffer.get_n_bits(1);
        let b_type = buffer.get_n_bits(2);
        is_final = b_final != 0;

        if b_type == 0 {
            println!("No compression");
            panic!();
        } else if b_type == 0b01 {
            println!("Fixed Huffmann");
            panic!();
        } else if b_type == 0b10 {
            let h_lit = buffer.get_n_bits(5);
            let h_dist = buffer.get_n_bits(5);
            let h_clen = buffer.get_n_bits(4);

            let mut code_lengths = vec![0; CODE_LENGTH_INDICES.len()];

            for i in 0..(4 + h_clen) {
                let cl = buffer.get_n_bits(3);
                code_lengths[CODE_LENGTH_INDICES[i as usize]] = cl as u32;
            }
            let hf_codes = build_huffman_codes(&code_lengths, true);

            let lits = fill_with_huffman(257 + h_lit as usize, &hf_codes, &mut buffer)?;
            let dists = fill_with_huffman(1 + h_dist as usize, &hf_codes, &mut buffer)?;

            let hf_lit = build_huffman_codes(&lits, true);
            let hf_dist = build_huffman_codes(&dists, true);

            loop {
                let val = hf_lit.find(&mut buffer);
                if val.is_none() {
                    return Err("Failed to find a code".to_string());
                }
                let val = val.unwrap();
                match val {
                    0..=255 => {
                        output.push(val as u8);
                    }
                    256 => break,
                    257..=285 => {
                        let idx = (val - 257) as usize;
                        let extra = LENGTH_EXTRA_BITS[idx];
                        let len = LENGTHS_BASE[idx] + buffer.get_n_bits(extra);

                        let dist_val = hf_dist.find(&mut buffer);
                        if dist_val.is_none() {
                            return Err("Failed to find a code".to_string());
                        }
                        let dist_val = dist_val.unwrap();
                        if dist_val > 29 {
                            return Err("Corrupted datastream".to_string());
                        }

                        let idx = dist_val as usize;
                        let extra = DIST_EXTRA_BITS[idx];
                        let dist = DIST_BASE[idx] + buffer.get_n_bits(extra);

                        for _ in 0..len {
                            let v = output[(output.len() - dist as usize)];
                            output.push(v);
                        }
                    }
                    _ => return Err("Corrupted datastream".to_string()),
                }
            }
        } else if b_type == 0b11 {
            return Err("Invalid header in zlib stream".to_string());
        }
    }

    Ok(output)
}

fn decode_huffman(
    hf_codes: &Vec<(u32, u32, u32)>,
    buffer: &mut BitBuffer,
) -> Result<(u32, u32, u32), String> {
    let mut c = 0;
    let mut current_bits: u32 = 0;
    let mut prev_length = 0;
    let mut length_mask = 0;
    for pair in hf_codes.iter() {
        let length = pair.0 as u32;
        if current_bits != length {
            let bits_needed = length - current_bits;
            current_bits += bits_needed;
            c |= buffer.get_n_bits(bits_needed) << (current_bits - bits_needed);
        }
        if prev_length != length {
            prev_length = length;
            length_mask = (1 << pair.0) - 1;
        }
        if c as u32 & length_mask == pair.1 {
            return Ok(*pair);
        }
    }

    Err("No valid bit pattern found".to_string())
}

fn fill_with_huffman(
    num_needed: usize,
    hf_codes: &HuffmanTree,
    buffer: &mut BitBuffer,
) -> Result<Vec<u32>, String> {
    let mut lengths = Vec::with_capacity(num_needed);
    while lengths.len() < num_needed {
        let len = hf_codes.find(buffer);
        if len.is_none() {
            return Err("Failed to find a code".to_string());
        }
        let len = len.unwrap();
        match len {
            0..=15 => {
                lengths.push(len);
            }
            16 => {
                let c = buffer.get_n_bits(2);
                let previous = *lengths.last().unwrap();
                for _ in 0..(c + 3) {
                    lengths.push(previous);
                }
            }
            17 => {
                let c = buffer.get_n_bits(3);
                for _ in 0..(c + 3) {
                    lengths.push(0);
                }
            }
            18 => {
                let c = buffer.get_n_bits(7);
                for _ in 0..(c + 11) {
                    lengths.push(0);
                }
            }
            _ => {
                return Err("Corrupted bitstream".to_string());
            }
        }
    }

    Ok(lengths)
}

fn build_huffman_codes(bit_lengths: &Vec<u32>, reverse_bits: bool) -> HuffmanTree {
    let mut counts = vec![0; bit_lengths.len()];
    let mut next_code = vec![0; bit_lengths.len()];
    let mut codes = vec![0; bit_lengths.len()];

    let mut max = 0;
    for i in bit_lengths.iter() {
        counts[*i as usize] += 1;
        if *i as u32 > max {
            max = *i as u32;
        }
    }
    counts[0] = 0;

    let mut code: i32 = 0;
    for i in 1..(max + 1) {
        code = (code + counts[(i - 1) as usize]) << 1;
        next_code[i as usize] = code;
    }

    for i in 0..bit_lengths.len() {
        if bit_lengths[i] != 0 {
            codes[i] = next_code[bit_lengths[i] as usize];
            next_code[bit_lengths[i] as usize] += 1;
        }
    }

    let mut result = HuffmanTree::new();
    for i in 0..codes.len() {
        if bit_lengths[i] > 0 {
            let mut code;
            if reverse_bits {
                code = codes[i] as u16;
                code = code.reverse_bits() >> (16 - bit_lengths[i]);
            } else {
                code = codes[i] as u16;
            };
            if !result.insert(code, bit_lengths[i] as u16, i as u32) {
                panic!();
            }
        }
    }

    result
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

        loop {
            let c = buffer.get_n_bits(1);

            if c > 0 {
                if self.nodes[current_node].right.is_some() {
                    let idx = self.nodes[current_node].right.unwrap();
                    if self.nodes[idx].val.is_some() {
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

#[test]
fn test_huffman_tree() {
    let mut tree = HuffmanTree::new();
    tree.insert(6, 8, 100);
    tree.insert(5, 8, 3);
    tree.insert(16, 8, 5);
    println!("{:?}", tree.nodes);
    let mut data = vec![6, 5, 16].into_iter().collect();
    let mut buffer = BitBuffer::new(&mut data);

    assert_eq!(tree.find(&mut buffer), Some(100));
    assert_eq!(tree.find(&mut buffer), Some(3));
    assert_eq!(tree.find(&mut buffer), Some(5));
}

#[test]
fn test_huffman() {
    /* Note, values defined by the DEFLATE spec */
    let bit_lengths = vec![3, 3, 3, 3, 3, 2, 4, 4];
    let mut hf_codes = build_huffman_codes(&bit_lengths, false);
    let target = vec![2, 3, 4, 5, 6, 0, 14, 15];

    hf_codes.sort_unstable_by(|a, b| a.2.cmp(&b.2));
    for i in 0..hf_codes.len() {
        assert_eq!(hf_codes[i].1, target[i]);
    }
}

#[test]
fn bitbuffer_even() {
    let mut b = vec![0b10101010, 0b11001100, 0b11101110]
        .into_iter()
        .collect();
    let mut buffer = BitBuffer::new(&mut b);

    assert_eq!(buffer.get_n_bits(2), 2);
    assert_eq!(buffer.get_n_bits(2), 2);
    assert_eq!(buffer.get_n_bits(2), 2);
    assert_eq!(buffer.get_n_bits(2), 2);
    assert_eq!(buffer.get_n_bits(4), 12);
    assert_eq!(buffer.get_n_bits(4), 12);
    assert_eq!(buffer.get_n_bits(6), 46);
    assert_eq!(buffer.get_n_bits(2), 3);
}

#[test]
fn bitbuffer_uneven() {
    let mut b = vec![0b10101010, 0b11101110].into_iter().collect();
    let mut buffer = BitBuffer::new(&mut b);

    assert_eq!(buffer.get_n_bits(4), 10);
    assert_eq!(buffer.get_n_bits(5), 10);
    assert_eq!(buffer.get_n_bits(7), 119);
}
