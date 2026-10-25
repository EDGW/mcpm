#!/bin/sh
# Install, upgrade or uninstall a complete mcpm suite for the current Unix user.

set -eu

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

# Compare SemVer precedence against the first release using suite archives.
validate_version() {
    awk -v version="$1" 'BEGIN {
        if (version !~ /^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/) exit 1
        sub(/^v/, "", version); sub(/\+.*/, "", version)
        core = version; sub(/-.*/, "", core); split(core, parts, "\\.")
        split("0.1.0", minimum, "\\.")
        for (i = 1; i <= 3; i++) {
            if (parts[i] + 0 > minimum[i] + 0) exit 0
            if (parts[i] + 0 < minimum[i] + 0) exit 1
        }
        if (version == core) exit 0
        sub(/^[^-]*-/, "", version); count = split(version, pre, "\\.")
        split("dev.26w43c", lower, "\\.")
        for (i = 1; i <= 2; i++) {
            if (i > count) exit 1
            numeric = pre[i] ~ /^[0-9]+$/; lower_numeric = lower[i] ~ /^[0-9]+$/
            if (numeric != lower_numeric) exit numeric
            if (pre[i] == lower[i]) continue
            if (numeric) exit !(pre[i] + 0 > lower[i] + 0)
            exit !("x" pre[i] > "x" lower[i])
        }
        exit 0
    }' || fail 'supported versions start at v0.1.0-dev.26w43c; specify that release or a newer SemVer tag'
}

# The sole interpreter of both package and installation manifests on Unix.
read_manifest() {
    manifest_format='' manifest_version='' manifest_asset='' manifest_files=''
    [ -f "$1" ] && [ ! -L "$1" ] || fail "missing regular installation manifest: $1"
    while IFS='=' read -r key value || [ -n "$key$value" ]; do
        case "$key" in
            format) [ -z "$manifest_format" ] && [ "$value" = 1 ] || fail 'invalid or duplicate manifest format'; manifest_format=$value ;;
            version) [ -z "$manifest_version" ] && [ -n "$value" ] || fail 'empty or duplicate manifest version'; manifest_version=$value ;;
            asset) [ -z "$manifest_asset" ] && [ -n "$value" ] || fail 'empty or duplicate manifest asset'; manifest_asset=$value ;;
            file)
                case "$value" in mcpm|mcpmc|mcpm-*) ;; *) fail 'invalid manifest executable' ;; esac
                case "$value" in *[!a-zA-Z0-9-]*|*..*) fail 'invalid manifest executable' ;; esac
                case " $manifest_files " in *" $value "*) fail 'duplicate manifest executable' ;; esac
                manifest_files="${manifest_files:+$manifest_files }$value"
                ;;
            *) fail "unknown manifest field: $key" ;;
        esac
    done < "$1"
    [ "$manifest_format" = 1 ] && [ -n "$manifest_asset" ] && [ -n "$manifest_files" ] || fail 'invalid installation manifest'
    validate_version "$manifest_version"
}

write_manifest() {
    printf 'format=1\nversion=%s\nasset=%s\n' "$manifest_version" "$manifest_asset"
    for executable_name in $manifest_files; do printf 'file=%s\n' "$executable_name"; done
}

owned_link() {
    [ -L "$link_dir/$1" ] && [ "$(readlink "$link_dir/$1")" = "$install_dir/bin/$1" ]
}

cleanup() {
    result=$?
    trap - EXIT HUP INT TERM
    set +e
    if [ "$committed" = no ] && [ "$transaction" = yes ]; then
        printf 'Restoring previous installation...\n' >&2
        if [ "$new_bin" = yes ]; then rm -rf "${install_dir:?}/bin" || result=1; fi
        if [ "$new_manifest" = yes ]; then rm -f "$install_dir/manifest"; fi
        if [ -d "$stage/old-bin" ]; then
            if [ ! -e "$install_dir/bin" ]; then mv "$stage/old-bin" "$install_dir/bin" || result=1; else result=1; fi
        fi
        if [ -f "$stage/old-manifest" ]; then mv "$stage/old-manifest" "$install_dir/manifest" || result=1; fi
        for executable_name in $created_links; do
            if owned_link "$executable_name"; then rm -f "$link_dir/$executable_name"; fi
        done
    fi
    if [ -n "$stage" ]; then
        # Preserve a backup if restoring it failed.
        if [ -e "$stage/old-bin" ] || [ -e "$stage/old-manifest" ]; then
            if [ "$committed" = yes ]; then rm -rf "$stage"; else printf 'Backup retained at %s\n' "$stage" >&2; fi
        else rm -rf "$stage"; fi
    fi
    rmdir "$install_dir/.install-lock" 2>/dev/null || :
    rmdir "$install_dir" 2>/dev/null || :
    exit "$result"
}

uninstall() {
    if [ ! -e "$install_dir/manifest" ]; then
        printf 'No managed mcpm installation at %s. Legacy installations are not removed.\n' "$install_dir"
        return
    fi
    read_manifest "$install_dir/manifest"
    for executable_name in $manifest_files; do
        if owned_link "$executable_name"; then rm -f "$link_dir/$executable_name"; fi
        [ ! -d "$install_dir/bin/$executable_name" ] || fail "expected a file: $executable_name"
        rm -f "$install_dir/bin/$executable_name"
    done
    rm -f "$install_dir/manifest"
    rmdir "$install_dir/bin" 2>/dev/null || :
    printf 'Removed mcpm executables and owned links. Configuration, caches and logs were kept.\n'
}

main() {
    [ "$#" -eq 1 ] || fail 'usage: install.sh VERSION | --uninstall'
    [ -n "${HOME-}" ] || fail 'HOME is required'
    install_dir=$HOME/.local/share/mcpm
    link_dir=$HOME/.local/bin
    stage='' transaction=no committed=no new_bin=no new_manifest=no created_links=''
    if [ "$1" != --uninstall ]; then validate_version "$1"; fi
    mkdir -p "$install_dir"
    [ ! -L "$install_dir" ] && [ ! -L "$install_dir/bin" ] || fail 'installation directories must not be symbolic links'
    mkdir "$install_dir/.install-lock" 2>/dev/null || fail "another installer is running (lock: $install_dir/.install-lock)"
    trap cleanup EXIT
    trap 'exit 1' HUP INT TERM
    if [ "$1" = --uninstall ]; then uninstall; return; fi
    version=$1
    case "$(uname -s)" in Darwin) platform=macos ;; Linux) platform=linux ;; *) fail 'supported systems: macOS and Linux' ;; esac
    case "$(uname -m)" in x86_64|amd64) architecture=x86_64 ;; arm64|aarch64) architecture=arm64 ;; *) fail 'supported architectures: x86_64 and ARM64' ;; esac
    asset=mcpm-$platform-$architecture
    if [ "$platform" = linux ]; then asset=$asset-musl; fi
    old_files=
    if [ -e "$install_dir/manifest" ]; then
        read_manifest "$install_dir/manifest"
        old_files=$manifest_files
    elif [ -e "$install_dir/bin" ]; then fail 'existing bin directory has no installation manifest'; fi
    if [ -d "$install_dir/bin" ]; then
        for installed_file in "$install_dir/bin/"* "$install_dir/bin/".[!.]* "$install_dir/bin/"..?*; do
            [ -e "$installed_file" ] || [ -L "$installed_file" ] || continue
            case " $old_files " in *" ${installed_file##*/} "*) ;; *) fail "unmanaged file in installation bin directory: $installed_file" ;; esac
            [ -f "$installed_file" ] && [ ! -L "$installed_file" ] || fail "expected a regular installed executable: $installed_file"
        done
    fi
    stage=$(mktemp -d "$install_dir/.stage.XXXXXXXX")
    url=https://github.com/EDGW/mcpm/releases/download/$version/$asset.tar.gz
    printf 'Downloading %s...\n' "$url"
    curl --fail --silent --show-error --location "$url" --output "$stage/package.tar.gz"
    curl --fail --silent --show-error --location "$url.sha256" --output "$stage/checksum"
    expected=$(awk 'NR == 1 {print $1}' "$stage/checksum")
    [ "${#expected}" -eq 64 ] || fail 'invalid SHA-256 checksum'
    case "$expected" in *[!0-9a-fA-F]*) fail 'invalid SHA-256 checksum' ;; esac
    if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$stage/package.tar.gz"); else actual=$(shasum -a 256 "$stage/package.tar.gz"); fi
    [ "${actual%% *}" = "$expected" ] || fail 'archive SHA-256 mismatch'
    tar -tzf "$stage/package.tar.gz" > "$stage/entries"
    while IFS= read -r entry; do
        case "$entry" in manifest|bin/mcpm|bin/mcpmc|bin/mcpm-*) ;; *) fail "unexpected archive entry: $entry" ;; esac
        entry_name=${entry#bin/}
        case "$entry_name" in */*|*..*|*[!a-zA-Z0-9-]*) fail 'invalid archive entry' ;; esac
    done < "$stage/entries"
    [ -z "$(sort "$stage/entries" | uniq -d)" ] || fail 'duplicate archive entries'
    tar -tvzf "$stage/package.tar.gz" > "$stage/types"
    awk 'substr($0, 1, 1) != "-" {exit 1}' "$stage/types" || fail 'archive must contain only regular files'
    mkdir "$stage/package"
    tar -xzf "$stage/package.tar.gz" -C "$stage/package"
    read_manifest "$stage/package/manifest"
    [ "$manifest_version" = "$version" ] && [ "$manifest_asset" = "$asset" ] || fail 'archive version or platform does not match request'
    [ ! -L "$stage/package/bin" ] || fail 'archive bin must be a directory'
    for executable_name in $manifest_files; do
        executable=$stage/package/bin/$executable_name
        [ -f "$executable" ] && [ ! -L "$executable" ] || fail "missing regular executable: $executable_name"
        chmod 755 "$executable"
        [ "$("$executable" -V)" = "$executable_name ${version#v}" ] || fail "executable version mismatch: $executable_name"
        "$executable" --help >/dev/null
        if [ -e "$link_dir/$executable_name" ] || [ -L "$link_dir/$executable_name" ]; then
            owned_link "$executable_name" || fail "command already belongs to another installation: $link_dir/$executable_name"
        fi
    done
    # Every archive file must be declared; preserve a narrowly owned bin directory.
    for packaged_file in "$stage/package/bin/"*; do
        case " $manifest_files " in *" ${packaged_file##*/} "*) ;; *) fail 'undeclared package executable' ;; esac
    done
    write_manifest > "$stage/new-manifest"
    mkdir -p "$link_dir"
    transaction=yes
    if [ -d "$install_dir/bin" ]; then mv "$install_dir/bin" "$stage/old-bin"; fi
    if [ -f "$install_dir/manifest" ]; then mv "$install_dir/manifest" "$stage/old-manifest"; fi
    new_bin=yes
    mv "$stage/package/bin" "$install_dir/bin"
    for executable_name in $manifest_files; do
        if ! owned_link "$executable_name"; then
            ln -s "$install_dir/bin/$executable_name" "$link_dir/$executable_name"
            created_links="${created_links:+$created_links }$executable_name"
        fi
    done
    new_manifest=yes
    mv "$stage/new-manifest" "$install_dir/manifest"
    committed=yes
    for executable_name in $old_files; do
        case " $manifest_files " in *" $executable_name "*) ;; *) if owned_link "$executable_name"; then rm -f "$link_dir/$executable_name"; fi ;; esac
    done
    printf 'Installed %s to %s. Run: mcpm --help\n' "$version" "$install_dir/bin"
    # Keep shell variables literal in the command printed for the user.
    # shellcheck disable=SC2016
    case ":${PATH-}:" in *":$link_dir:"*) ;; *) printf 'Add ~/.local/bin to your shell PATH, for example in ~/.zshrc or ~/.bashrc:\n  export PATH="$HOME/.local/bin:$PATH"\n' ;; esac
    resolved=$(command -v mcpm || :)
    if [ -n "$resolved" ] && [ "$resolved" != "$link_dir/mcpm" ]; then
        printf 'An existing command takes precedence: %s. Review PATH or remove the old installation explicitly.\n' "$resolved"
    fi
}

main "$@"
