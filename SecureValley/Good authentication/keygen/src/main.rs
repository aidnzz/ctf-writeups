#![feature(slice_as_chunks)]

fn validate(password: &[u8; 12]) -> Vec<u8> {
    const KEY: [u8; 3] = [7, 11, 9];

    let blocks = unsafe { password.as_chunks_unchecked::<4>() };

    blocks
        .iter()
        .zip(KEY.iter())
        .flat_map(|(block, key)| block.map(|b| b ^ key))
        .collect()
}

fn main() {
    let s = validate(b"sontTbxTjffe");
    println!("Test: {:?}", String::from_utf8_lossy(&s));
}
