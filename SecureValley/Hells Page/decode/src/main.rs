#![feature(iter_array_chunks)]

fn decode(data: &str) -> Vec<u8> {
    data.split(&[':', '-'])
        .map(|s| s.parse::<u8>().unwrap())
        .array_chunks::<2>()
        .map(|[lo, hi]| hi << 4 | lo)
        .collect()
}

fn main() {
    let s = decode("3:5-5:6-3:6-6:5-1:6-12:6-11:7-10:6-3:7-15:5-9:6-3:7-15:5-8:6-5:6-12:6-12:6-15:5-2:6-5:7-4:7-15:5-6:6-5:7-14:6-1:2-13:7");
    println!("{}", String::from_utf8_lossy(&s));
}
