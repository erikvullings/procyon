//! macOS batched directory listing with `getattrlistbulk(2)`.
//!
//! One call returns a batch of entries together with their type, device, BSD flags, file ID,
//! link count and sizes, replacing a `readdir` plus one `lstat` per entry. The approach follows
//! BlitzTree's scanner (MIT, <https://github.com/ahmedkhaleel2004/blitztree>).

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// `ATTR_CMN_ERROR` is missing from `libc`; it reports a per-entry failure.
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const VDIR: u32 = 2;
const VLNK: u32 = 5;
/// `attribute_set_t`: five `attrgroup_t` words (common, volume, directory, file, fork).
const ATTRIBUTE_SET_LEN: usize = 20;
/// Kernel buffer per call, reused per thread; large directories take several calls.
const BUFFER_WORDS: usize = 16 * 1024;

const COMMON_ATTRIBUTES: u32 = libc::ATTR_CMN_RETURNED_ATTRS
    | ATTR_CMN_ERROR
    | libc::ATTR_CMN_NAME
    | libc::ATTR_CMN_DEVID
    | libc::ATTR_CMN_OBJTYPE
    | libc::ATTR_CMN_FLAGS
    | libc::ATTR_CMN_FILEID;
const DIRECTORY_ATTRIBUTES: u32 =
    libc::ATTR_DIR_MOUNTSTATUS | libc::ATTR_DIR_ALLOCSIZE | libc::ATTR_DIR_DATALENGTH;
const FILE_ATTRIBUTES: u32 =
    libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_TOTALSIZE | libc::ATTR_FILE_ALLOCSIZE;

thread_local! {
    // `u64` words keep the kernel buffer 8-byte aligned.
    static BUFFER: RefCell<Vec<u64>> = RefCell::new(vec![0; BUFFER_WORDS]);
}

/// Object type reported for one listed entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkEntryKind {
    /// A directory.
    Directory,
    /// A symbolic link (never followed).
    Symlink,
    /// A regular file or any other non-directory object.
    Other,
}

/// Attributes of one listed entry, matching what `lstat` would report for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkAttributes {
    /// Object type.
    pub kind: BulkEntryKind,
    /// Device ID, as `st_dev` widened to `u64`.
    pub device: u64,
    /// File ID within the device (`st_ino` on APFS).
    pub file_id: u64,
    /// BSD flags (`st_flags`), e.g. `SF_DATALESS`.
    pub bsd_flags: u32,
    /// Hard-link count; always `1` for directories.
    pub link_count: u32,
    /// Apparent size in bytes (`st_size`).
    pub logical_bytes: u64,
    /// Allocated size in bytes (`st_blocks * 512`).
    pub physical_bytes: u64,
    /// Whether a filesystem is mounted on this directory; its attributes then describe the
    /// covered directory rather than the mounted volume's root.
    pub mount_point: bool,
}

/// One entry of a bulk listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkEntry {
    /// Entry name (not a path).
    pub name: OsString,
    /// Attributes, or `None` when the kernel reported an error or omitted a required attribute
    /// for this entry, in which case the caller should `lstat` it.
    pub attributes: Option<BulkAttributes>,
}

/// The entries of one directory, in kernel order.
#[derive(Debug, Default)]
pub struct BulkListing {
    /// Entries that could be named.
    pub entries: Vec<BulkEntry>,
    /// Failures that could not be attributed to a named entry, including a listing that
    /// stopped part-way through.
    pub failures: Vec<io::ErrorKind>,
}

/// Lists a directory with `getattrlistbulk`, without following a symlink at `path`.
///
/// # Errors
///
/// Returns the error from opening the directory; later failures are reported in
/// [`BulkListing::failures`] alongside the entries read so far.
pub fn list_directory_bulk(path: &Path) -> io::Result<BulkListing> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    let mut request = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: COMMON_ATTRIBUTES,
        volattr: 0,
        dirattr: DIRECTORY_ATTRIBUTES,
        fileattr: FILE_ATTRIBUTES,
        forkattr: 0,
    };
    let mut listing = BulkListing::default();
    BUFFER.with_borrow_mut(|buffer| {
        loop {
            let byte_len = buffer.len() * std::mem::size_of::<u64>();
            #[allow(unsafe_code)]
            // SAFETY: `directory` is an open directory descriptor for the duration of the call,
            // `request` is a valid `attrlist`, and `buffer` is a live, writable allocation of
            // `byte_len` bytes that the kernel fills without retaining the pointer.
            let count = unsafe {
                libc::getattrlistbulk(
                    directory.as_raw_fd(),
                    (&raw mut request).cast(),
                    buffer.as_mut_ptr().cast(),
                    byte_len,
                    0,
                )
            };
            if count == 0 {
                break;
            }
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                listing.failures.push(error.kind());
                break;
            }
            let bytes = words_as_bytes(buffer);
            if parse_batch(bytes, count.unsigned_abs() as usize, &mut listing).is_none() {
                listing.failures.push(io::ErrorKind::InvalidData);
                break;
            }
        }
    });
    Ok(listing)
}

fn words_as_bytes(words: &[u64]) -> &[u8] {
    #[allow(unsafe_code)]
    // SAFETY: any initialized `u64` slice is a valid byte slice of eight times the length, `u8`
    // has no alignment requirement, and the borrow ties the bytes' lifetime to `words`.
    unsafe {
        std::slice::from_raw_parts(words.as_ptr().cast(), std::mem::size_of_val(words))
    }
}

/// Parses `count` packed entries, or `None` when the buffer is malformed.
fn parse_batch(bytes: &[u8], count: usize, listing: &mut BulkListing) -> Option<()> {
    let mut offset = 0;
    for _ in 0..count {
        let length = usize::try_from(read_u32(bytes, offset)?).ok()?;
        let entry = bytes.get(offset..offset.checked_add(length)?)?;
        match parse_entry(entry)? {
            ParsedEntry::Named(entry) => listing.entries.push(entry),
            ParsedEntry::Unnamed(kind) => listing.failures.push(kind),
        }
        offset += length;
    }
    Some(())
}

enum ParsedEntry {
    Named(BulkEntry),
    Unnamed(io::ErrorKind),
}

/// Attributes appear in a fixed order: the returned set, the error, the common attributes by
/// bit, then directory attributes (directories only) and file attributes (non-directories
/// only). Each is present only when its bit is set in the returned set.
fn parse_entry(entry: &[u8]) -> Option<ParsedEntry> {
    let mut offset = 4;
    let common = read_u32(entry, offset)?;
    let directory = read_u32(entry, offset + 8)?;
    let file = read_u32(entry, offset + 12)?;
    offset += ATTRIBUTE_SET_LEN;

    let mut error = 0;
    if common & ATTR_CMN_ERROR != 0 {
        error = read_u32(entry, offset)?;
        offset += 4;
    }
    let mut name = None;
    if common & libc::ATTR_CMN_NAME != 0 {
        let relative = i32::from_ne_bytes(read_array(entry, offset)?);
        let length = usize::try_from(read_u32(entry, offset + 4)?).ok()?;
        let start = offset.checked_add_signed(isize::try_from(relative).ok()?)?;
        let raw = entry.get(start..start.checked_add(length)?)?;
        // The length includes the trailing NUL.
        let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
        if !raw.is_empty() {
            name = Some(OsString::from_vec(raw.to_vec()));
        }
        offset += 8;
    }
    let Some(name) = name else {
        return Some(ParsedEntry::Unnamed(error_kind(error)));
    };
    if error != 0 {
        return Some(ParsedEntry::Named(BulkEntry {
            name,
            attributes: None,
        }));
    }

    let required = libc::ATTR_CMN_DEVID
        | libc::ATTR_CMN_OBJTYPE
        | libc::ATTR_CMN_FLAGS
        | libc::ATTR_CMN_FILEID;
    let mut device = 0;
    if common & libc::ATTR_CMN_DEVID != 0 {
        // `dev_t` is an `i32`; widen it the way `std`'s `MetadataExt::dev` does.
        device = i64::from(i32::from_ne_bytes(read_array(entry, offset)?)) as u64;
        offset += 4;
    }
    let mut object_type = 0;
    if common & libc::ATTR_CMN_OBJTYPE != 0 {
        object_type = read_u32(entry, offset)?;
        offset += 4;
    }
    let mut bsd_flags = 0;
    if common & libc::ATTR_CMN_FLAGS != 0 {
        bsd_flags = read_u32(entry, offset)?;
        offset += 4;
    }
    let mut file_id = 0;
    if common & libc::ATTR_CMN_FILEID != 0 {
        file_id = u64::from_ne_bytes(read_array(entry, offset)?);
        offset += 8;
    }
    let complete_common = common & required == required;

    let kind = match object_type {
        VDIR => BulkEntryKind::Directory,
        VLNK => BulkEntryKind::Symlink,
        _ => BulkEntryKind::Other,
    };
    let mut mount_point = false;
    let mut link_count = 1;
    let mut logical_bytes = 0;
    let mut physical_bytes = 0;
    let complete_sizes = if kind == BulkEntryKind::Directory {
        if directory & libc::ATTR_DIR_MOUNTSTATUS != 0 {
            mount_point = read_u32(entry, offset)? & libc::DIR_MNTSTATUS_MNTPOINT != 0;
            offset += 4;
        }
        if directory & libc::ATTR_DIR_ALLOCSIZE != 0 {
            physical_bytes = read_off_t(entry, offset)?;
            offset += 8;
        }
        if directory & libc::ATTR_DIR_DATALENGTH != 0 {
            logical_bytes = read_off_t(entry, offset)?;
        }
        directory & DIRECTORY_ATTRIBUTES == DIRECTORY_ATTRIBUTES
    } else {
        if file & libc::ATTR_FILE_LINKCOUNT != 0 {
            link_count = read_u32(entry, offset)?;
            offset += 4;
        }
        if file & libc::ATTR_FILE_TOTALSIZE != 0 {
            logical_bytes = read_off_t(entry, offset)?;
            offset += 8;
        }
        if file & libc::ATTR_FILE_ALLOCSIZE != 0 {
            physical_bytes = read_off_t(entry, offset)?;
        }
        file & FILE_ATTRIBUTES == FILE_ATTRIBUTES
    };

    Some(ParsedEntry::Named(BulkEntry {
        name,
        attributes: (complete_common && complete_sizes).then_some(BulkAttributes {
            kind,
            device,
            file_id,
            bsd_flags,
            link_count,
            logical_bytes,
            physical_bytes,
            mount_point,
        }),
    }))
}

fn error_kind(errno: u32) -> io::ErrorKind {
    i32::try_from(errno)
        .map(|errno| io::Error::from_raw_os_error(errno).kind())
        .unwrap_or(io::ErrorKind::Other)
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    read_array(bytes, offset).map(u32::from_ne_bytes)
}

/// Reads a non-negative `off_t`.
fn read_off_t(bytes: &[u8], offset: usize) -> Option<u64> {
    read_array(bytes, offset).map(|raw| u64::try_from(i64::from_ne_bytes(raw)).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packs one entry the way the kernel does, for the given returned attribute groups.
    fn packed_entry(
        common: u32,
        directory: u32,
        file: u32,
        name: &[u8],
        tail: &[&[u8]],
    ) -> Vec<u8> {
        let mut fixed = Vec::new();
        for group in [common, 0, directory, file, 0] {
            fixed.extend_from_slice(&group.to_ne_bytes());
        }
        let name_reference_at = fixed.len();
        fixed.extend_from_slice(&[0; 8]);
        for part in tail {
            fixed.extend_from_slice(part);
        }
        // The reference offset is relative to the reference itself (after the length word).
        let name_offset = i32::try_from(fixed.len() - name_reference_at).expect("small offset");
        fixed[name_reference_at..name_reference_at + 4].copy_from_slice(&name_offset.to_ne_bytes());
        let name_length = u32::try_from(name.len() + 1).expect("short name");
        fixed[name_reference_at + 4..name_reference_at + 8]
            .copy_from_slice(&name_length.to_ne_bytes());
        fixed.extend_from_slice(name);
        fixed.push(0);
        let mut entry = u32::try_from(fixed.len() + 4)
            .expect("small entry")
            .to_ne_bytes()
            .to_vec();
        entry.extend_from_slice(&fixed);
        entry
    }

    fn common_parts(device: i32, object_type: u32, flags: u32, file_id: u64) -> Vec<Vec<u8>> {
        vec![
            device.to_ne_bytes().to_vec(),
            object_type.to_ne_bytes().to_vec(),
            flags.to_ne_bytes().to_vec(),
            file_id.to_ne_bytes().to_vec(),
        ]
    }

    const ALL_COMMON: u32 = libc::ATTR_CMN_RETURNED_ATTRS
        | libc::ATTR_CMN_NAME
        | libc::ATTR_CMN_DEVID
        | libc::ATTR_CMN_OBJTYPE
        | libc::ATTR_CMN_FLAGS
        | libc::ATTR_CMN_FILEID;

    #[test]
    fn parses_files_and_directories_from_one_batch() {
        let mut file_tail = common_parts(7, 1, 0, 42);
        file_tail.push(3_u32.to_ne_bytes().to_vec());
        file_tail.push(13_i64.to_ne_bytes().to_vec());
        file_tail.push(4096_i64.to_ne_bytes().to_vec());
        let file_tail = file_tail.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let mut directory_tail = common_parts(7, VDIR, 0x4000_0000, 43);
        directory_tail.push(libc::DIR_MNTSTATUS_MNTPOINT.to_ne_bytes().to_vec());
        directory_tail.push(0_i64.to_ne_bytes().to_vec());
        directory_tail.push(96_i64.to_ne_bytes().to_vec());
        let directory_tail = directory_tail.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let mut batch = packed_entry(ALL_COMMON, 0, FILE_ATTRIBUTES, b"file.bin", &file_tail);
        batch.extend(packed_entry(
            ALL_COMMON,
            DIRECTORY_ATTRIBUTES,
            0,
            "Документы".as_bytes(),
            &directory_tail,
        ));

        let mut listing = BulkListing::default();
        parse_batch(&batch, 2, &mut listing).expect("well-formed batch");

        assert!(listing.failures.is_empty());
        assert_eq!(
            listing.entries,
            [
                BulkEntry {
                    name: OsString::from("file.bin"),
                    attributes: Some(BulkAttributes {
                        kind: BulkEntryKind::Other,
                        device: 7,
                        file_id: 42,
                        bsd_flags: 0,
                        link_count: 3,
                        logical_bytes: 13,
                        physical_bytes: 4096,
                        mount_point: false,
                    }),
                },
                BulkEntry {
                    name: OsString::from("Документы"),
                    attributes: Some(BulkAttributes {
                        kind: BulkEntryKind::Directory,
                        device: 7,
                        file_id: 43,
                        bsd_flags: 0x4000_0000,
                        link_count: 1,
                        logical_bytes: 96,
                        physical_bytes: 0,
                        mount_point: true,
                    }),
                },
            ]
        );
    }

    #[test]
    fn entry_errors_and_missing_attributes_defer_to_lstat() {
        let errored = packed_entry(
            libc::ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_ERROR | libc::ATTR_CMN_NAME,
            0,
            0,
            b"locked",
            &[],
        );
        // The error word precedes the name reference, so splice it in after the returned set.
        let mut errored_with_code = errored[..24].to_vec();
        errored_with_code.extend_from_slice(&(libc::EACCES as u32).to_ne_bytes());
        errored_with_code.extend_from_slice(&errored[24..]);
        let length = u32::try_from(errored_with_code.len()).expect("small entry");
        errored_with_code[..4].copy_from_slice(&length.to_ne_bytes());
        let partial_tail = common_parts(7, 1, 0, 44);
        let partial_tail = partial_tail.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let partial = packed_entry(ALL_COMMON, 0, 0, b"partial", &partial_tail);
        let mut batch = errored_with_code;
        batch.extend(partial);

        let mut listing = BulkListing::default();
        parse_batch(&batch, 2, &mut listing).expect("well-formed batch");

        assert_eq!(
            listing
                .entries
                .iter()
                .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.attributes))
                .collect::<Vec<_>>(),
            [("locked".to_owned(), None), ("partial".to_owned(), None)]
        );
    }

    #[test]
    fn truncated_batches_are_rejected() {
        let tail = common_parts(7, 1, 0, 45);
        let tail = tail.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let entry = packed_entry(ALL_COMMON, 0, 0, b"cut", &tail);

        let mut listing = BulkListing::default();
        assert!(parse_batch(&entry[..entry.len() - 3], 1, &mut listing).is_none());
        assert!(parse_batch(&entry, 2, &mut listing).is_none());
    }

    #[test]
    fn lists_a_real_directory_like_lstat() {
        use std::os::unix::fs::MetadataExt;

        let root = tempfile::tempdir().expect("temp dir");
        std::fs::write(root.path().join("a.bin"), [1_u8; 5000]).expect("write file");
        std::fs::create_dir(root.path().join("nested")).expect("create dir");
        std::os::unix::fs::symlink("a.bin", root.path().join("link")).expect("symlink");
        std::fs::hard_link(root.path().join("a.bin"), root.path().join("b.bin")).expect("hardlink");

        let mut listing = list_directory_bulk(root.path()).expect("list temp dir");
        listing
            .entries
            .sort_by(|left, right| left.name.cmp(&right.name));

        assert!(listing.failures.is_empty());
        let names = listing
            .entries
            .iter()
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, ["a.bin", "b.bin", "link", "nested"]);
        for entry in &listing.entries {
            let metadata =
                std::fs::symlink_metadata(root.path().join(&entry.name)).expect("lstat entry");
            let attributes = entry.attributes.expect("attributes for a readable entry");
            assert_eq!(attributes.device, metadata.dev(), "{names:?}");
            assert_eq!(attributes.file_id, metadata.ino());
            assert_eq!(attributes.bsd_flags, metadata.st_flags_compat());
            assert_eq!(attributes.logical_bytes, metadata.len());
            assert_eq!(attributes.physical_bytes, metadata.blocks() * 512);
            if !metadata.is_dir() {
                assert_eq!(u64::from(attributes.link_count), metadata.nlink());
            }
            assert_eq!(
                attributes.kind,
                if metadata.is_dir() {
                    BulkEntryKind::Directory
                } else if metadata.file_type().is_symlink() {
                    BulkEntryKind::Symlink
                } else {
                    BulkEntryKind::Other
                }
            );
        }
    }

    trait FlagsCompat {
        fn st_flags_compat(&self) -> u32;
    }

    impl FlagsCompat for std::fs::Metadata {
        fn st_flags_compat(&self) -> u32 {
            std::os::macos::fs::MetadataExt::st_flags(self)
        }
    }
}
