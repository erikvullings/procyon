//! Automatic shell integration for command blocks (task 0333), for zsh and bash.
//!
//! When the person has consented, the embedded terminal starts its shell through small wrapper
//! files kept in the app's data folder. The wrappers load the person's own startup files exactly
//! as the shell would, then install hooks that print OSC 133 marks (prompt, command output start,
//! command end with exit status). Nothing in the person's dotfiles changes, and turning the
//! setting off starts new shells without the wrappers. Other shells, and remote shells, are not
//! wrapped.

use std::io;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;

/// Hooks for zsh. They run after the person's `.zshrc`, so their prompt setup cannot remove them.
const ZSH_HOOKS: &str = r#"# Procyon shell integration (command blocks)
__procyon_precmd() {
  local exit_code=$?
  printf '\e]133;D;%d\a' "$exit_code"
  printf '\e]133;A\a'
}
__procyon_preexec() {
  printf '\e]133;C\a'
}
autoload -Uz add-zsh-hook
add-zsh-hook precmd __procyon_precmd
add-zsh-hook preexec __procyon_preexec
"#;

/// Startup for zsh's interactive shell: the person's `.zshrc`, then the hooks.
const ZSHRC: &str = r#"# Procyon shell integration: loads the person's own .zshrc, then the command-block hooks.
export ZDOTDIR="$PROCYON_USER_ZDOTDIR"
if [[ -r "$ZDOTDIR/.zshrc" ]]; then source "$ZDOTDIR/.zshrc"; fi
"#;

/// Wrapper for zsh's environment file; sourced first, from the person's own folder.
const ZSHENV: &str = r#"# Procyon shell integration: the person's own .zshenv, unchanged.
if [[ -r "$PROCYON_USER_ZDOTDIR/.zshenv" ]]; then source "$PROCYON_USER_ZDOTDIR/.zshenv"; fi
"#;

/// Wrapper for zsh's login file; the person's own .zprofile, unchanged.
const ZPROFILE: &str = r#"# Procyon shell integration: the person's own .zprofile, unchanged.
if [[ -r "$PROCYON_USER_ZDOTDIR/.zprofile" ]]; then source "$PROCYON_USER_ZDOTDIR/.zprofile"; fi
"#;

/// Startup for bash. It runs in place of `.bash_profile`/`.bashrc`: the login startup a terminal
/// normally gets, then the person's `.bashrc` unless their profile already reads it, then hooks.
const BASH_STARTUP: &str = r#"# Procyon shell integration (command blocks): the login startup, then the hooks.
if [ -r /etc/profile ]; then . /etc/profile; fi
__procyon_loaded_bashrc=0
for __procyon_profile in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
  if [ -r "$__procyon_profile" ]; then
    . "$__procyon_profile"
    if grep -q bashrc "$__procyon_profile"; then __procyon_loaded_bashrc=1; fi
    break
  fi
done
if [ "$__procyon_loaded_bashrc" = 0 ] && [ -r "$HOME/.bashrc" ]; then . "$HOME/.bashrc"; fi
unset __procyon_profile __procyon_loaded_bashrc

__procyon_begin() { __procyon_status=$?; }
__procyon_end() {
  printf '\e]133;D;%d\a' "$__procyon_status"
  printf '\e]133;A\a'
  __procyon_at_prompt=1
}
__procyon_at_prompt=1
__procyon_preexec() {
  if [ "$__procyon_at_prompt" = 1 ]; then
    __procyon_at_prompt=0
    printf '\e]133;C\a'
  fi
}
trap '__procyon_preexec' DEBUG
PROMPT_COMMAND="__procyon_begin${PROMPT_COMMAND:+; $PROMPT_COMMAND}; __procyon_end"
"#;

/// Hooks for fish, loaded before the person's `config.fish` with `-C`. They use fish events, so a
/// person's own prompt function cannot remove them.
const FISH_HOOKS: &str = r#"# Procyon shell integration (command blocks)
function __procyon_prompt --on-event fish_prompt
    set -l exit_code $status
    printf '\e]133;D;%d\a' $exit_code
    printf '\e]133;A\a'
end
function __procyon_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end
"#;

/// The shells this module can wrap, identified from the shell program's file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellKind {
    Zsh,
    Bash,
    Fish,
}

impl ShellKind {
    /// The kind for a shell program path, or `None` when command blocks are not supported for it.
    pub(crate) fn of(program: &Path) -> Option<Self> {
        match program.file_name()?.to_str()? {
            "zsh" => Some(Self::Zsh),
            "bash" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }
}

/// Writes the wrapper files into `dir`. Existing files are replaced, so an app update takes effect
/// the next time a terminal starts.
pub(crate) fn write_wrappers(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let files = [
        (".zshenv", ZSHENV.to_owned()),
        (".zprofile", ZPROFILE.to_owned()),
        (".zshrc", format!("{ZSHRC}{ZSH_HOOKS}")),
        ("bashrc", BASH_STARTUP.to_owned()),
        (FISH_FILE, FISH_HOOKS.to_owned()),
    ];
    for (name, contents) in files {
        std::fs::write(dir.join(name), contents)?;
    }
    Ok(())
}

/// The fish hook file in the wrapper folder.
const FISH_FILE: &str = "fish-integration.fish";

/// Single-quotes `text` for fish, escaping quotes and backslashes.
fn fish_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The shell command that starts `program` with command-block integration, or `None` when the
/// shell is not wrapped. The person's own folder for zsh comes from `ZDOTDIR` when set, else `HOME`.
pub(crate) fn integrated_command(
    program: &Path,
    wrappers: &Path,
    cwd: &Path,
    home: Option<&Path>,
    user_zdotdir: Option<&Path>,
) -> Option<CommandBuilder> {
    let kind = ShellKind::of(program)?;
    let mut command = CommandBuilder::new(program);
    command.cwd(cwd);
    match kind {
        ShellKind::Zsh => {
            // `-l` reads .zprofile, as a login terminal does; the wrappers in `wrappers` are read
            // in its place through ZDOTDIR, and they source the person's own files.
            command.arg("-l");
            command.env("ZDOTDIR", wrappers);
            let original = user_zdotdir.or(home)?;
            command.env("PROCYON_USER_ZDOTDIR", original);
        }
        ShellKind::Fish => {
            // `-C` runs before the person's config.fish, which then loads as usual.
            command.arg("-l");
            command.arg("-C");
            command.arg(format!(
                "source {}",
                fish_quote(&wrappers.join(FISH_FILE).to_string_lossy())
            ));
        }
        ShellKind::Bash => {
            // Bash reads `--init-file` only for non-login shells; `BASH_STARTUP` does the login
            // startup itself, so this is interactive but not login.
            command.arg("--init-file");
            command.arg(wrappers.join("bashrc"));
            command.arg("-i");
        }
    }
    Some(command)
}

/// Where the wrapper files live, given the app's data folder.
pub(crate) fn wrapper_dir(app_data: &Path) -> PathBuf {
    app_data.join("shell-integration")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_and_bash_are_wrapped_and_other_shells_are_not() {
        assert_eq!(ShellKind::of(Path::new("/bin/zsh")), Some(ShellKind::Zsh));
        assert_eq!(
            ShellKind::of(Path::new("/usr/local/bin/bash")),
            Some(ShellKind::Bash)
        );
        assert_eq!(
            ShellKind::of(Path::new("/opt/homebrew/bin/fish")),
            Some(ShellKind::Fish)
        );
        assert_eq!(ShellKind::of(Path::new("pwsh")), None);
    }

    #[test]
    fn the_zsh_wrapper_points_zsh_at_the_person_folder_and_keeps_hooks_last() {
        let zshrc = format!("{ZSHRC}{ZSH_HOOKS}");
        assert!(zshrc.contains("source \"$ZDOTDIR/.zshrc\""));
        assert!(zshrc.find("source").unwrap() < zshrc.find("add-zsh-hook precmd").unwrap());
        assert!(ZSH_HOOKS.contains(r"\e]133;C\a"));
    }

    #[test]
    fn writing_the_wrappers_creates_every_file() {
        let dir = tempfile::tempdir().unwrap();
        write_wrappers(dir.path()).unwrap();
        for name in [".zshenv", ".zprofile", ".zshrc", "bashrc", FISH_FILE] {
            assert!(dir.path().join(name).is_file(), "{name} missing");
        }
        let bashrc = std::fs::read_to_string(dir.path().join("bashrc")).unwrap();
        assert!(bashrc.contains("133;D;"));
        assert!(bashrc.contains("trap '__procyon_preexec' DEBUG"));
    }

    #[test]
    fn a_zsh_command_reads_the_wrappers_and_records_the_person_folder() {
        let command = integrated_command(
            Path::new("/bin/zsh"),
            Path::new("/data/shell-integration"),
            Path::new("/work"),
            Some(Path::new("/home/me")),
            None,
        )
        .unwrap();
        assert_eq!(command.get_argv()[1], "-l");
        assert_eq!(
            command.get_env("ZDOTDIR"),
            Some(std::ffi::OsStr::new("/data/shell-integration"))
        );
        assert_eq!(
            command.get_env("PROCYON_USER_ZDOTDIR"),
            Some(std::ffi::OsStr::new("/home/me"))
        );
    }

    #[test]
    fn a_bash_command_uses_the_wrapper_as_its_init_file() {
        let command = integrated_command(
            Path::new("/bin/bash"),
            Path::new("/data/shell-integration"),
            Path::new("/work"),
            Some(Path::new("/home/me")),
            None,
        )
        .unwrap();
        let argv: Vec<_> = command
            .get_argv()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            argv,
            [
                "/bin/bash",
                "--init-file",
                "/data/shell-integration/bashrc",
                "-i"
            ]
        );
    }

    #[test]
    fn unsupported_shells_get_no_integrated_command() {
        assert!(
            integrated_command(
                Path::new("/usr/bin/nu"),
                Path::new("/data"),
                Path::new("/work"),
                Some(Path::new("/home/me")),
                None,
            )
            .is_none()
        );
    }

    /// Runs `command` in a pseudo-terminal with `lines` typed in, and returns everything printed.
    fn run_in_pty(command: CommandBuilder, lines: &[&str]) -> String {
        use std::io::{Read, Write};
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize::default())
            .expect("open pty");
        let mut child = pair.slave.spawn_command(command).expect("spawn shell");
        drop(pair.slave);
        let mut writer = pair.master.take_writer().expect("pty writer");
        let mut reader = pair.master.try_clone_reader().expect("pty reader");
        for line in lines {
            writer.write_all(line.as_bytes()).expect("type input");
        }
        drop(writer);
        // Drain before waiting: on macOS the child cannot exit while its PTY output is unread.
        let mut output = String::new();
        let _ = reader.read_to_string(&mut output);
        let _ = child.wait();
        drop(pair.master);
        output
    }

    #[cfg(unix)]
    #[test]
    fn bash_loads_the_persons_bashrc_and_reports_command_marks() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join(".bashrc"),
            "export PROCYON_TEST_RC=loaded\n",
        )
        .unwrap();
        let wrappers = tempfile::tempdir().unwrap();
        write_wrappers(wrappers.path()).unwrap();
        let mut command = integrated_command(
            Path::new("/bin/bash"),
            wrappers.path(),
            home.path(),
            Some(home.path()),
            None,
        )
        .unwrap();
        command.env("HOME", home.path());
        command.env("PS1", "$ ");
        let output = run_in_pty(
            command,
            &["echo rc=$PROCYON_TEST_RC\n", "false\n", "exit\n"],
        );

        assert!(
            output.contains("rc=loaded"),
            "bashrc not loaded: {output:?}"
        );
        assert!(
            output.contains("\x1b]133;C\x07"),
            "no command start: {output:?}"
        );
        assert!(
            output.contains("\x1b]133;D;1\x07"),
            "no failed end: {output:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn zsh_loads_the_persons_zshrc_and_reports_command_marks() {
        if !Path::new("/bin/zsh").exists() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join(".zshrc"),
            "export PROCYON_TEST_RC=loaded\n",
        )
        .unwrap();
        let wrappers = tempfile::tempdir().unwrap();
        write_wrappers(wrappers.path()).unwrap();
        let mut command = integrated_command(
            Path::new("/bin/zsh"),
            wrappers.path(),
            home.path(),
            Some(home.path()),
            None,
        )
        .unwrap();
        command.env("HOME", home.path());
        command.env("PROMPT", "$ ");
        let output = run_in_pty(
            command,
            &["echo rc=$PROCYON_TEST_RC\n", "false\n", "exit\n"],
        );

        assert!(output.contains("rc=loaded"), "zshrc not loaded: {output:?}");
        assert!(
            output.contains("\x1b]133;C\x07"),
            "no command start: {output:?}"
        );
        assert!(
            output.contains("\x1b]133;D;1\x07"),
            "no failed end: {output:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn fish_loads_the_persons_config_and_reports_command_marks() {
        let fish = [
            "/opt/homebrew/bin/fish",
            "/usr/local/bin/fish",
            "/usr/bin/fish",
        ]
        .into_iter()
        .map(Path::new)
        .find(|path| path.exists());
        let Some(fish) = fish else {
            return;
        };
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".config/fish");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("config.fish"),
            "set -gx PROCYON_TEST_RC loaded\n",
        )
        .unwrap();
        let wrappers = tempfile::tempdir().unwrap();
        write_wrappers(wrappers.path()).unwrap();
        let mut command =
            integrated_command(fish, wrappers.path(), home.path(), Some(home.path()), None)
                .unwrap();
        command.env("HOME", home.path());
        command.env("XDG_CONFIG_HOME", home.path().join(".config"));
        let output = run_in_pty(
            command,
            &["echo rc=$PROCYON_TEST_RC\n", "false\n", "exit\n"],
        );

        assert!(
            output.contains("rc=loaded"),
            "config.fish not loaded: {output:?}"
        );
        assert!(
            output.contains("\x1b]133;C\x07"),
            "no command start: {output:?}"
        );
        assert!(
            output.contains("\x1b]133;D;1\x07"),
            "no failed end: {output:?}"
        );
    }
}
