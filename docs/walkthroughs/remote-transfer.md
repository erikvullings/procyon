# Transfer files to and from a server

If you have not configured the server yet, start with
[Connect over SSH/SFTP](ssh-sftp.md). FTP/FTPS, WebDAV, S3-compatible, and other
providers can also appear in a pane when configured.

1. Open a local source folder in one pane and the remote connection in the
   other. For a download, reverse the panes. Confirm the destination is
   writable and that the selected files are the ones you intend to transfer.
2. Select files, then choose **Copy** (normally **F5**). Procyon's operation
   engine handles the transfer; inspect its progress in the Operations Centre.
3. If a destination name already exists, choose a conflict action deliberately.
   Do not choose **Move** unless you intend to remove the source after transfer.
4. Open the destination to verify the result. If the connection drops, inspect
   the operation's reported state before retrying; do not assume a partial
   transfer succeeded.

Transfers use the same workflow for local and remote locations, but a remote
server's permissions and capabilities still determine what is possible.
