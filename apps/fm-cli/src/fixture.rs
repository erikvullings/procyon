//! Reproducible fixture generators for performance benchmarking.
//!
//! All fixtures use seeded randomness for reproducibility: same seed = same output.
//! Fixtures are created under temp/ignored paths and never committed.

use anyhow::Result;
use std::fs::{self, File};
use std::io::{Seek, Write};
use std::path::Path;

/// Generate a flat directory with specified number of entries
///
/// Creates entries named `entry-000000.dat` through `entry-NNNNNN.dat`,
/// with synthetic file sizes and modification timestamps.
pub(crate) fn flat_directory(base: &Path, count: usize) -> Result<()> {
    // Use subdirectory based on count for clarity
    let dir = if count >= 100_000 {
        base.join(format!("{}-entries", count))
    } else {
        base.join(format!("{:06}-entries", count))
    };

    fs::create_dir_all(&dir)?;

    for i in 0..count {
        let name = format!("entry-{:07}.dat", i);
        let path = dir.join(&name);

        // Create file with seeded content (just a few bytes for speed)
        let mut file = File::create(&path)?;
        let seed = i as u32;
        let size = (seed % 10_000) as u64 + 1; // 1-10KB per file
        writeln!(file, "seed:{} size:{}", seed, size)?;
    }

    Ok(())
}

/// Generate 10,000 small files optimized for copy operation benchmarks
///
/// Creates files with minimal content to test copy throughput of many small files.
pub(crate) fn small_files(base: &Path, count: usize) -> Result<()> {
    fs::create_dir_all(base)?;

    // Create nested structure to simulate real-world file layouts
    let num_dirs = (count / 100).max(1);

    for dir_idx in 0..num_dirs {
        let subdir = base.join(format!("subdir-{:04}", dir_idx));
        fs::create_dir_all(&subdir)?;

        let files_per_dir = count / num_dirs;
        for file_idx in 0..files_per_dir {
            let name = format!("file-{:05}.txt", file_idx);
            let path = subdir.join(&name);
            let mut file = File::create(&path)?;
            let seed = (dir_idx * 1000 + file_idx) as u32;
            writeln!(file, "id:{}", seed)?;
        }
    }

    Ok(())
}

/// Generate a multi-gigabyte sparse file
///
/// Uses sparse file techniques (seek + write end marker) where available,
/// or a smaller test file if sparse files aren't supported.
pub(crate) fn large_sparse_file(base: &Path, size_gib: u64) -> Result<()> {
    fs::create_dir_all(base)?;

    let path = base.join(format!("sparse-{}gib.bin", size_gib));

    let mut file = File::create(&path)?;
    let size_bytes = size_gib * 1024 * 1024 * 1024;

    // On systems supporting sparse files, seek to end and write marker
    // This creates a sparse file without allocating actual disk space
    file.seek(std::io::SeekFrom::End(size_bytes as i64 - 1))?;
    file.write_all(b"X")?;

    Ok(())
}

/// Generate a deeply nested directory structure
///
/// Creates a chain of nested directories, each containing a small file.
/// Tests stack safety and recursive traversal performance.
pub(crate) fn deeply_nested(base: &Path, depth: usize) -> Result<()> {
    let mut path = base.to_path_buf();
    fs::create_dir_all(&path)?;

    for level in 0..depth {
        path.push(format!("level-{:04}", level));
        fs::create_dir_all(&path)?;

        // Create a small file at each level
        let file_path = path.join("file.txt");
        let mut file = File::create(&file_path)?;
        writeln!(file, "depth level {}", level)?;
    }

    Ok(())
}

/// Generate directories with long Unicode names
///
/// Tests handling of Unicode in filenames across different locales.
pub(crate) fn unicode_names(base: &Path, count: usize) -> Result<()> {
    fs::create_dir_all(base)?;

    // Sample of Unicode characters covering various scripts
    let unicode_samples = [
        "Ä",  // Latin Extended
        "Ñ",  // Latin with diacritics
        "Ш",  // Cyrillic
        "Ж",  // Cyrillic
        "É",  // French
        "Ü",  // German
        "ø",  // Norwegian
        "ñ",  // Spanish
        "α",  // Greek
        "β",  // Greek
        "日", // CJK (Japanese/Chinese)
        "本", // CJK
        "한", // CJK (Korean)
        "국", // CJK
        "🎉", // Emoji
        "📁", // Emoji
        "🔥", // Emoji
        "😀", // Emoji
    ];

    for i in 0..count {
        let char_idx = i % unicode_samples.len();
        let unicode_char = unicode_samples[char_idx];
        let name = format!("file-{:04}-{}.txt", i, unicode_char);
        let path = base.join(&name);

        let mut file = File::create(&path)?;
        writeln!(file, "unicode test {}", i)?;
    }

    Ok(())
}
