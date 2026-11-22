//! Skills (task 0339): Agent Skills folders (`<name>/SKILL.md`) from the person's skill
//! folders and the project, plus notes as virtual skills. The prompt carries a short, bounded
//! catalogue (name and description); the body is loaded on demand. Everything here is read-only:
//! scripts in a skill are never run by the harness.

use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use super::frontmatter::split_front_matter;
use crate::terminal_assist::{is_credential_folder, is_secret_name};

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
pub(crate) enum SkillSource {
    Project,
    User,
    Note,
}

impl SkillSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Note => "note",
        }
    }
}

/// Where a skill's text is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkillLocation {
    /// The folder that holds `SKILL.md`.
    Folder(PathBuf),
    /// A note, by its path in the notes folder.
    Note(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Skill {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) source: SkillSource,
    pub(crate) location: SkillLocation,
}

/// A note offered as a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoteSkill {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) path: String,
}

/// Why a skill file could not be read. A missing path and one outside the skill read the same.
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum SkillFileError {
    #[error("{0} is not available.")]
    NotAvailable(String),
    #[error("{0} may hold secrets and is not read.")]
    Secret(String),
    #[error("{0} is not a regular text file.")]
    NotText(String),
    #[error("Notes have no extra files; read the note with read_skill.")]
    NoteHasNoFiles,
}

/// The folders skills are looked for in.
#[derive(Debug, Clone, Default)]
pub(crate) struct SkillRoots {
    /// Nearest first.
    pub(crate) project: Vec<PathBuf>,
    pub(crate) user: Vec<PathBuf>,
}

impl SkillRoots {
    /// The standard folders: `~/.claude/skills` and `~/.agents/skills`, the same two folders in
    /// the project (the working folder up to the nearest git root), and extra user folders.
    pub(crate) fn standard(
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
pub(crate) struct SkillCatalogue {
    skills: Vec<Skill>,
}

impl SkillCatalogue {
    /// Reads every `<root>/<name>/SKILL.md`. Project skills win over user skills on a name clash.
    pub(crate) fn discover(roots: &SkillRoots) -> Self {
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
                catalogue.add_folder_skills(root, source);
                scanned.push(real);
            }
        }
        catalogue
    }

    /// Adds notes as skills, below folder skills when a name is taken.
    pub(crate) fn add_notes(&mut self, notes: Vec<NoteSkill>) {
        for note in notes {
            self.push(Skill {
                name: note.name,
                description: note.description,
                source: SkillSource::Note,
                location: SkillLocation::Note(note.path),
            });
        }
    }

    pub(crate) fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// The skill called `name`, ignoring case.
    pub(crate) fn find(&self, name: &str) -> Option<&Skill> {
        let name = name.trim();
        self.skills
            .iter()
            .find(|skill| skill.name.eq_ignore_ascii_case(name))
    }

    fn push(&mut self, skill: Skill) {
        if self.find(&skill.name).is_none() {
            self.skills.push(skill);
        }
    }

    fn add_folder_skills(&mut self, root: &Path, source: SkillSource) {
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
            if let Some(skill) = read_skill_header(&folder, source) {
                self.push(skill);
            }
        }
    }

    /// The catalogue for the prompt: at most [`MAX_PROMPT_SKILLS`] skills within
    /// [`MAX_CATALOGUE_BYTES`], notes first, then the skills that match `request` best.
    pub(crate) fn prompt_section(&self, request: &str) -> String {
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
            "Skills you can load with {\"tool\":\"read_skill\",\"name\":\"<name>\"}; load one \
             when the request matches its description:",
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

/// The skill in `folder`, from the front matter of its `SKILL.md`.
fn read_skill_header(folder: &Path, source: SkillSource) -> Option<Skill> {
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
    Some(Skill {
        name,
        description,
        source,
        location: SkillLocation::Folder(folder.to_path_buf()),
    })
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
    /// The `SKILL.md` body (without front matter), bounded. Only for folder skills.
    pub(crate) fn read_body(&self) -> Option<Result<String, SkillFileError>> {
        let SkillLocation::Folder(folder) = &self.location else {
            return None;
        };
        Some(read_inside(folder, "SKILL.md").map(|text| {
            let (_, body) = split_front_matter(&text);
            body.trim_start().to_owned()
        }))
    }

    /// A file inside the skill's folder. It cannot leave the folder, not even through a symlink or
    /// `..`, and secret-looking files are refused.
    pub(crate) fn read_file(&self, relative: &str) -> Result<String, SkillFileError> {
        match &self.location {
            SkillLocation::Folder(folder) => read_inside(folder, relative),
            SkillLocation::Note(_) => Err(SkillFileError::NoteHasNoFiles),
        }
    }

    /// The files inside the skill's folder worth reading, relative, bounded.
    pub(crate) fn list_files(&self) -> Vec<String> {
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

/// The text of `relative` inside `folder`, bounded; binary files are refused.
fn read_inside(folder: &Path, relative: &str) -> Result<String, SkillFileError> {
    let unavailable = || SkillFileError::NotAvailable(relative.to_owned());
    let candidate = Path::new(relative.trim());
    if relative.trim().is_empty()
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
    let mut buffer = Vec::new();
    fs::File::open(&real)
        .and_then(|file| {
            file.take(MAX_SKILL_BYTES as u64 + 1)
                .read_to_end(&mut buffer)
        })
        .map_err(|_| SkillFileError::NotText(name.clone()))?;
    if buffer.contains(&0) {
        return Err(SkillFileError::NotText(name));
    }
    let truncated = buffer.len() > MAX_SKILL_BYTES;
    buffer.truncate(MAX_SKILL_BYTES);
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if truncated {
        text.push_str("\n[truncated]");
    }
    Ok(text)
}

#[cfg(test)]
mod tests;
