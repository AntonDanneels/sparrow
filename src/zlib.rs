use std::collections::VecDeque;

struct Parser {}

pub fn parse(data: &mut VecDeque<u8>) -> Result<(), String> {
    if data.len() < 2 {
        return Err("Failed to read header".to_string());
    }

    let cmf = data.pop_front().unwrap();
    let flg = data.pop_front().unwrap();

    if (cmf as u16 * 256 + flg as u16) % 31 != 0 {
        return Err("Error in bitstream header".to_string());
    }

    let c_method = cmf & 0b1111;
    let c_info = cmf >> 4;

    println!("{:#0b}, {:#0b}", cmf, flg);

    println!("Method: {}, info: {}", c_method, c_info);

    let f_check = flg & 0b1111;
    let f_dict = (flg >> 5) & 0b1;
    let f_level = flg >> 6;

    println!(
        "Fcheck: {}, fdict: {}, flevel: {}",
        f_check, f_dict, f_level
    );

    if f_dict > 0 {
        if data.len() < 4 {
            return Err("Not enough data".to_string());
        }

        panic!("Not implemented");
    }

    if c_method != 8 {
        return Err("Not a DEFLATE stream".to_string());
    }

    let c = data.pop_front().unwrap();

    println!("{:#0b}", c);

    Ok(())
}
