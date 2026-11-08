#!/bin/sh
set -eu

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

register_shell_file() (
    startup=$1
    target=$startup
    links=0
    while [ -L "$target" ]; do
        links=$((links + 1))
        [ "$links" -le 40 ] || fail "Could not resolve $startup. Fix the startup file symlink loop and rerun the installer."
        link=$(readlink "$target") || fail "Could not read $startup. Check the startup file symlink."
        case "$link" in
            /*) target=$link ;;
            *) target=$(dirname "$target")/$link ;;
        esac
        [ -e "$target" ] || fail "Could not resolve $startup. Fix its missing symlink target and rerun the installer."
    done
    parent=$(dirname "$target")
    mkdir -p "$parent" || fail "Could not create $parent. Check folder permissions."
    backup=
    if [ -e "$target" ]; then
        safe=$(find "$target" -prune -type f -user "$(id -u)" ! -perm -002 ! -perm -020 -print)
        [ -n "$safe" ] || fail "Could not update $startup safely. Use a regular file owned by you and not writable by other users."
        if grep -F -x -e '# >>> ssh-key-man >>>' -e '# <<< ssh-key-man <<<' "$target" >/dev/null; then
            awk '/^# >>> ssh-key-man >>>$/ { active=1 } active { print } /^# <<< ssh-key-man <<<$/ { active=0 }' "$target" > "$work_dir/existing-block"
            cmp -s "$work_dir/shell-block" "$work_dir/existing-block" || fail "The ssh-key-man block in $startup is incomplete or was edited. Repair or remove it, then rerun the installer."
            printf 'SSH agent settings already exist in %s.\n' "$startup"
            exit 0
        fi
        backup=$(mktemp "$target.skm-backup.XXXXXX") || fail "Could not back up $startup. Check folder permissions and free disk space."
        cat "$target" > "$backup" || fail "Could not back up $startup. Check file permissions and free disk space."
    fi
    next=$(mktemp "$parent/.skm-shell.XXXXXX") || fail "Could not prepare $startup. Check folder permissions and free disk space."
    trap 'rm -f "$next"' EXIT
    trap 'exit 1' HUP INT TERM
    if [ -n "$backup" ]; then
        cp -p "$target" "$next" || fail "Could not copy $startup. Check file permissions and free disk space."
        printf '\n' >> "$next"
    fi
    cat "$work_dir/shell-block" >> "$next" || fail "Could not write settings for $startup. Check free disk space."
    if [ -n "$backup" ]; then
        cmp -s "$target" "$backup" || fail "$startup changed during setup. Rerun the installer to preserve those edits."
        mv -f "$next" "$target" || fail "Could not save $startup. Check folder permissions. Backup: $backup"
    else
        ln "$next" "$target" || fail "Could not create $startup. Check folder permissions or rerun the installer if this file was just created."
    fi
    printf 'Added SSH agent settings to %s.\n' "$startup"
    [ -z "$backup" ] || printf 'Backup: %s\n' "$backup"
)

setup_shell() (
    shell_path=${SHELL:-}
    case "${HOME:-}" in
        /*) ;;
        *) fail 'HOME must be an absolute path. Set it to your home folder and rerun the installer.' ;;
    esac
    # shortcut: startup probes rely on ssh-add returning; use a bounded probe if stalled agents occur.
    case "${shell_path##*/}" in
        bash|zsh)
            cat > "$work_dir/shell-block" <<'EOF'
# >>> ssh-key-man >>>
if [ -z "${SSH_AUTH_SOCK:-}" ] || [ ! -S "$SSH_AUTH_SOCK" ] || ! (ssh-add -l >/dev/null 2>&1; test "$?" -le 1); then
    export SSH_AUTH_SOCK="$HOME/.ssh/agent.sock"
fi
# <<< ssh-key-man <<<
EOF
            if [ "${shell_path##*/}" = zsh ]; then
                config_dir=${ZDOTDIR:-$HOME}
                case "$config_dir" in /*) ;; *) fail 'ZDOTDIR must be an absolute path. Fix it or unset it and rerun the installer.' ;; esac
                register_shell_file "$config_dir/.zshrc"
            else
                register_shell_file "$HOME/.bashrc"
                profile=$HOME/.bash_profile
                for name in .bash_profile .bash_login .profile; do
                    if [ -e "$HOME/$name" ] || [ -L "$HOME/$name" ]; then
                        profile=$HOME/$name
                        break
                    fi
                done
                register_shell_file "$profile"
            fi
            ;;
        fish)
            cat > "$work_dir/shell-block" <<'EOF'
# >>> ssh-key-man >>>
if not set -q SSH_AUTH_SOCK; or not test -S "$SSH_AUTH_SOCK"; or not sh -c 'ssh-add -l >/dev/null 2>&1; test "$?" -le 1'
    set -gx SSH_AUTH_SOCK "$HOME/.ssh/agent.sock"
end
# <<< ssh-key-man <<<
EOF
            config_dir=${XDG_CONFIG_HOME:-$HOME/.config}
            case "$config_dir" in /*) ;; *) fail 'XDG_CONFIG_HOME must be an absolute path. Fix it or unset it and rerun the installer.' ;; esac
            register_shell_file "$config_dir/fish/conf.d/ssh-key-man.fish"
            ;;
        *) fail 'Could not detect Bash, Zsh, or Fish from SHELL. Set SHELL to your shell path and rerun the installer.' ;;
    esac
    printf 'Open a new terminal to use the agent settings.\n'
)

install_skm() {
    for tool in curl tar uname mktemp awk grep mkdir mv chmod ln rm cat cmp cp dirname find id readlink; do
        command -v "$tool" >/dev/null 2>&1 || fail "Could not find $tool. Install it and try again."
    done
    if command -v sha256sum >/dev/null 2>&1; then
        checksum=sha256sum
    elif command -v shasum >/dev/null 2>&1; then
        checksum=shasum
    else
        fail 'Could not find sha256sum or shasum. Install a SHA256 checksum tool and try again.'
    fi
    case "$(uname -s)" in
        Linux) platform=unknown-linux-musl ;;
        Darwin) platform=apple-darwin ;;
        *) fail 'This installer supports Linux, macOS, and WSL. Use WSL on Windows.' ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) architecture=x86_64 ;;
        aarch64|arm64) architecture=aarch64 ;;
        *) fail 'This CPU is not supported. Build ssh-key-man from source with Cargo.' ;;
    esac
    version=${SKM_VERSION:-latest}
    case "$version" in
        *[!a-zA-Z0-9._-]*|'') fail 'Invalid SKM_VERSION. Use a release tag such as v0.1.0.' ;;
    esac
    if [ -n "${SKM_INSTALL_DIR:-}" ]; then
        install_dir=$SKM_INSTALL_DIR
    else
        [ -n "${HOME:-}" ] || fail 'HOME is not set. Set SKM_INSTALL_DIR to an installation folder.'
        install_dir=$HOME/.local/bin
    fi
    case "$install_dir" in
        /*) ;;
        *) fail 'SKM_INSTALL_DIR must be an absolute path.' ;;
    esac
    mkdir -p "$install_dir" || fail "Could not create $install_dir. Choose a writable SKM_INSTALL_DIR."
    [ -w "$install_dir" ] || fail "Could not write to $install_dir. Fix its permissions or choose another SKM_INSTALL_DIR."
    for name in ssh-key-man skm; do
        [ ! -d "$install_dir/$name" ] || fail "$install_dir/$name is a directory. Move it before installing."
    done
    work_dir=$(mktemp -d "$install_dir/.skm-install.XXXXXX") || fail "Could not prepare installation in $install_dir. Check permissions and free disk space."
    trap 'rm -rf "$work_dir"' EXIT
    trap 'exit 1' HUP INT TERM
    archive=ssh-key-man-$architecture-$platform.tar.gz
    base=https://github.com/zainphp/ssh-key-man/releases
    if [ "$version" = latest ]; then
        download=$base/latest/download
    else
        download=$base/download/$version
    fi
    printf 'Downloading %s (%s)...\n' "$archive" "$version"
    for asset in "$archive" "$archive.sha256"; do
        curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 \
            "$download/$asset" -o "$work_dir/$asset" || fail "Could not download $asset. Check your connection and that the requested GitHub release exists, then retry."
    done
    expected=$(awk 'NR == 1 { print $1 }' "$work_dir/$archive.sha256")
    case "$expected" in
        *[!a-fA-F0-9]*|'') fail 'The release checksum is invalid. Retry the download or report the broken release.' ;;
    esac
    [ "${#expected}" -eq 64 ] || fail 'The release checksum is invalid. Retry the download or report the broken release.'
    if [ "$checksum" = sha256sum ]; then
        actual=$(sha256sum "$work_dir/$archive" | awk '{ print $1 }')
    else
        actual=$(shasum -a 256 "$work_dir/$archive" | awk '{ print $1 }')
    fi
    [ "$actual" = "$expected" ] || fail 'The download checksum does not match. Your existing installation was kept. Retry the download.'
    tar -xzf "$work_dir/$archive" -C "$work_dir" ssh-key-man || fail 'Could not unpack the release. Your existing installation was kept. Retry the download.'
    [ -f "$work_dir/ssh-key-man" ] && [ ! -L "$work_dir/ssh-key-man" ] || fail 'The release does not contain a regular ssh-key-man binary. Report the broken release.'
    chmod 755 "$work_dir/ssh-key-man"
    installed_version=$("$work_dir/ssh-key-man" --version) || fail 'The downloaded binary could not run. Your existing installation was kept. Build from source or report your operating system and CPU.'
    current_version=$("$install_dir/ssh-key-man" --version 2>/dev/null) || current_version=
    if [ "$current_version" = "$installed_version" ]; then
        printf '%s is already installed.\n' "$installed_version"
    else
        mv -f "$work_dir/ssh-key-man" "$install_dir/ssh-key-man" || fail "Could not replace $install_dir/ssh-key-man. Check file permissions and try again."
        printf 'Installed %s in %s\n' "$installed_version" "$install_dir"
    fi
    if [ ! -L "$install_dir/skm" ] || [ "$(readlink "$install_dir/skm")" != ssh-key-man ]; then
        ln -s ssh-key-man "$work_dir/skm"
        mv -f "$work_dir/skm" "$install_dir/skm" || fail "Could not update $install_dir/skm. Check file permissions and rerun the installer."
    fi
    setup_shell
    case ":${PATH:-}:" in
        *:"$install_dir":*) printf 'Run skm doctor to check OpenSSH and your agent.\n' ;;
        *) printf 'Add %s to PATH, then run skm doctor.\n' "$install_dir" ;;
    esac
}

install_skm
