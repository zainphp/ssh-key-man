#!/bin/sh
set -eu

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

install_skm() {
    for tool in curl tar uname mktemp awk mkdir mv chmod ln rm; do
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
    ln -s ssh-key-man "$work_dir/skm"
    mv -f "$work_dir/ssh-key-man" "$install_dir/ssh-key-man" || fail "Could not replace $install_dir/ssh-key-man. Check file permissions and try again."
    mv -f "$work_dir/skm" "$install_dir/skm" || fail "Could not update $install_dir/skm. Check file permissions and rerun the installer."
    printf 'Installed %s in %s\n' "$installed_version" "$install_dir"
    case ":${PATH:-}:" in
        *:"$install_dir":*) printf 'Run skm doctor to check OpenSSH and your agent.\n' ;;
        *) printf 'Add %s to PATH, then run skm doctor.\n' "$install_dir" ;;
    esac
}

install_skm
