//! Pure-Rust, bounded ZIP archive reader and writer for container formats (`.fide`).
//!
//! Stores or deflates entries according to PKWARE's APPNOTE.TXT specification,
//! validating CRC32, enforcing decompression caps, and rejecting traversal attacks.

use std::io::{Read as _, Write as _};

use crc32fast::Hasher;
use flate2::Compression;
use flate2::write::DeflateEncoder;

const DOS_DATE: u16 = (1 << 5) | 1;
const UTF8_NAMES: u16 = 1 << 11;
const ZIP_VERSION: u16 = 20;

/// Maximum number of files permitted in a `.fide` project archive.
pub const MAX_ARCHIVE_FILES: usize = 10_000;
/// Maximum uncompressed bytes permitted for a single archive entry (256 MB).
pub const MAX_ARCHIVE_ENTRY_BYTES: usize = 256 << 20;
/// Maximum total uncompressed bytes permitted across all archive entries (1 GB).
pub const MAX_TOTAL_EXPANSION_BYTES: usize = 1024 << 20;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ZipError {
    #[error("not a valid ZIP archive")]
    NotZip,
    #[error("archive contains too many files ({0} > {MAX_ARCHIVE_FILES})")]
    TooManyFiles(usize),
    #[error("ZIP64 archives are not supported")]
    Zip64NotSupported,
    #[error("encrypted entries are not supported in `{0}`")]
    Encrypted(String),
    #[error("compression method {1} in `{0}` is not supported")]
    UnsupportedMethod(String, u16),
    #[error("entry `{0}` exceeds the maximum allowable size ({1} bytes)")]
    TooLarge(String, usize),
    #[error("total uncompressed archive size exceeds safety limit")]
    TotalSizeExceeded,
    #[error("entry path `{0}` is unsafe or contains traversal components")]
    UnsafePath(String),
    #[error("archive entry `{0}` is damaged or failed CRC verification")]
    Damaged(String),
    #[error("failed to create archive: {0}")]
    Create(String),
}

/// Checks whether `path` is a safe, canonical relative subpath.
/// Rejects absolute paths, drive letters, backslashes, `..` segments, and empty components.
pub fn is_safe_entry_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    if path.contains('\\') || path.contains(':') || path.contains('\0') {
        return false;
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return false;
        }
    }
    true
}

/// An entry to be written into a ZIP archive.
pub struct ZipFileEntry<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub deflate: bool,
}

/// Creates a standard ZIP archive from entries.
pub fn create_zip(entries: &[ZipFileEntry<'_>]) -> Result<Vec<u8>, ZipError> {
    if entries.len() > MAX_ARCHIVE_FILES {
        return Err(ZipError::TooManyFiles(entries.len()));
    }
    let count = u16::try_from(entries.len()).map_err(|_| ZipError::Create("too many files for standard ZIP".into()))?;

    let mut out = Vec::new();
    let mut central = Vec::new();

    for entry in entries {
        if !is_safe_entry_path(entry.name) {
            return Err(ZipError::UnsafePath(entry.name.to_string()));
        }
        let name_bytes = entry.name.as_bytes();
        let name_len = u16::try_from(name_bytes.len()).map_err(|_| ZipError::Create(format!("entry name too long: {}", entry.name)))?;

        let mut hasher = Hasher::new();
        hasher.update(entry.data);
        let crc = hasher.finalize();

        let uncompressed_len = u32::try_from(entry.data.len()).map_err(|_| ZipError::TooLarge(entry.name.to_string(), entry.data.len()))?;

        let (method, payload) = if entry.deflate && !entry.data.is_empty() {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(entry.data).map_err(|e| ZipError::Create(format!("deflate write failed: {e}")))?;
            let compressed = encoder.finish().map_err(|e| ZipError::Create(format!("deflate finish failed: {e}")))?;
            (8u16, compressed)
        } else {
            (0u16, entry.data.to_vec())
        };

        let compressed_len = u32::try_from(payload.len()).map_err(|_| ZipError::Create("compressed data exceeds 4 GB".into()))?;

        let offset = u32::try_from(out.len()).map_err(|_| ZipError::Create("archive exceeds standard 4 GB limit".into()))?;

        // Shared header fields: flags, method, time, date, CRC, sizes, name length.
        let mut shared = Vec::with_capacity(26);
        for v in [UTF8_NAMES, method, 0, DOS_DATE] {
            shared.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, compressed_len, uncompressed_len] {
            shared.extend_from_slice(&v.to_le_bytes());
        }
        shared.extend_from_slice(&name_len.to_le_bytes());

        // Local header: signature (0x04034b50), version, shared, extra field length (0), name, payload.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        out.extend_from_slice(&shared);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&payload);

        // Central directory entry: signature (0x02014b50), versions, shared, extra/comment, attrs, offset, name.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        central.extend_from_slice(&ZIP_VERSION.to_le_bytes());
        central.extend_from_slice(&shared);
        central.extend_from_slice(&[0u8; 2 + 2 + 2 + 2 + 4]); // extra len, comment len, disk, internal/external attr
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let central_start = u32::try_from(out.len()).map_err(|_| ZipError::Create("archive exceeds 4 GB".into()))?;
    let central_len = u32::try_from(central.len()).map_err(|_| ZipError::Create("central directory exceeds 4 GB".into()))?;
    out.extend_from_slice(&central);

    // End of central directory record.
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]); // disk numbers
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_len.to_le_bytes());
    out.extend_from_slice(&central_start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length

    Ok(out)
}

struct ArchiveEntry {
    name: String,
    flags: u16,
    method: u16,
    crc: u32,
    packed: usize,
    size: usize,
    local: usize,
}

/// A parsed ZIP archive for safe random-access reading of entries.
pub struct ZipArchive<'a> {
    data: &'a [u8],
    entries: Vec<ArchiveEntry>,
}

impl<'a> ZipArchive<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, ZipError> {
        let damaged = || ZipError::Damaged(String::new());
        let u16_at = |at: usize| at.checked_add(2).and_then(|end| data.get(at..end)).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes);
        let u32_at = |at: usize| at.checked_add(4).and_then(|end| data.get(at..end)).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);

        let last = data.len().checked_sub(22).ok_or(ZipError::NotZip)?;
        let candidates = || (last.saturating_sub(0xFFFF)..=last).rev().filter(|&at| u32_at(at) == Some(0x0605_4b50));
        let end =
            candidates().find(|&at| u16_at(at + 20).map(usize::from) == Some(last - at)).or_else(|| candidates().next()).ok_or(ZipError::NotZip)?;

        let count = u16_at(end + 10).ok_or_else(damaged)?;
        let start = u32_at(end + 16).ok_or_else(damaged)?;
        if count == 0xFFFF || start == 0xFFFF_FFFF || u32_at(end + 12) == Some(0xFFFF_FFFF) {
            return Err(ZipError::Zip64NotSupported);
        }
        let count = usize::from(count);
        if count > MAX_ARCHIVE_FILES {
            return Err(ZipError::TooManyFiles(count));
        }

        let mut at = start as usize;
        let mut entries = Vec::with_capacity(count);

        for _ in 0..count {
            if u32_at(at) != Some(0x0201_4b50) {
                return Err(damaged());
            }
            let short = |off: usize| at.checked_add(off).and_then(u16_at).ok_or_else(damaged);
            let long = |off: usize| at.checked_add(off).and_then(u32_at).ok_or_else(damaged);

            let name_len = usize::from(short(28)?);
            let extra = usize::from(short(30)?);
            let comment = usize::from(short(32)?);

            let name_at = at.checked_add(46).ok_or_else(damaged)?;
            let name_bytes = name_at.checked_add(name_len).and_then(|end| data.get(name_at..end)).ok_or_else(damaged)?;
            let name = String::from_utf8_lossy(name_bytes).into_owned();

            if !is_safe_entry_path(&name) {
                return Err(ZipError::UnsafePath(name));
            }

            entries.push(ArchiveEntry {
                name,
                flags: short(8)?,
                method: short(10)?,
                crc: long(16)?,
                packed: long(20)? as usize,
                size: long(24)? as usize,
                local: long(42)? as usize,
            });

            at = name_at.checked_add(name_len + extra + comment).ok_or_else(damaged)?;
        }

        Ok(Self { data, entries })
    }

    pub fn entry_names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name.eq_ignore_ascii_case(name))
    }

    pub fn read(&self, name: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, ZipError> {
        let Some(e) = self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)) else {
            return Ok(None);
        };
        let damaged = || ZipError::Damaged(e.name.clone());

        if e.flags & 1 != 0 {
            return Err(ZipError::Encrypted(e.name.clone()));
        }
        if e.size > max_bytes {
            return Err(ZipError::TooLarge(e.name.clone(), e.size));
        }

        let header = e.local.checked_add(30).and_then(|end| self.data.get(e.local..end)).ok_or_else(damaged)?;
        let (Some(sig), Some(name_len_raw), Some(extra_raw)) = (header.get(..4), header.get(26..28), header.get(28..30)) else {
            return Err(damaged());
        };
        if sig != 0x0403_4b50u32.to_le_bytes() {
            return Err(damaged());
        }

        let len_from = |b: &[u8]| b.try_into().map(|arr| usize::from(u16::from_le_bytes(arr))).map_err(|_| damaged());
        let name_len = len_from(name_len_raw)?;
        let extra_len = len_from(extra_raw)?;

        let start = e.local.checked_add(30 + name_len + extra_len).ok_or_else(damaged)?;
        let payload = start.checked_add(e.packed).and_then(|end| self.data.get(start..end)).ok_or_else(damaged)?;

        let bytes = match e.method {
            0 => {
                if e.packed != e.size {
                    return Err(damaged());
                }
                payload.to_vec()
            }
            8 => {
                let mut out = Vec::with_capacity(e.size);
                flate2::read::DeflateDecoder::new(payload).take((max_bytes as u64).saturating_add(1)).read_to_end(&mut out).map_err(|_| damaged())?;
                if out.len() > max_bytes {
                    return Err(ZipError::TooLarge(e.name.clone(), out.len()));
                }
                out
            }
            m => return Err(ZipError::UnsupportedMethod(e.name.clone(), m)),
        };

        let mut hasher = Hasher::new();
        hasher.update(&bytes);
        if bytes.len() != e.size || hasher.finalize() != e.crc {
            return Err(damaged());
        }

        Ok(Some(bytes))
    }
}
