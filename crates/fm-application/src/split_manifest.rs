//! Pure naming, set-detection and manifest rules for splitting and combining files (task 0257).
//!
//! Parts are named `<name>.001`, `<name>.002`, ... (three digits, growing past 999). A split can
//! also write `<name>.blake3`, a plain-text manifest whose hash lines are accepted by `b3sum -c`:
//!
//! ```text
//! # procyon-split 1
//! # name: movie.mkv
//! # size: 2500000000
//! # part-size: 1000000000
//! # parts: 3
//! # blake3: <hex of the whole file>
//! <hex>  movie.mkv.001
//! <hex>  movie.mkv.002
//! <hex>  movie.mkv.003
//! ```

use std::collections::BTreeSet;

const MANIFEST_MAGIC: &str = "# procyon-split 1";
/// Hex length of a BLAKE3 digest.
const HASH_LEN: usize = 64;

/// Name of part `index` (1-based) of `base`.
pub(crate) fn part_name(base: &str, index: u32) -> String {
    format!("{base}.{index:03}")
}

/// Name of the manifest that belongs to `base`.
pub(crate) fn manifest_name(base: &str) -> String {
    format!("{base}.blake3")
}

/// Splits `movie.mkv.003` into (`movie.mkv`, 3); `None` when the name is not a part name.
pub(crate) fn parse_part_name(name: &str) -> Option<(&str, u32)> {
    let (base, suffix) = name.rsplit_once('.')?;
    if base.is_empty() || suffix.len() < 3 || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let index: u32 = suffix.parse().ok()?;
    (index >= 1).then_some((base, index))
}

/// Number of parts a file of `size` bytes needs; an empty file still yields one empty part.
pub(crate) fn part_count(size: u64, part_size: u64) -> u64 {
    if size == 0 {
        1
    } else {
        size.div_ceil(part_size)
    }
}

/// Why the parts found next to a selection do not form a joinable set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetProblem {
    /// Part numbers missing between the first and the last.
    Gaps(Vec<u32>),
    /// The selected names belong to different sets.
    Mixed,
    /// A selected name is not a part name.
    NotAPart(String),
}

impl SetProblem {
    pub(crate) fn describe(&self, base: &str) -> String {
        match self {
            Self::Gaps(missing) => {
                let shown: Vec<String> = missing
                    .iter()
                    .take(5)
                    .map(|index| part_name(base, *index))
                    .collect();
                let more = missing.len().saturating_sub(5);
                if more > 0 {
                    format!("missing parts: {} and {more} more", shown.join(", "))
                } else {
                    format!("missing parts: {}", shown.join(", "))
                }
            }
            Self::Mixed => "the selected files belong to different split sets".into(),
            Self::NotAPart(name) => format!("{name} is not a numbered part (.001, .002, ...)"),
        }
    }
}

/// The set a selection of part names points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedSet {
    /// Name of the original file (the part name without its number).
    pub base: String,
    /// All part indices present in the folder for this base, ascending.
    pub present: Vec<u32>,
    /// Problem that makes the set unusable, if any.
    pub problem: Option<SetProblem>,
}

/// Works out the set that `selected` names belong to, given every name in their folder.
///
/// Every selected name must be a part of the same base; the set is then every part of that base
/// in the folder. Gaps (including a missing `.001`) make the set unusable.
pub(crate) fn detect_set<'a>(
    selected: impl IntoIterator<Item = &'a str>,
    folder_names: impl IntoIterator<Item = &'a str>,
) -> Option<DetectedSet> {
    let mut base: Option<String> = None;
    let mut problem = None;
    for name in selected {
        match parse_part_name(name) {
            None => {
                problem = Some(SetProblem::NotAPart(name.to_owned()));
                base.get_or_insert_with(|| name.to_owned());
            }
            Some((candidate, _)) => match &base {
                None => base = Some(candidate.to_owned()),
                Some(existing) if existing != candidate => problem = Some(SetProblem::Mixed),
                Some(_) => {}
            },
        }
    }
    let base = base?;
    if problem.is_some() {
        return Some(DetectedSet {
            base,
            present: Vec::new(),
            problem,
        });
    }
    let present: BTreeSet<u32> = folder_names
        .into_iter()
        .filter_map(parse_part_name)
        .filter(|(candidate, _)| *candidate == base)
        .map(|(_, index)| index)
        .collect();
    let last = present.last().copied().unwrap_or(0);
    let missing: Vec<u32> = (1..=last)
        .filter(|index| !present.contains(index))
        .collect();
    Some(DetectedSet {
        base,
        present: present.into_iter().collect(),
        problem: (!missing.is_empty()).then_some(SetProblem::Gaps(missing)),
    })
}

/// One part line of a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestPart {
    pub name: String,
    pub hash: String,
}

/// Parsed content of a `.blake3` manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub name: String,
    pub size: u64,
    pub part_size: u64,
    /// BLAKE3 of the whole original file.
    pub whole_hash: String,
    pub parts: Vec<ManifestPart>,
}

impl Manifest {
    /// Renders the manifest text.
    pub(crate) fn render(&self) -> String {
        let mut text = format!(
            "{MANIFEST_MAGIC}\n# name: {}\n# size: {}\n# part-size: {}\n# parts: {}\n# blake3: {}\n",
            self.name,
            self.size,
            self.part_size,
            self.parts.len(),
            self.whole_hash
        );
        for part in &self.parts {
            text.push_str(&format!("{}  {}\n", part.hash, part.name));
        }
        text
    }

    /// Parses manifest text; errors name the first thing wrong with it.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(MANIFEST_MAGIC) {
            return Err("not a Procyon split manifest".into());
        }
        let (mut name, mut size, mut part_size, mut count, mut whole) =
            (None, None, None, None, None);
        let mut parts = Vec::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(header) = line.strip_prefix('#') {
                let Some((key, value)) = header.split_once(':') else {
                    continue;
                };
                let value = value.trim();
                match key.trim() {
                    "name" => name = Some(value.to_owned()),
                    "size" => size = value.parse::<u64>().ok(),
                    "part-size" => part_size = value.parse::<u64>().ok(),
                    "parts" => count = value.parse::<usize>().ok(),
                    "blake3" => whole = Some(value.to_owned()),
                    _ => {}
                }
                continue;
            }
            let (hash, part) = line
                .split_once("  ")
                .ok_or_else(|| format!("malformed manifest line: {line}"))?;
            if !is_hash(hash) {
                return Err(format!("malformed hash for {part}"));
            }
            parts.push(ManifestPart {
                name: part.trim().to_owned(),
                hash: hash.to_ascii_lowercase(),
            });
        }
        let whole_hash = whole
            .filter(|hash| is_hash(hash))
            .ok_or("manifest has no valid whole-file hash")?
            .to_ascii_lowercase();
        let manifest = Self {
            name: name.ok_or("manifest has no name")?,
            size: size.ok_or("manifest has no size")?,
            part_size: part_size.ok_or("manifest has no part size")?,
            whole_hash,
            parts,
        };
        if count != Some(manifest.parts.len()) {
            return Err("manifest part count does not match its hash lines".into());
        }
        Ok(manifest)
    }
}

fn is_hash(value: &str) -> bool {
    value.len() == HASH_LEN && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const H1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const H2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn names_parts_with_three_digits_and_grows_past_999() {
        assert_eq!(part_name("a.bin", 1), "a.bin.001");
        assert_eq!(part_name("a.bin", 1000), "a.bin.1000");
    }

    #[test]
    fn parses_only_numbered_part_names() {
        assert_eq!(parse_part_name("a.bin.001"), Some(("a.bin", 1)));
        assert_eq!(parse_part_name("a.bin.1000"), Some(("a.bin", 1000)));
        assert_eq!(parse_part_name("a.bin.000"), None);
        assert_eq!(parse_part_name("a.bin.01"), None);
        assert_eq!(parse_part_name("a.bin"), None);
        assert_eq!(parse_part_name(".001"), None);
        assert_eq!(parse_part_name("a.bin.0a1"), None);
    }

    #[test]
    fn counts_parts_including_the_empty_file() {
        assert_eq!(part_count(0, 10), 1);
        assert_eq!(part_count(10, 10), 1);
        assert_eq!(part_count(11, 10), 2);
    }

    #[test]
    fn detects_a_set_from_any_part_among_unrelated_and_interleaved_names() {
        let folder = [
            "z.txt",
            "b.bin.002",
            "a.bin.003",
            "a.bin.001",
            "b.bin.001",
            "a.bin.002",
            "a.bin",
        ];
        let set = detect_set(["a.bin.002"], folder).expect("set");
        assert_eq!(set.base, "a.bin");
        assert_eq!(set.present, vec![1, 2, 3]);
        assert_eq!(set.problem, None);
        let both = detect_set(["a.bin.003", "a.bin.001"], folder).expect("set");
        assert_eq!(both.present, vec![1, 2, 3]);
    }

    #[test]
    fn reports_gaps_including_a_missing_first_part() {
        let set = detect_set(["a.bin.002"], ["a.bin.002", "a.bin.004"]).expect("set");
        assert_eq!(set.problem, Some(SetProblem::Gaps(vec![1, 3])));
        assert!(
            set.problem
                .expect("problem")
                .describe("a.bin")
                .contains("a.bin.001")
        );
    }

    #[test]
    fn rejects_mixed_selections_and_non_parts() {
        let mixed = detect_set(["a.bin.001", "b.bin.001"], ["a.bin.001", "b.bin.001"]);
        assert_eq!(mixed.expect("set").problem, Some(SetProblem::Mixed));
        let odd = detect_set(["a.bin"], ["a.bin"]);
        assert_eq!(
            odd.expect("set").problem,
            Some(SetProblem::NotAPart("a.bin".into()))
        );
        assert_eq!(detect_set([], ["a.bin.001"]), None);
    }

    #[test]
    fn manifest_round_trips() {
        let manifest = Manifest {
            name: "a.bin".into(),
            size: 15,
            part_size: 10,
            whole_hash: H1.into(),
            parts: vec![
                ManifestPart {
                    name: "a.bin.001".into(),
                    hash: H1.into(),
                },
                ManifestPart {
                    name: "a.bin.002".into(),
                    hash: H2.into(),
                },
            ],
        };
        let text = manifest.render();
        assert!(text.contains(&format!("{H2}  a.bin.002")));
        assert_eq!(Manifest::parse(&text), Ok(manifest));
    }

    #[test]
    fn manifest_parse_rejects_damaged_text() {
        assert!(Manifest::parse("hello").is_err());
        let truncated = format!(
            "{MANIFEST_MAGIC}\n# name: a\n# size: 1\n# part-size: 1\n# parts: 2\n# blake3: {H1}\n{H1}  a.001\n"
        );
        assert!(Manifest::parse(&truncated).unwrap_err().contains("count"));
        let bad_hash = format!(
            "{MANIFEST_MAGIC}\n# name: a\n# size: 1\n# part-size: 1\n# parts: 1\n# blake3: {H1}\nzz  a.001\n"
        );
        assert!(Manifest::parse(&bad_hash).is_err());
    }
}
