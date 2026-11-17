//! The OpenSubtitles file hash (feature 050): the file size plus the 64-bit little-endian sums
//! of the first and last 64 KiB, wrapping, as 16 lowercase hex digits.
//!
//! It identifies the exact file, so a match means the timing fits. Two small reads, no
//! dependency, and it works after Put.io has deleted the file or for a local video.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const BLOCK: u64 = 64 * 1024;

/// Files smaller than this get no hash (the service ignores them too)
pub const MIN_BYTES: u64 = 2 * BLOCK;

/// The hash of `path`, or `None` for a file under 128 KiB
pub fn opensubtitles_hash(path: &Path) -> std::io::Result<Option<String>> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size < MIN_BYTES {
        return Ok(None);
    }
    let mut hash = size;
    let mut buf = vec![0u8; BLOCK as usize];
    for start in [0, size - BLOCK] {
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut buf)?;
        let (words, _) = buf.as_chunks::<8>();
        hash = words
            .iter()
            .fold(hash, |h, w| h.wrapping_add(u64::from_le_bytes(*w)));
    }
    Ok(Some(format!("{hash:016x}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(data: &[u8]) -> Option<String> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("v.mkv");
        std::fs::write(&p, data).unwrap();
        opensubtitles_hash(&p).unwrap()
    }

    // The expected values were computed with an independent script (struct.unpack over the same
    // blocks). The published breakdance.avi sample needs the 12 MB file, which is not kept here.

    #[test]
    fn zeros_hash_to_the_size() {
        assert_eq!(
            hash_of(&vec![0u8; 200_000]).as_deref(),
            Some("0000000000030d40")
        );
    }

    #[test]
    fn a_patterned_file_gives_the_known_answer() {
        let data: Vec<u8> = (0..200_003u32).map(|i| ((i * 7 + 3) % 251) as u8).collect();
        assert_eq!(hash_of(&data).as_deref(), Some("4752758491abaeb9"));
    }

    #[test]
    fn the_sum_wraps_instead_of_overflowing() {
        assert_eq!(
            hash_of(&vec![0xff; 131_072]).as_deref(),
            Some("000000000001c000")
        );
    }

    #[test]
    fn a_size_that_is_not_a_multiple_of_eight_is_fine() {
        assert_eq!(
            hash_of(&vec![0xff; 140_001]).as_deref(),
            Some("000000000001e2e1")
        );
    }

    #[test]
    fn small_files_have_no_hash() {
        assert_eq!(hash_of(&vec![1u8; 131_071]), None);
        assert_eq!(hash_of(b""), None);
        assert!(hash_of(&vec![1u8; 131_072]).is_some());
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(opensubtitles_hash(Path::new("/no/such/file.mkv")).is_err());
    }
}
