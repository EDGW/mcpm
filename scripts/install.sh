#!/bin/sh
# Install a tagged mcpm release on macOS/Linux, or remove its executable.

set -eu

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

with_install_permissions() {
    if [ -w "$install_dir" ] || [ "$(id -u)" -eq 0 ]; then
        "$@"
    else
        command -v sudo >/dev/null 2>&1 || fail "sudo is required to write to $install_dir"
        sudo "$@"
    fi
}

cleanup() {
    rm -rf "$temporary_dir"
    if [ -n "${staged_executable-}" ]; then
        with_install_permissions rm -f "$staged_executable"
    fi
}

main() {
    [ "$#" -eq 1 ] || fail 'usage: install.sh VERSION | --uninstall'
    install_dir=/usr/local/bin
    executable=$install_dir/mcpm

    case "$(uname -s)" in
        Darwin) platform=macos ;;
        Linux) platform=linux ;;
        *) fail 'supported systems: macOS and Linux' ;;
    esac

    if [ "$1" = --uninstall ]; then
        if [ -e "$executable" ] || [ -L "$executable" ]; then
            with_install_permissions rm -f "$executable"
        fi
        printf 'Removed %s. Configuration and cache files were kept.\n' "$executable"
        return
    fi

    version=$1
    case "$version" in
        v[0-9]*) ;;
        *) fail 'version must be a release tag such as v0.1.0-dev.26w43b' ;;
    esac
    case "$version" in
        *[!a-zA-Z0-9._-]*) fail 'invalid release tag' ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) architecture=x86_64 ;;
        arm64|aarch64) architecture=arm64 ;;
        *) fail 'supported architectures: x86_64 and ARM64' ;;
    esac

    asset=mcpm-$platform-$architecture
    if [ "$platform" = linux ]; then
        asset=$asset-musl
    fi
    url=https://github.com/EDGW/mcpm/releases/download/$version/$asset
    temporary_dir=$(mktemp -d)
    trap cleanup EXIT
    trap 'exit 1' HUP INT TERM

    printf 'Downloading %s...\n' "$asset"
    curl --fail --silent --show-error --location "$url" --output "$temporary_dir/mcpm"
    [ -s "$temporary_dir/mcpm" ] || fail 'downloaded executable is empty'
    chmod 755 "$temporary_dir/mcpm"
    "$temporary_dir/mcpm" -V

    with_install_permissions mkdir -p "$install_dir"
    [ ! -d "$executable" ] || fail "$executable is a directory"
    staged_executable=$(with_install_permissions mktemp "$install_dir/.mcpm.XXXXXXXX")
    with_install_permissions install -m 755 "$temporary_dir/mcpm" "$staged_executable"
    with_install_permissions mv -f "$staged_executable" "$executable"
    staged_executable=
    printf 'Installed %s to %s. Run: mcpm --help\n' "$version" "$executable"
}

main "$@"
