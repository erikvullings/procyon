//! Skills: Agent Skills folders (`<name>/SKILL.md`) from the person's skill
//! folders and the project, plus notes as virtual skills. The prompt carries a short, bounded
//! catalogue (name and description); the body is loaded on demand. Everything here is read-only:
//! scripts in a skill are never run by the harness.

use std::collections::HashMap;
use std::fs;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use thiserror::Error;

use super::frontmatter::split_front_matter;
use crate::paths::{is_credential_folder, is_secret_name};

/// Most skills listed in the prompt.
const MAX_PROMPT_SKILLS: usize = 40;
/// Longest description in the prompt, in characters.
const MAX_DESCRIPTION_CHARS: usize = 160;
/// Most bytes of the catalogue in the prompt.
const MAX_CATALOGUE_BYTES: usize = 6 * 1024;
/// Most skill folders looked at in one skills folder.
const MAX_FOLDERS_PER_ROOT: usize = 500;
/// How much of a `SKILL.md` is read to find its front matter.
const MAX_HEADER_BYTES: u64 = 16 * 1024;
/// Most bytes of a skill file returned.
pub(crate) const MAX_SKILL_BYTES: usize = 32 * 1024;
/// Most files listed for one skill.
const MAX_LISTED_FILES: usize = 40;
/// Deepest folder level listed inside a skill.
const MAX_LIST_DEPTH: usize = 2;
/// Longest skill name, in characters.
const MAX_NAME_CHARS: usize = 64;

/// Where a skill comes from. The order is the precedence when names collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillSource {
    /// A skills folder of the project (the working folder up to its git root).
    Project,
    /// One of the person's skills folders.
    User,
    /// One of the person's notes.
    Note,
}

impl SkillSource {
    /// The label shown in the catalogue.
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Note => "note",
        }
    }
}

/// Where a skill's text is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillLocation {
    /// The folder that holds `SKILL.md`.
    Folder(PathBuf),
    /// A note, by its path in the notes folder.
    Note(String),
}

/// One skill of the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Its name, unique in the catalogue (ignoring case).
    pub name: String,
    /// When to use it.
    pub description: String,
    /// Where it comes from.
    pub source: SkillSource,
    /// Where its text is read from.
    pub location: SkillLocation,
}

/// A note offered as a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSkill {
    /// The skill name.
    pub name: String,
    /// When to use it.
    pub description: String,
    /// The note's path in the notes folder.
    pub path: String,
}

/// Why a skill file could not be read. A missing path and one outside the skill read the same.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SkillFileError {
    /// Missing, outside the skill, or in a credential folder.
    #[error("{0} is not available.")]
    NotAvailable(String),
    /// A file name that usually holds secrets.
    #[error("{0} may hold secrets and is not read.")]
    Secret(String),
    /// A folder, a device, or binary content.
    #[error("{0} is not a regular text file.")]
    NotText(String),
    /// Notes are single files.
    #[error("Notes have no extra files; read the note with read_skill.")]
    NoteHasNoFiles,
}

/// The folders skills are looked for in.
#[derive(Debug, Clone, Default)]
pub struct SkillRoots {
    /// Nearest first.
    pub project: Vec<PathBuf>,
    /// The person's folders.
    pub user: Vec<PathBuf>,
}

impl SkillRoots {
    /// The standard folders: `~/.claude/skills` and `~/.agents/skills`, the same two folders in
    /// the project (the working folder up to the nearest git root), and extra user folders.
    pub fn standard(
        working_directory: Option<&Path>,
        home: Option<&Path>,
        extra: &[PathBuf],
    ) -> Self {
        let mut roots = Self::default();
        if let Some(home) = home {
            roots.user.push(home.join(".claude").join("skills"));
            roots.user.push(home.join(".agents").join("skills"));
        }
        roots.user.extend(extra.iter().cloned());
        if let Some(start) = working_directory {
            for folder in start.ancestors() {
                if Some(folder) == home {
                    break;
                }
                roots.project.push(folder.join(".claude").join("skills"));
                roots.project.push(folder.join(".agents").join("skills"));
                if folder.join(".git").exists() {
                    break;
                }
            }
        }
        roots
    }
}

/// The skills one request can use.
#[derive(Debug, Clone, Default)]
pub struct SkillCatalogue {
    skills: Vec<Skill>,
    /// Lower-case name to position in `skills`.
    by_name: HashMap<String, usize>,
}

impl SkillCatalogue {
    /// Reads every `<root>/<name>/SKILL.md`. Project skills win over user skills on a name clash.
    /// Blocking: call it off the async threads. [`SkillIndex::discover`] does the same with a cache.
    pub fn discover(roots: &SkillRoots) -> Self {
        Self::discover_with(roots, None)
    }

    fn discover_with(roots: &SkillRoots, cache: Option<&SkillIndex>) -> Self {
        let mut catalogue = Self::default();
        // `~/.agents/skills` is often a symlink to (or a copy of) `~/.claude/skills`: a folder
        // is scanned once, and a name is listed once whichever folder it came from.
        let mut scanned = Vec::new();
        for (root, source) in roots
            .project
            .iter()
            .map(|root| (root, SkillSource::Project))
            .chain(roots.user.iter().map(|root| (root, SkillSource::User)))
        {
            let real = root.canonicalize().unwrap_or_else(|_| root.clone());
            if !scanned.contains(&real) {
                catalogue.add_folder_skills(root, source, cache);
                scanned.push(real);
            }
        }
        catalogue
    }

    /// Adds notes as skills, below folder skills when a name is taken.
    pub fn add_notes(&mut self, notes: Vec<NoteSkill>) {
        for note in notes {
            self.push(Skill {
                name: note.name,
                description: note.description,
                source: SkillSource::Note,
                location: SkillLocation::Note(note.path),
            });
        }
    }

    /// Every skill, in precedence order.
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// How many skills there are.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// The skill called `name`, ignoring case.
    pub fn find(&self, name: &str) -> Option<&Skill> {
        let index = *self.by_name.get(&name.trim().to_lowercase())?;
        self.skills.get(index)
    }

    /// Adds a skill unless its name is taken (the earlier one has precedence).
    pub fn push(&mut self, skill: Skill) {
        let key = skill.name.to_lowercase();
        if !self.by_name.contains_key(&key) {
            self.by_name.insert(key, self.skills.len());
            self.skills.push(skill);
        }
    }

    fn add_folder_skills(&mut self, root: &Path, source: SkillSource, cache: Option<&SkillIndex>) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        let mut folders: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
            })
            .collect();
        folders.sort();
        for folder in folders.into_iter().take(MAX_FOLDERS_PER_ROOT) {
            let header = match cache {
                Some(cache) => cache.header(&folder),
                None => read_skill_header(&folder),
            };
            if let Some(header) = header {
                self.push(Skill {
                    name: header.name,
                    description: header.description,
                    source,
                    location: SkillLocation::Folder(folder),
                });
            }
        }
    }

    /// The catalogue for the prompt: at most [`MAX_PROMPT_SKILLS`] skills within
    /// [`MAX_CATALOGUE_BYTES`], notes first, then the skills that match `request` best.
    pub fn prompt_section(&self, request: &str) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let terms: Vec<String> = request
            .split(|c: char| !c.is_alphanumeric())
            .filter(|term| term.chars().count() > 2)
            .map(str::to_lowercase)
            .collect();
        let mut ranked: Vec<(usize, &Skill)> = self
            .skills
            .iter()
            .map(|skill| {
                let text = format!("{} {}", skill.name, skill.description).to_lowercase();
                let hits = terms
                    .iter()
                    .filter(|term| text.contains(term.as_str()))
                    .count();
                (hits, skill)
            })
            .collect();
        // Stable: equal matches keep the precedence order.
        ranked.sort_by_key(|(hits, skill)| {
            (skill.source != SkillSource::Note, std::cmp::Reverse(*hits))
        });
        let mut text = String::from(
            "Skills (load one with the read_skill tool when the request matches its description):",
        );
        let header = text.len();
        for (_, skill) in ranked.into_iter().take(MAX_PROMPT_SKILLS) {
            let line = format!(
                "\n- {}: {} [{}]",
                skill.name,
                bounded_description(&skill.description),
                skill.source.label()
            );
            if text.len() + line.len() > MAX_CATALOGUE_BYTES {
                break;
            }
            text.push_str(&line);
        }
        if text.len() == header {
            return String::new();
        }
        text
    }
}

/// The name and description of a skill folder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillHeader {
    name: String,
    description: String,
}

/// Remembers the headers of `SKILL.md` files between requests: a header is read again only when
/// its file's size or modification time changed, so a request costs one `stat` per skill instead
/// of one read. Share one index for the life of the host.
#[derive(Debug, Default)]
pub struct SkillIndex {
    headers: Mutex<HashMap<PathBuf, CachedHeader>>,
}

#[derive(Debug, Clone)]
struct CachedHeader {
    modified: Option<SystemTime>,
    len: u64,
    header: Option<SkillHeader>,
}

impl SkillIndex {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// [`SkillCatalogue::discover`], reading only the `SKILL.md` files that changed since the last
    /// call. Blocking: call it off the async threads.
    pub fn discover(&self, roots: &SkillRoots) -> SkillCatalogue {
        SkillCatalogue::discover_with(roots, Some(self))
    }

    fn header(&self, folder: &Path) -> Option<SkillHeader> {
        let manifest = folder.join("SKILL.md");
        let metadata = fs::metadata(&manifest).ok()?;
        let (modified, len) = (metadata.modified().ok(), metadata.len());
        let mut headers = self
            .headers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(cached) = headers.get(&manifest)
            && cached.modified.is_some()
            && cached.modified == modified
            && cached.len == len
        {
            return cached.header.clone();
        }
        drop(headers);
        let header = read_skill_header(folder);
        headers = self
            .headers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        headers.insert(
            manifest,
            CachedHeader {
                modified,
                len,
                header: header.clone(),
            },
        );
        header
    }
}

/// The skill in `folder`, from the front matter of its `SKILL.md`.
fn read_skill_header(folder: &Path) -> Option<SkillHeader> {
    let manifest = folder.join("SKILL.md");
    if !fs::metadata(&manifest).ok()?.is_file() || !fs::metadata(folder).ok()?.is_dir() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(&manifest)
        .ok()?
        .take(MAX_HEADER_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let (front, body) = split_front_matter(&text);
    let folder_name = folder.file_name()?.to_string_lossy().into_owned();
    let name = front
        .as_ref()
        .and_then(|front| front.get("name"))
        .map(str::trim)
        .filter(|name| is_valid_name(name))
        .map_or(folder_name, str::to_owned);
    let description = front
        .as_ref()
        .and_then(|front| front.get("description"))
        .map(str::to_owned)
        .or_else(|| first_paragraph(body))
        .unwrap_or_default();
    Some(SkillHeader { name, description })
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The first plain paragraph of a Markdown body, on one line.
fn first_paragraph(body: &str) -> Option<String> {
    let paragraph: Vec<&str> = body
        .lines()
        .map(str::trim)
        .skip_while(|line| line.is_empty() || line.starts_with('#'))
        .take_while(|line| !line.is_empty())
        .collect();
    (!paragraph.is_empty()).then(|| paragraph.join(" "))
}

fn bounded_description(description: &str) -> String {
    let one_line = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX_DESCRIPTION_CHARS {
        return one_line;
    }
    let mut cut: String = one_line.chars().take(MAX_DESCRIPTION_CHARS).collect();
    cut.push('…');
    cut
}

impl Skill {
    /// The `SKILL.md` body (without front matter), bounded. `None` for a note.
    pub fn read_body(&self) -> Option<Result<String, SkillFileError>> {
        let SkillLocation::Folder(_) = &self.location else {
            return None;
        };
        Some(self.read_file("SKILL.md").map(|text| {
            let (_, body) = split_front_matter(&text);
            body.trim_start().to_owned()
        }))
    }

    /// The skill's folder, for a skill that lives in one (a note has none).
    pub fn folder(&self) -> Option<&Path> {
        match &self.location {
            SkillLocation::Folder(folder) => Some(folder),
            SkillLocation::Note(_) => None,
        }
    }

    /// The real path of a file inside the skill's folder. It cannot leave the folder, not even
    /// through a symlink or `..`; credential folders and secret-looking names are refused; it must
    /// be a regular file.
    pub fn resolve_file(&self, relative: &str) -> Result<PathBuf, SkillFileError> {
        match &self.location {
            SkillLocation::Folder(folder) => resolve_inside(folder, relative),
            SkillLocation::Note(_) => Err(SkillFileError::NoteHasNoFiles),
        }
    }

    /// The text of a file inside the skill's folder (see [`resolve_file`](Self::resolve_file)),
    /// bounded; binary files are refused.
    pub fn read_file(&self, relative: &str) -> Result<String, SkillFileError> {
        let path = self.resolve_file(relative)?;
        self.read_checked(&path, 0)
    }

    /// The text of a path [`resolve_file`](Self::resolve_file) returned, from byte `offset`,
    /// bounded; a cut-off text ends with the offset to continue from.
    pub fn read_checked(&self, path: &Path, offset: usize) -> Result<String, SkillFileError> {
        read_text(path, offset)
    }

    /// The files inside the skill's folder worth reading, relative, bounded.
    pub fn list_files(&self) -> Vec<String> {
        let SkillLocation::Folder(folder) = &self.location else {
            return Vec::new();
        };
        let Ok(base) = folder.canonicalize() else {
            return Vec::new();
        };
        let mut files = Vec::new();
        list_into(&base, &base, 0, &mut files);
        files.retain(|file| file != "SKILL.md");
        files
    }
}

fn list_into(base: &Path, folder: &Path, depth: usize, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if files.len() >= MAX_LISTED_FILES {
            return;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') || is_secret_name(&name) || is_credential_folder(&name) {
            continue;
        }
        // A symlink pointing out of the skill is not listed.
        let Ok(real) = path.canonicalize() else {
            continue;
        };
        if !real.starts_with(base) {
            continue;
        }
        if real.is_dir() {
            if depth < MAX_LIST_DEPTH {
                list_into(base, &real, depth + 1, files);
            }
        } else if let Ok(relative) = real.strip_prefix(base) {
            files.push(relative.display().to_string());
        }
    }
}

/// The real path of `relative` inside `folder`, checked (see [`Skill::resolve_file`]).
fn resolve_inside(folder: &Path, relative: &str) -> Result<PathBuf, SkillFileError> {
    let relative = relative.trim();
    let unavailable = || SkillFileError::NotAvailable(relative.to_owned());
    let candidate = Path::new(relative);
    if relative.is_empty()
        || candidate.is_absolute()
        || candidate
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(unavailable());
    }
    let base = folder.canonicalize().map_err(|_| unavailable())?;
    let real = base
        .join(candidate)
        .canonicalize()
        .map_err(|_| unavailable())?;
    let inside = real.strip_prefix(&base).map_err(|_| unavailable())?;
    if inside
        .components()
        .any(|part| is_credential_folder(&part.as_os_str().to_string_lossy()))
    {
        return Err(unavailable());
    }
    let name = real
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if is_secret_name(&name) {
        return Err(SkillFileError::Secret(name));
    }
    if !fs::metadata(&real).is_ok_and(|metadata| metadata.is_file()) {
        return Err(SkillFileError::NotText(name));
    }
    Ok(real)
}

/// The text of a checked file from byte `offset`, at most [`MAX_SKILL_BYTES`]; binary is refused.
fn read_text(path: &Path, offset: usize) -> Result<String, SkillFileError> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let not_text = || SkillFileError::NotText(name.clone());
    let mut file = fs::File::open(path).map_err(|_| not_text())?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|_| not_text())?;
    let mut buffer = Vec::new();
    file.take(MAX_SKILL_BYTES as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| not_text())?;
    if buffer.contains(&0) {
        return Err(not_text());
    }
    let truncated = buffer.len() > MAX_SKILL_BYTES;
    buffer.truncate(MAX_SKILL_BYTES);
    // Cut at a character boundary so the next read starts on one.
    let text = match std::str::from_utf8(&buffer) {
        Ok(text) => text.to_owned(),
        Err(error) if truncated && error.error_len().is_none() => {
            buffer.truncate(error.valid_up_to());
            String::from_utf8_lossy(&buffer).into_owned()
        }
        Err(_) => String::from_utf8_lossy(&buffer).into_owned(),
    };
    if truncated {
        let next = offset + buffer.len();
        return Ok(format!("{text}\n[truncated: continue with offset {next}]"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests;
