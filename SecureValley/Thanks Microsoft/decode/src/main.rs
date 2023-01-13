#![feature(array_chunks)]

use std::fmt;

/// Error type if decoding message failed
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecodeError {
    /// If marker wasn't found in numbers
    MarkerError,
    /// If flags 'x' or 'y' contained invalid numbers
    FlagError,
}

impl std::error::Error for DecodeError {}

impl fmt::Display for DecodeError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let reason = match *self {
            Self::MarkerError => "Could not find marker in message",
            Self::FlagError => "Flag 'x' or 'y' contained invalid numbers",
        };
        fmt.write_str(reason)
    }
}

fn decode(b: u32) -> Result<u8, DecodeError> {
    const MARKER: u32 = 0x0E;
    const NIBBLE_WAS_GREATER_THAN_NINE: u8 = 26;

    ((b >> 20) == MARKER)
        .then_some(())
        .ok_or(DecodeError::MarkerError)?;

    let (x, y) = match (((b >> 4) & 0x3F) as u8, ((b >> 14) & 0x3F) as u8) {
        (x @ (11 | 26), y @ (14 | 26)) => (x, y),
        _ => return Err(DecodeError::FlagError),
    };

    let hn = match x {
        NIBBLE_WAS_GREATER_THAN_NINE => (b & 0x0F) as u8 + 9,
        _ => (b & 0x0F) as u8,
    };

    let ln = match y {
        NIBBLE_WAS_GREATER_THAN_NINE => ((b >> 10) & 0x0F) as u8 + 9,
        _ => ((b >> 10) & 0x0F) as u8,
    };

    let result = (hn << 4) | ln;
    Ok(result)
}

const MESSAGE: &[u8] = include_bytes!("message.txt");

fn main() {
    let flag: Vec<_> = MESSAGE
        .array_chunks::<6>()
        .map(|chunk| {
            let mut buffer = [0u8; 4];
            hex::decode_to_slice(chunk, &mut buffer[1..]).unwrap();
            decode(u32::from_be_bytes(buffer)).unwrap()
        })
        .collect();

    println!("{}", String::from_utf8_lossy(&flag));
}
