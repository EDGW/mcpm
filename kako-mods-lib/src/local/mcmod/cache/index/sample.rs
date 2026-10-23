//! Standard imohash-1.0 sampling: MurmurHash3 x64-128, 16 KiB samples, 128 KiB threshold.

use std::io::{self, Cursor, Read, Seek, SeekFrom};

const SAMPLE_SIZE: u64 = 16 * 1024;
const SAMPLE_THRESHOLD: u64 = 128 * 1024;

/// Operates on a pinned handle; no UTF-8 path conversion or second file open.
pub(in crate::local::mcmod::cache) fn hash(
    reader: &mut (impl Read + Seek),
) -> io::Result<[u8; 16]> {
    let size = reader.seek(SeekFrom::End(0))?;
    reader.rewind()?;
    let mut samples = Vec::with_capacity(SAMPLE_THRESHOLD as usize);
    if size < SAMPLE_THRESHOLD {
        reader.take(size).read_to_end(&mut samples)?;
        if samples.len() as u64 != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "content changed during imohash",
            ));
        }
    } else {
        for offset in [0, size / 2, size - SAMPLE_SIZE] {
            reader.seek(SeekFrom::Start(offset))?;
            let start = samples.len();
            samples.resize(start + SAMPLE_SIZE as usize, 0);
            reader.read_exact(&mut samples[start..])?;
        }
    }
    let value = murmur3::murmur3_x64_128(&mut Cursor::new(samples), 0)?;
    let mut bytes = value.rotate_right(64).swap_bytes().to_le_bytes();
    let mut remaining = size;
    let mut offset = 0;
    while remaining >= 0x80 {
        bytes[offset] = remaining as u8 | 0x80;
        remaining >>= 7;
        offset += 1;
    }
    bytes[offset] = remaining as u8;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_vector_and_unsampled_differences() {
        assert_eq!(
            hash(&mut Cursor::new(b"hello")).unwrap(),
            [
                0x05, 0xd8, 0xa7, 0xb3, 0x41, 0xbd, 0x9b, 0x02, 0x5b, 0x1e, 0x90, 0x6a, 0x48, 0xae,
                0x1d, 0x19
            ]
        );
        let a = vec![b'a'; 256 * 1024];
        let mut b = a.clone();
        b[32 * 1024] = b'b';
        assert_eq!(
            hash(&mut Cursor::new(&a)).unwrap(),
            hash(&mut Cursor::new(&b)).unwrap()
        );
        assert_ne!(blake3::hash(&a), blake3::hash(&b));
    }
}
