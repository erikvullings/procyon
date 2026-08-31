//! Command line host for the file manager engine.
//!
//! Used for diagnostics and for generating the benchmark fixtures described in
//! task 0065. Like the Axum host it is a thin adapter over `fm-application`.

mod fixture;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fm-cli")]
#[command(about = "File manager CLI for diagnostics and fixture generation")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate performance benchmark fixtures
    Fixture {
        #[command(subcommand)]
        command: FixtureCommand,
    },
}

#[derive(Subcommand)]
enum FixtureCommand {
    /// Generate a flat directory with specified number of entries
    FlatDirectory {
        /// Number of entries (1000, 10000, 100000, or 1000000)
        #[arg(value_parser = ["1000", "10000", "100000", "1000000"])]
        count: String,

        /// Target directory (created if missing)
        #[arg(short, long)]
        target: Option<PathBuf>,
    },

    /// Generate 10,000 small files for copy testing
    SmallFiles {
        /// Target directory (created if missing)
        #[arg(short, long)]
        target: Option<PathBuf>,
    },

    /// Generate a multi-gigabyte sparse file
    LargeFile {
        /// Size in GiB (default: 1)
        #[arg(short, long, default_value = "1")]
        size_gib: u64,

        /// Target directory (created if missing)
        #[arg(short, long)]
        target: Option<PathBuf>,
    },

    /// Generate deeply nested directory structure
    DeeplyNested {
        /// Nesting depth (default: 100)
        #[arg(short, long, default_value = "100")]
        depth: usize,

        /// Target directory (created if missing)
        #[arg(short, long)]
        target: Option<PathBuf>,
    },

    /// Generate directories with long Unicode names
    UnicodeNames {
        /// Number of entries (default: 100)
        #[arg(short, long, default_value = "100")]
        count: usize,

        /// Target directory (created if missing)
        #[arg(short, long)]
        target: Option<PathBuf>,
    },

    /// Generate all fixtures (convenience command)
    All {
        /// Target base directory (created if missing)
        #[arg(short, long)]
        target: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Fixture { command } => match command {
            FixtureCommand::FlatDirectory { count, target } => {
                let count: usize = count.parse()?;
                let base = target.unwrap_or_else(|| "fixtures/benchmark".into());
                fixture::flat_directory(&base, count)?;
                println!("Generated {} flat entries in {}", count, base.display());
            }
            FixtureCommand::SmallFiles { target } => {
                let base = target.unwrap_or_else(|| "fixtures/benchmark/small-files".into());
                fixture::small_files(&base, 10_000)?;
                println!("Generated 10,000 small files in {}", base.display());
            }
            FixtureCommand::LargeFile { size_gib, target } => {
                let base = target.unwrap_or_else(|| "fixtures/benchmark".into());
                fixture::large_sparse_file(&base, size_gib)?;
                println!(
                    "Generated sparse file ({}GiB) in {}",
                    size_gib,
                    base.display()
                );
            }
            FixtureCommand::DeeplyNested { depth, target } => {
                let base = target.unwrap_or_else(|| "fixtures/benchmark/nested".into());
                fixture::deeply_nested(&base, depth)?;
                println!(
                    "Generated deeply nested structure ({} levels) in {}",
                    depth,
                    base.display()
                );
            }
            FixtureCommand::UnicodeNames { count, target } => {
                let base = target.unwrap_or_else(|| "fixtures/benchmark/unicode".into());
                fixture::unicode_names(&base, count)?;
                println!(
                    "Generated {} unicode-named entries in {}",
                    count,
                    base.display()
                );
            }
            FixtureCommand::All { target } => {
                let base = target.unwrap_or_else(|| "fixtures/benchmark".into());
                println!("Generating all fixtures in {}...", base.display());

                fixture::flat_directory(&base, 1_000)?;
                println!("  ✓ 1,000 flat entries");

                fixture::flat_directory(&base, 10_000)?;
                println!("  ✓ 10,000 flat entries");

                fixture::flat_directory(&base, 100_000)?;
                println!("  ✓ 100,000 flat entries");

                let small_files_dir = base.join("small-files");
                fixture::small_files(&small_files_dir, 10_000)?;
                println!("  ✓ 10,000 small files");

                fixture::large_sparse_file(&base, 1)?;
                println!("  ✓ 1 GiB sparse file");

                let nested_dir = base.join("nested");
                fixture::deeply_nested(&nested_dir, 100)?;
                println!("  ✓ Deeply nested structure (100 levels)");

                let unicode_dir = base.join("unicode");
                fixture::unicode_names(&unicode_dir, 100)?;
                println!("  ✓ 100 unicode-named entries");

                println!("\nAll fixtures generated successfully!");
            }
        },
    }

    Ok(())
}
