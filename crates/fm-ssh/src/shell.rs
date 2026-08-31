//! A remote interactive shell channel over SSH (task 0105).
//!
//! Wraps a `russh` client channel after a PTY has been requested and either
//! a login shell (`RequestShell`) or a `cd <dir> && exec $SHELL -l` command
//! (`Exec`) has started running on it - the same "start remote, cd, replace
//! with a login shell" trick interactive SSH-terminal tools use, since SSH's
//! `exec` channel takes a single opaque command string, not a `cwd` field.

use std::sync::Arc;

use russh::client::Msg;
use russh::{ChannelMsg, ChannelReadHalf, ChannelWriteHalf};

use crate::error::SshError;

/// One event read from a remote shell channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteShellEvent {
    /// Bytes of terminal output (stdout and stderr, merged - the remote PTY
    /// itself merges them, matching a local PTY's behavior).
    Data(Vec<u8>),
    /// The remote command/shell exited and the channel closed.
    Closed,
}

/// The write half of a remote shell channel: sending input and resize
/// requests. Cheaply cloneable (an `Arc` around the underlying `russh` write
/// half) so a caller can hold one copy for input while a separate task owns
/// [`RemoteShellReader`].
#[derive(Clone, Debug)]
pub struct RemoteShellWriter {
    write: Arc<ChannelWriteHalf<Msg>>,
}

impl RemoteShellWriter {
    pub(crate) fn new(write: ChannelWriteHalf<Msg>) -> Self {
        Self {
            write: Arc::new(write),
        }
    }

    /// Sends bytes to the remote process's stdin.
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.write
            .data_bytes(data.to_vec())
            .await
            .map_err(|error| SshError::Session(error.to_string()))
    }

    /// Reports a new terminal size to the remote PTY.
    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.write
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(|error| SshError::Session(error.to_string()))
    }
}

/// The read half of a remote shell channel: exclusively owned, since only
/// one task can await the next [`ChannelMsg`] at a time.
#[derive(Debug)]
pub struct RemoteShellReader {
    read: ChannelReadHalf,
}

impl RemoteShellReader {
    pub(crate) fn new(read: ChannelReadHalf) -> Self {
        Self { read }
    }

    /// Waits for the next output chunk or channel close. Returns `None` once
    /// the channel is fully gone (matches [`ChannelReadHalf::wait`]); a
    /// caller should stop polling after either `None` or one
    /// [`RemoteShellEvent::Closed`].
    pub async fn next(&mut self) -> Option<RemoteShellEvent> {
        loop {
            return match self.read.wait().await? {
                ChannelMsg::Data { data } => Some(RemoteShellEvent::Data(data.to_vec())),
                ChannelMsg::ExtendedData { data, .. } => {
                    Some(RemoteShellEvent::Data(data.to_vec()))
                }
                ChannelMsg::Eof | ChannelMsg::Close => Some(RemoteShellEvent::Closed),
                _ => continue,
            };
        }
    }
}

/// A freshly opened remote shell channel, split into independently owned
/// read/write halves.
#[derive(Debug)]
pub struct RemoteShellChannel {
    /// Exclusively-owned output stream.
    pub reader: RemoteShellReader,
    /// Cheaply cloneable input/resize handle.
    pub writer: RemoteShellWriter,
}

impl RemoteShellChannel {
    pub(crate) fn new(read: ChannelReadHalf, write: ChannelWriteHalf<Msg>) -> Self {
        Self {
            reader: RemoteShellReader::new(read),
            writer: RemoteShellWriter::new(write),
        }
    }
}

/// Quotes `value` as a single POSIX shell word, so it can be embedded in a
/// `cd <value>` command sent over an SSH `exec` channel without an awkward
/// or attacker-controlled path (spaces, quotes, shell metacharacters)
/// breaking out of the intended command. SSH's `exec` channel has no argv -
/// only one opaque command string - so this is the same defense a shell's
/// own quoting provides, applied client-side before the string ever leaves
/// this process.
pub(crate) fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_wraps_a_plain_path_in_single_quotes() {
        assert_eq!(shell_quote("/home/erik/projects"), "'/home/erik/projects'");
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("/tmp/o'brien"), "'/tmp/o'\\''brien'");
    }

    #[test]
    fn shell_quote_treats_metacharacters_as_inert_inside_single_quotes() {
        // Everything except a single quote is inert between single quotes in
        // a POSIX shell, so `;`, `$`, backticks etc. need no extra escaping.
        assert_eq!(shell_quote("/tmp/a; rm -rf ~"), "'/tmp/a; rm -rf ~'");
    }
}
