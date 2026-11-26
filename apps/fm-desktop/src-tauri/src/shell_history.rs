//! The local shell's own history file, read for command suggestions (task 0333). Read-only and
//! local: nothing is sent anywhere. Only the newest part of the file is read, so a long history
//! costs a bounded amount of time and memory.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Most bytes read from the end of a history file.
const MAX_READ_BYTES: u64 = 512 * 1024;
/// Most distinct commands kept, the newest.
pub(crate) const MAX_COMMANDS: usize = 2_000;

/// The commands of the local shell's history, oldest first and without repeats (a repeat keeps its
/// newest place). Empty when the shell has no readable history file.
pub(crate) fn read_history(shell: &str) -> Vec<String> {
    let Some(path) = history_path(shell) else {
        return Vec::new();
    };
    let Some(text) = read_tail(&path) else {
        return Vec::new();
    };
    let commands = match shell {
        "zsh" => parse_zsh(&text),
        "fish" => parse_fish(&text),
        _ => parse_bash(&text),
    };
    newest_distinct(commands)
}

/// Where the shell keeps its history. `HISTFILE` is not used: the app does not start the shell and
/// does not inherit its environment, so the usual defaults are read.
fn history_path(shell: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    match shell {
        "zsh" => Some(
            std::env::var_os("ZDOTDIR")
                .map_or(home.clone(), PathBuf::from)
                .join(".zsh_history"),
        ),
        "fish" => Some(match std::env::var_os("XDG_DATA_HOME") {
            Some(data) => PathBuf::from(data).join("fish/fish_history"),
            None => home.join(".local/share/fish/fish_history"),
        }),
        "bash" => Some(home.join(".bash_history")),
        _ => None,
    }
}

/// The last `MAX_READ_BYTES` of a file as text; a first line cut by the start is dropped.
fn read_tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(MAX_READ_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        Some(
            text.split_once('\n')
                .map_or(String::new(), |(_, rest)| rest.to_owned()),
        )
    } else {
        Some(text)
    }
}

/// zsh lines are `: <epoch>:<duration>;<command>` with `EXTENDED_HISTORY`, else the command itself.
pub(crate) fn parse_zsh(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| match line.strip_prefix(": ") {
            Some(rest) => rest.split_once(';').map_or(line, |(_, command)| command),
            None => line,
        })
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_owned)
        .collect()
}

/// bash lines are commands; `#<epoch>` lines are timestamps when `HISTTIMEFORMAT` is set.
pub(crate) fn parse_bash(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !(line.starts_with('#') && line[1..].chars().all(|c| c.is_ascii_digit())))
        .map(str::to_owned)
        .collect()
}

/// fish stores YAML-like entries: `- cmd: <command>`, with `when:` and `paths:` lines after it.
pub(crate) fn parse_fish(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("- cmd: "))
        .map(|command| command.replace("\\n", " ").replace("\\\\", "\\"))
        .map(|command| command.trim().to_owned())
        .filter(|command| !command.is_empty())
        .collect()
}

/// The newest copy of each command, in order of last use, keeping at most `MAX_COMMANDS`.
fn newest_distinct(commands: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut newest_first: Vec<String> = commands
        .into_iter()
        .rev()
        .filter(|command| seen.insert(command.clone()))
        .take(MAX_COMMANDS)
        .collect();
    newest_first.reverse();
    newest_first
}

#[cfg(test)]
mod tests {
    use super::{newest_distinct, parse_bash, parse_fish, parse_zsh};

    #[test]
    fn zsh_extended_and_plain_lines_are_commands() {
        let text = ": 1700000000:0;git status\nls -la\n: 1700000001:2;cargo test\n";
        assert_eq!(parse_zsh(text), ["git status", "ls -la", "cargo test"]);
    }

    #[test]
    fn bash_timestamp_lines_are_not_commands() {
        assert_eq!(
            parse_bash("#1700000000\ngit log\n\n#1700000001\nmake\n"),
            ["git log", "make"]
        );
    }

    #[test]
    fn fish_entries_are_read_by_their_cmd_key() {
        let text = "- cmd: git push\n  when: 1700000000\n- cmd: echo a\\nb\n  when: 1700000001\n";
        assert_eq!(parse_fish(text), ["git push", "echo a b"]);
    }

    #[test]
    fn a_repeat_keeps_its_newest_place() {
        let commands = ["make", "git status", "make", "cargo test"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            newest_distinct(commands),
            ["git status", "make", "cargo test"]
        );
    }
}
