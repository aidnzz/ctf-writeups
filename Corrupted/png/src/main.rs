#![feature(slice_take)]

mod png {
    use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
    use zerocopy::{
        network_endian::U32, Immutable, IntoBytes, KnownLayout, TryFromBytes, TryReadError,
    };

    use flate2::{read::ZlibEncoder, write::ZlibDecoder, Compression, Crc};
    use std::{
        io::{self, Read, Write},
        mem::size_of,
        ops::AddAssign,
        slice::{from_raw_parts, from_raw_parts_mut},
    };

    const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
    pub struct Rgba {
        pub r: u8,
        pub g: u8,
        pub b: u8,
        pub a: u8,
    }

    impl Rgba {
        #[inline]
        pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
            Self { r, g, b, a }
        }
    }

    impl AddAssign for Rgba {
        fn add_assign(&mut self, rhs: Self) {
            *self = Rgba {
                r: self.r.wrapping_add(rhs.r),
                g: self.g.wrapping_add(rhs.g),
                b: self.b.wrapping_add(rhs.b),
                a: self.a.wrapping_add(rhs.a),
            }
        }
    }

    // We only care about rbga parse will fail if colour type other than 
    // True Colour with alpha is present
    #[repr(u8)]
    #[non_exhaustive]
    #[derive(TryFromBytes, Clone, Copy, Debug, IntoBytes, Immutable)]
    pub enum ColourType {
        TrueColourWithAlpha = 6,
    }

    #[repr(u8)]
    #[non_exhaustive]
    #[derive(TryFromBytes, Clone, Copy, Debug, IntoBytes, Immutable)]
    pub enum BitDepth {
        Eight = 8,
    }

    #[repr(C, packed)]
    #[derive(TryFromBytes, Debug, IntoBytes, Immutable)]
    pub struct ImageHeader {
        pub width: U32,
        pub height: U32,
        pub bit_depth: BitDepth,
        pub colour_type: ColourType,
        pub compression_method: u8,
        pub filter_method: u8,
        pub interlace_method: u8,
    }

    struct Chunk<'a> {
        pub data: &'a [u8],
        pub chunk_type: [u8; 4],
        pub crc: u32,
    }

    struct ChunkIterator<'a>(&'a [u8]);

    impl<'a> ChunkIterator<'a> {
        #[inline]
        fn new(data: &'a [u8]) -> Self {
            Self(data)
        }
    }

    impl<'a> Iterator for ChunkIterator<'a> {
        type Item = Chunk<'a>;

        fn next(&mut self) -> Option<Self::Item> {
            let length = self.0.read_u32::<BigEndian>().ok()? as usize;

            let mut chunk_type = [0; 4];
            self.0.read_exact(&mut chunk_type).ok()?;
            
            let data = <[u8]>::take(&mut self.0, ..length)?;
            let crc = self.0.read_u32::<BigEndian>().ok()?;

            Some(Chunk {
                chunk_type,
                data,
                crc,
            })
        }
    }

    #[derive(Debug)]
    pub enum ParseError<'a> {
        InvalidSignature,
        ImageHeaderNotFound,
        DecompressError(io::Error),
        ImageHeaderInvalid(TryReadError<&'a [u8], ImageHeader>),
    }

    pub struct RgbaImage {
        buffer: Box<[u8]>,
        header: ImageHeader,
    }

    #[repr(u8)]
    #[derive(TryFromBytes, KnownLayout, Immutable, Debug, PartialEq)]
    pub enum FilterType {
        NoFilter = 0,
        Sub = 1,
        Up = 2,
        Average = 3,
        Paeth = 4,
    }

    fn write_chunk<W: Write>(writer: &mut W, name: &[u8; 4], data: &[u8]) -> io::Result<()> {
        writer.write_u32::<BigEndian>(data.len() as u32)?;
        writer.write_all(name)?;
        writer.write_all(data)?;

        let mut crc = Crc::new();
        crc.update(name);
        crc.update(data);
        writer.write_u32::<BigEndian>(crc.sum())?;

        Ok(())
    }

    impl RgbaImage {
        pub fn parse(mut png: &[u8]) -> Result<Self, ParseError> {
            let mut signature = [0; 8];
            png.read_exact(&mut signature).map_err(|_| ParseError::InvalidSignature)?;

            if signature != SIGNATURE {
                return Err(ParseError::InvalidSignature);
            }

            let mut chunks = ChunkIterator::new(png);

            let header_chunk = chunks
                .next()
                .filter(|c| c.chunk_type == *b"IHDR")
                .ok_or(ParseError::ImageHeaderNotFound)?;

            let header = ImageHeader::try_read_from_bytes(header_chunk.data)
                .map_err(ParseError::ImageHeaderInvalid)?;

            let mut writer = Vec::new();
            let mut z = ZlibDecoder::new(writer);

            for chunk in chunks {
                if chunk.chunk_type == *b"IDAT" {
                    z.write_all(chunk.data)
                        .map_err(ParseError::DecompressError)?;
                }
            }

            writer = z.finish().map_err(ParseError::DecompressError)?;

            Ok(Self {
                header,
                buffer: writer.into(),
            })
        }

        pub fn lines(&self) -> impl Iterator<Item = (&FilterType, &[Rgba])> {
            let length = (self.header.width.get() as usize * size_of::<Rgba>()) + size_of::<FilterType>();

            self.buffer.chunks_exact(length).map(|r| {
                let (filter, pixels) = FilterType::try_ref_from_prefix(r).unwrap();
                let pixels = unsafe {
                    from_raw_parts(
                        pixels.as_ptr().cast::<Rgba>(),
                        self.header.width.get() as usize,
                    )
                };
                (filter, pixels)
            })
        }

        pub fn lines_mut(&mut self) -> impl Iterator<Item = (&mut FilterType, &mut [Rgba])> {
            let length = (self.header.width.get() as usize * size_of::<Rgba>()) + size_of::<FilterType>();

            self.buffer.chunks_exact_mut(length).map(|r| {
                let (filter, pixels) = FilterType::try_mut_from_prefix(r).unwrap();
                let pixels = unsafe {
                    from_raw_parts_mut(
                        pixels.as_mut_ptr().cast::<Rgba>(),
                        self.header.width.get() as usize,
                    )
                };
                (filter, pixels)
            })
        }

        pub fn recon(&mut self) {
            // Unfilters each scanline
            let mut above = vec![Rgba::default(); self.header.width.get() as usize]; // Initial row

            for (filter, current) in self.lines_mut() {
                match filter {
                    FilterType::Up => recon::paeth(current, &above),
                    FilterType::Paeth => recon::paeth(current, &above),
                    FilterType::Average => recon::average(current, &above),
                    FilterType::Sub => recon::sub(current),
                    FilterType::NoFilter => {}
                }

                above.copy_from_slice(current);
                *filter = FilterType::NoFilter;
            }
        }

        pub fn encode<W: Write>(&self, mut writer: W) -> io::Result<()> {
            // Write png signature
            writer.write_all(&SIGNATURE)?;
            write_chunk(&mut writer, b"IHDR", self.header.as_bytes())?;

            // Compress data
            let mut compressed = Vec::new();
            ZlibEncoder::new(&self.buffer[..], Compression::fast()).read_to_end(&mut compressed)?;

            // Write IDAT
            write_chunk(&mut writer, b"IDAT", &compressed)?;
            // Write end chunk
            write_chunk(&mut writer, b"IEND", &[])
        }
    }

    mod recon {
        // CBA to SIMD
        use super::Rgba;

        pub fn paeth_predictor(left: u8, above: u8, upper_left: u8) -> u8 {
            // To prevent overflows
            let predictor = left as i16 + above as i16 - upper_left as i16;

            let predictor_left = predictor.abs_diff(left as i16);
            let predictor_above = predictor.abs_diff(above as i16);
            let predictor_upper_left = predictor.abs_diff(upper_left as i16);

            if predictor_left <= predictor_above && predictor_left <= predictor_upper_left {
                return left;
            }
            
            if predictor_above <= predictor_upper_left {
                return above;
            }

            upper_left
        }

        pub fn up(current_line: &mut [Rgba], above: &[Rgba]) {
            for (pixel, &above_pixel) in current_line.iter_mut().zip(above) {
                *pixel += above_pixel;
            }
        }

        pub fn sub(current_line: &mut [Rgba]) {
            let mut left_pixel = Rgba::default();

            for pixel in current_line {
                *pixel += left_pixel;
                left_pixel = *pixel;
            }
        }

        pub fn average(current_line: &mut [Rgba], above: &[Rgba]) {
            let mut left_pixel = Rgba::default();

            for (pixel, &above_pixel) in current_line.iter_mut().zip(above) {
                *pixel += Rgba::new(
                    ((left_pixel.r as u16 + above_pixel.r as u16) >> 1) as u8,
                    ((left_pixel.g as u16 + above_pixel.g as u16) >> 1) as u8,
                    ((left_pixel.b as u16 + above_pixel.b as u16) >> 1) as u8,
                    ((left_pixel.a as u16 + above_pixel.a as u16) >> 1) as u8,
                );

                left_pixel = *pixel;
            }
        }

        pub fn paeth(current_line: &mut [Rgba], above: &[Rgba]) {
            let mut left_pixel = Rgba::default();
            let mut upper_left_pixel = Rgba::default();

            for (pixel, &above_pixel) in current_line.iter_mut().zip(above) {
                *pixel += Rgba::new(
                    paeth_predictor(left_pixel.r, above_pixel.r, upper_left_pixel.r),
                    paeth_predictor(left_pixel.g, above_pixel.g, upper_left_pixel.g),
                    paeth_predictor(left_pixel.b, above_pixel.b, upper_left_pixel.b),
                    paeth_predictor(left_pixel.a, above_pixel.a, upper_left_pixel.a),
                );

                left_pixel = *pixel;
                upper_left_pixel = above_pixel;
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn test_up() {
                let mut row = [
                    Rgba::new(1, 0, 0, 0),
                    Rgba::new(1, 3, 2, 0),
                    Rgba::new(0, 4, 3, 0),
                    Rgba::new(5, 0, 1, 0),
                ];

                const PREVIOUS: [Rgba; 4] = [
                    Rgba::new(128, 60, 40, 10),
                    Rgba::new(130, 64, 40, 10),
                    Rgba::new(128, 61, 40, 10),
                    Rgba::new(130, 46, 20, 10),
                ];

                const RESULT: [Rgba; 4] = [
                    Rgba::new(129, 60, 40, 10),
                    Rgba::new(131, 67, 42, 10),
                    Rgba::new(128, 65, 43, 10),
                    Rgba::new(135, 46, 21, 10),
                ];

                up(&mut row, &PREVIOUS);
                assert_eq!(row, RESULT);
            }

            #[test]
            fn test_average() {
                let mut row = [
                    Rgba::new(65, 30, 20, 5),
                    Rgba::new(2, 5, 2, 5),
                    Rgba::new(255, 1, 2, 5),
                    Rgba::new(6, 247, 10, 5),
                ];

                const PREVIOUS: [Rgba; 4] = [
                    Rgba::new(128, 60, 40, 10),
                    Rgba::new(130, 64, 40, 10),
                    Rgba::new(128, 61, 40, 10),
                    Rgba::new(130, 46, 20, 10),
                ];

                const RESULT: [Rgba; 4] = [
                    Rgba::new(129, 60, 40, 10),
                    Rgba::new(131, 67, 42, 15),
                    Rgba::new(128, 65, 43, 17),
                    Rgba::new(135, 46, 41, 18),
                ];

                average(&mut row, &PREVIOUS);
                assert_eq!(row, RESULT);
            }

            #[test]
            fn test_paeth() {
                let mut row = [
                    Rgba::new(1, 0, 5, 0),
                    Rgba::new(1, 4, 255, 0),
                    Rgba::new(255, 252, 3, 0),
                    Rgba::new(4, 2, 3, 0),
                ];

                const PREVIOUS: [Rgba; 4] = [
                    Rgba::new(128, 60, 90, 10),
                    Rgba::new(129, 61, 90, 10),
                    Rgba::new(128, 60, 91, 10),
                    Rgba::new(130, 65, 97, 10),
                ];

                const RESULT: [Rgba; 4] = [
                    Rgba::new(129, 60, 95, 10),
                    Rgba::new(130, 65, 94, 10),
                    Rgba::new(128, 61, 97, 10),
                    Rgba::new(134, 67, 100, 10),
                ];

                paeth(&mut row, &PREVIOUS);
                assert_eq!(row, RESULT);
            }

            #[test]
            fn test_sub() {
                let mut row = [
                    Rgba::new(128, 60, 40, 10),
                    Rgba::new(2, 4, 0, 0),
                    Rgba::new(254, 253, 0, 0),
                    Rgba::new(2, 241, 236, 0),
                ];

                const RESULT: [Rgba; 4] = [
                    Rgba::new(128, 60, 40, 10),
                    Rgba::new(130, 64, 40, 10),
                    Rgba::new(128, 61, 40, 10),
                    Rgba::new(130, 46, 20, 10),
                ];

                sub(&mut row);
                assert_eq!(row, RESULT);
            }
        }
    }
}

use png::{FilterType, RgbaImage};
use std::{fs, io::BufWriter};

fn fix(image: &mut RgbaImage) {
    for (filter, _row) in image.lines_mut() {
        if *filter == FilterType::Up {
            *filter = FilterType::Sub;
        }
    }
}

fn main() -> std::io::Result<()> {
    let buffer = fs::read("/home/aidnzz/ctf/Corrupted/challenge.png")?;
    let output = BufWriter::new(fs::File::create("/home/aidnzz/ctf/Corrupted/output.png")?);

    let mut image = png::RgbaImage::parse(&buffer).unwrap();
    
    fix(&mut image);
    image.encode(output)?;

    Ok(())
}
