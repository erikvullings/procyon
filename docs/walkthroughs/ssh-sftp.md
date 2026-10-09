# Connect over SSH/SFTP

Procyon's **SSH** connection browses files over SFTP. You need an account on
a server that actually enables SFTP; an SSH terminal add-on alone does not
necessarily provide it. For a Home Assistant host, check the add-on's
documentation before assuming file browsing is available.

1. Open **Connections** and add an **SSH** connection. Give it a recognizable
   name, then enter the server hostname, SSH port (usually `22`), username,
   and a supported password or private-key authentication method. Use your
   own host details, not somebody else's example credentials.
2. Test the connection. On first use, compare the presented host-key
   fingerprint with a value obtained from your server administrator or a
   trusted console before accepting it. If the key changes later, investigate
   rather than blindly replacing the trusted fingerprint.
3. Connect and open the SFTP location in a pane. Browse folders as you would
   locally; your saved connection is available again without embedding
   credentials in a path.

If the server supports a shell, the **embedded terminal** can open a remote
session for an SSH-backed location. That is optional and separate from SFTP
file browsing. For a copy example, continue with [Remote transfer](remote-transfer.md).
Never include passwords, private keys, or real host addresses in shared guides
or screen recordings.
