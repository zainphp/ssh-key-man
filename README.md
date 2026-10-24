# ssh-key-man

Create SSH keys, manage host aliases, add public keys to servers, and manage your SSH agent from a CLI or a keyboard-driven terminal interface. Both `ssh-key-man` and `skm` run the same application.

Requires Linux, macOS, or WSL, the current stable Rust toolchain (2021 edition), and OpenSSH's `ssh-keygen`, `ssh-agent`, `ssh-add`, and `ssh-copy-id` in `PATH`. Debian/Ubuntu users can install `build-essential openssh-client`; macOS users need the Command Line Tools and `ssh-copy-id` (for example, `brew install ssh-copy-id`). Native Windows is not supported.

```sh
cargo install --path . --locked
skm --help
skm
```

Running without a subcommand opens the Hosts page. K opens Keys and H opens Hosts. Up/Down selects a list item, and Left/Right switches pages directly while an item is selected. From the first item, Up focuses the page tabs; from the last item, Down focuses the actions. In either picker, Left/Right chooses an option and Enter activates it. N creates a new host or key on the current page. Enter loads the selected key; D deletes its local files after confirmation. E/D edit or delete a host. Each page keeps its selection. The main screens show only the tabs, action shortcuts, and Enter's action; navigation instructions are documented here rather than repeated in the TUI.

Esc quits from a page. In a form or confirmation, Esc cancels that step. Tab does not switch pages; it remains available for moving between form fields, alongside Up/Down. Key creation, server setup, and deletion choices also support arrow keys. Delete confirmations select Cancel by default.

The Keys page has a key list and selected-key details. Results, warnings, and errors appear only when needed in a message popup: Enter dismisses it and Esc quits. There is no persistent Status section. Pressing Enter on a selected key closes the TUI and loads the key into the SSH agent; pressing Enter on a host closes the TUI and starts SSH in the current terminal. The TUI does not reopen after either action, including a failed load or connection. Resize a small terminal to at least 60 columns and 18 rows.

```sh
skm hosts               # Add, edit, or delete SSH host aliases
skm gen                 # Create a key and optionally add its public key to a server
skm load id_ed25519     # Load one private key from ~/.ssh/
skm load                # Load all discovered private keys
skm doctor              # Find/start an agent and update ~/.ssh/agent.sock
skm init zsh            # Print shell configuration (bash and fish also supported)
```

Open the Hosts page with H or the arrow-key page picker, or run `skm hosts` directly to manage `~/.ssh/config`. Press Enter on a selected alias to open an SSH session in the current terminal. The app exits when the SSH session ends. Press N to add an alias, E to edit or rename the selected alias, and D to delete it after confirmation. Up/Down or Tab moves between alias, hostname, user, port, and key file; Ctrl+U clears a field. Enter saves and Escape cancels. Leave optional fields blank to use your SSH settings and defaults. Key paths such as `~/.ssh/id_ed25519` and paths containing spaces are supported.

For example, save alias `work`, hostname `server.example.com`, user `alice`, port `2222`, and key file `~/.ssh/id_work`. Then connect with:

```sh
ssh work
scp report.txt work:~/
```

The editor preserves other blocks, comments, and extra options such as `ProxyJump`. Before replacing an existing config, it saves a private `~/.ssh/config.skm-backup-*` copy. Saves are atomic, use permissions 600, and refuse detected external edits; reopen Hosts after changing the file elsewhere. To restore a backup, copy it over `~/.ssh/config` and keep permissions 600.

Only single-alias blocks with one value per managed setting can be edited or deleted here. Wildcards, multi-alias blocks, and repeated settings are read-only. `Match` sections and `Include` directives are preserved; included files are not opened or expanded. Symlinked config files must be edited directly. Values shown are the settings written in each block, not the effective settings after all SSH rules. Existing earlier rules can take priority; use `ssh -G work` to inspect the effective settings. New aliases go before existing rules, with a scope reset when needed to preserve global defaults for other hosts.

On the Keys page, D opens Delete with Cancel selected by default. Confirm deletion to permanently remove the selected private key and its matching public key file, if present. A public-only selection removes only that public key file. Files are checked against the selected fingerprint before removal; changed files, symlinks, and mismatched public keys are kept. A loaded private key is also removed from the current responding SSH agent before its files are deleted. Other agents, host aliases, and keys already added to servers are unchanged. There is no undo for deleted key files.

Generation supports Ed25519 (default) and RSA 4096. Choose a filename in `~/.ssh/`. Existing private or public files are never overwritten. OpenSSH asks for an optional passphrase using its own hidden terminal prompt; the application never accepts a passphrase argument. The TUI temporarily returns to the normal terminal for passphrase and server login prompts.

After creating a key, choose whether to add its public key on `user@hostname`, with an optional port. `ssh-copy-id` handles login, permissions, and duplicate public keys on the server. Verify the server's host fingerprint against a trusted source when SSH asks. The app preserves your OpenSSH configuration and host verification settings. It never sends a private key to the server. The saved key is loaded into the agent even if adding the public key to the server fails. Adding a public key to a server needs password login or another working authentication method; it cannot bypass server policy.

Add this line to `~/.zshrc` or `~/.bashrc`:

```sh
export SSH_AUTH_SOCK="$HOME/.ssh/agent.sock"
```

For Fish, add this to `~/.config/fish/config.fish`:

```fish
set -gx SSH_AUTH_SOCK "$HOME/.ssh/agent.sock"
```

Loading a key finds or starts an SSH agent and creates the link automatically. Use `skm doctor` only if you need to repair the connection. A child process cannot change its parent shell's environment. `skm init` prints configuration only; it does not edit startup files. After a reboot, run `skm doctor` or load a key to find or start a new agent.

Agent selection checks `SSH_AUTH_SOCK`, then the stable link, then `/tmp/ssh-*/agent.*` (and the system temporary directory on macOS), before starting `ssh-agent`. Each socket must belong to the current user and answer `ssh-add`. Probes time out after three seconds. Multiple instances serialize socket updates with a file lock. A regular file at `~/.ssh/agent.sock` is never replaced. The stable link follows whichever verified agent was selected, including a forwarded agent if that is your active `SSH_AUTH_SOCK`.

The key table shows OpenSSH private keys, including encrypted keys, and standalone `.pub` files. SHA256 fingerprints identify loaded keys without comparing comments or filenames. Public-only keys can be inspected but cannot be loaded. Private keys must be regular files owned by you with permissions 600 (or stricter). Symlinked keys are not followed. Legacy PEM keys are reported with conversion instructions; convert them with `ssh-keygen -p -o -f <filename>` before loading them through this tool. Keep `~/.ssh/` owned by you and not writable by other users.

Do not delete a `.skm-key-*` directory left by an interrupted generation until you have checked for a generated private key inside it. A successful generation removes its staging directory. If adding the public key fails, retry manually with `ssh-copy-id -i ~/.ssh/<filename>.pub -p 22 user@hostname`; if agent loading fails, use `skm load <filename>`.

Development checks:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
python3 tests/terminal.py
cargo build --release --locked
```

Integration tests require OpenSSH. They create disposable keys, including an encrypted key, and isolated temporary agents. They check actual fingerprints, duplicate prevention, socket links, file permissions, mismatched public keys, both binary names, and shell output. Tests never add a key to a remote server. CI runs these checks on Linux and macOS.
