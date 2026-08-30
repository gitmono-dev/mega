//! Wire-compatible port of Libra's deterministic `fastcdc-v1` chunker
//! (libra main 92e1d64a, lore.md §6). External FastCDC crate variants do not
//! necessarily use these frozen parameters and must not replace it in place.
//!
//! No external crate: a gear-hash rolling fingerprint with normalized chunking
//! and hard min/max clamps. The parameters and the GEAR table are **FROZEN** for
//! v1 — changing any of them changes chunk boundaries and would be a breaking
//! `fastcdc-v2` (never an in-place edit), because §6.1 requires byte-identical
//! boundaries across versions/clones/clients.
//!
//! Determinism: for the frozen (GEAR, MIN, AVG, MAX, masks), boundaries are a
//! pure deterministic function of the input bytes — the same bytes always yield
//! the same `Vec<Chunk>`. The streaming [`chunk_reader`] and the in-memory
//! [`chunk_bytes`] share one code path, so they can never disagree.

use std::io::{self, Read};

use super::sha256_hex;

/// The frozen chunker algorithm identifier recorded in the manifest.
pub const ALGORITHM: &str = "fastcdc-v1";

/// Minimum chunk size: no boundary may fire before this many bytes (512 KiB).
pub const MIN_SIZE: usize = 512 * 1024;
/// Target average chunk size (2 MiB); `log2(AVG_SIZE) == 21` sets the mask width.
pub const AVG_SIZE: usize = 2 * 1024 * 1024;
/// Maximum chunk size: a boundary is forced here (8 MiB; matches the §6.4
/// capability example `max_chunk_size = 8388608`).
pub const MAX_SIZE: usize = 8 * 1024 * 1024;

// Normalized chunking (FastCDC): use a STRICTER mask before the average size
// (biases toward larger chunks, pushing the cut toward AVG) and a LOOSER mask
// after it (prevents runaway chunks). avg_bits = log2(AVG_SIZE) = 21.
// MASK_STRICT uses avg_bits + 2 = 23 high bits; MASK_LOOSE uses avg_bits - 2 =
// 19 high bits. Masking the well-mixed HIGH bits of the gear hash.
const MASK_STRICT: u64 = ((1u64 << 23) - 1) << 41;
const MASK_LOOSE: u64 = ((1u64 << 19) - 1) << 45;

/// Deterministic 256-entry gear table, built at compile time from a fixed
/// splitmix64 sequence (frozen seed + constants) so it is fully reproducible
/// and carries no external data file.
const GEAR: [u64; 256] = build_gear();

const fn build_gear() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15; // frozen seed
    let mut i = 0;
    while i < 256 {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        table[i] = z;
        i += 1;
    }
    table
}

/// One content-defined chunk: its byte range within the media object and the
/// lowercase-hex SHA-256 of its RAW (uncompressed) bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub offset: u64,
    pub length: u64,
    pub chunk_hash: String,
}

/// Chunk a byte stream. Returns the ordered chunk list. Degenerate inputs (a
/// FROZEN contract, pinned by tests): empty input → zero chunks; input shorter
/// than [`MIN_SIZE`] → exactly one chunk equal to the whole input (no boundary
/// can fire before MIN).
pub fn chunk_reader<R: Read>(mut reader: R) -> io::Result<Vec<Chunk>> {
    let mut out = Vec::new();
    let mut buf = [0u8; 65536];
    // Bytes of the chunk currently being accumulated (bounded by MAX_SIZE).
    let mut cur: Vec<u8> = Vec::with_capacity(MAX_SIZE.min(1 << 20));
    let mut offset: u64 = 0;
    let mut fingerprint: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            cur.push(byte);
            fingerprint = (fingerprint << 1).wrapping_add(GEAR[byte as usize]);
            let len = cur.len();
            let cut = if len < MIN_SIZE {
                false
            } else if len < AVG_SIZE {
                (fingerprint & MASK_STRICT) == 0
            } else if len < MAX_SIZE {
                (fingerprint & MASK_LOOSE) == 0
            } else {
                true // forced boundary at MAX_SIZE
            };
            if cut {
                out.push(Chunk {
                    offset,
                    length: len as u64,
                    chunk_hash: sha256_hex(&cur),
                });
                offset += len as u64;
                cur.clear();
                fingerprint = 0;
            }
        }
    }
    if !cur.is_empty() {
        out.push(Chunk {
            offset,
            length: cur.len() as u64,
            chunk_hash: sha256_hex(&cur),
        });
    }
    Ok(out)
}

/// In-memory convenience over [`chunk_reader`] (infallible for a slice).
pub fn chunk_bytes(data: &[u8]) -> Vec<Chunk> {
    // INVARIANT: reading from an in-memory `Cursor<&[u8]>` never returns an I/O
    // error, so `chunk_reader` cannot fail here.
    chunk_reader(io::Cursor::new(data)).expect("in-memory chunking cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_zero_chunks() {
        assert!(chunk_bytes(&[]).is_empty());
    }

    #[test]
    fn short_input_is_a_single_chunk() {
        let data = vec![7u8; MIN_SIZE - 1];
        let chunks = chunk_bytes(&data);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[0].length, (MIN_SIZE - 1) as u64);
        assert_eq!(chunks[0].chunk_hash, sha256_hex(&data));
    }

    #[test]
    fn deterministic_and_contiguous_and_covers_input() {
        // A pseudo-random-but-fixed input larger than MAX so multiple chunks fire.
        let mut data = Vec::with_capacity(MAX_SIZE * 3);
        let mut x: u64 = 0x1234_5678_9ABC_DEF0;
        while data.len() < MAX_SIZE * 3 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            data.push((x >> 33) as u8);
        }
        let a = chunk_bytes(&data);
        let b = chunk_bytes(&data);
        assert_eq!(a, b, "same bytes must produce byte-identical chunks");
        assert!(a.len() > 1, "a >MAX input must split into multiple chunks");
        // Contiguity + full coverage; each chunk within [.., MAX_SIZE].
        let mut expected_offset = 0u64;
        for c in &a {
            assert_eq!(c.offset, expected_offset, "chunks must be contiguous");
            assert!(c.length >= 1 && c.length <= MAX_SIZE as u64);
            assert_eq!(
                c.chunk_hash,
                sha256_hex(&data[c.offset as usize..(c.offset + c.length) as usize])
            );
            expected_offset += c.length;
        }
        assert_eq!(
            expected_offset as usize,
            data.len(),
            "chunks must cover input"
        );
    }

    #[test]
    fn streaming_and_in_memory_agree() {
        let data = vec![0xABu8; MAX_SIZE + 123];
        let streamed = chunk_reader(io::Cursor::new(&data[..])).unwrap();
        assert_eq!(streamed, chunk_bytes(&data));
    }
}
