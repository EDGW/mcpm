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

# Local ownership record. The package's manifest.json is validated by mcpm-tools.
read_manifest() {
    manifest_format='' manifest_version='' manifest_asset='' manifest_files=''
    [ -f "$1" ] && [ ! -L "$1" ] || fail "missing regular installation manifest: $1"
    while IFS='=' read -r key value || [ -n "$key$value" ]; do
        case "$key" in
            format) [ -z "$manifest_format" ] && [ "$value" = 1 ] || fail 'invalid or duplicate manifest format'; manifest_format=$value ;;
            version) [ -z "$manifest_version" ] && [ -n "$value" ] || fail 'empty or duplicate manifest version'; manifest_version=$value ;;
            asset) [ -z "$manifest_asset" ] && [ -n "$value" ] || fail 'empty or duplicate manifest asset'; manifest_asset=$value ;;
            file)
                valid_filename "$value" || fail 'invalid manifest filename'
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
    for name in $manifest_files; do printf 'file=%s\n' "$name"; done
}

valid_filename() {
    case "$1" in
        ''|[!a-zA-Z0-9]*|*[!a-zA-Z0-9._-]*|*.) return 1 ;;
    esac
}

executable_name() {
    case "$1" in
        mcpm|mcpmc|mcpmd|mcpm-?*)
            case "$1" in *[!a-zA-Z0-9-]*) return 1 ;; esac ;;
        *) return 1 ;;
    esac
}

confirm_replacement() {
    printf 'Existing mcpm installation at %s. Replace and delete all previous contents? [y/N] ' "$install_dir" >&2
    answer=''
    if [ -t 0 ]; then
        IFS= read -r answer || :
    elif ( : </dev/tty ) 2>/dev/null; then
        IFS= read -r answer </dev/tty || :
    else
        IFS= read -r answer || :
    fi
    case "$answer" in [yY]|[yY][eE][sS]) return 0 ;; esac
    printf 'Installation cancelled; existing files were not changed.\n'
    return 1
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
        if [ "$publishing" = yes ]; then
            rm -rf "${install_dir:?}/bin" || result=1
            rm -f "$install_dir/manifest" "$install_dir/manifest.json" || result=1
        fi
        if [ "$record_changed" = yes ]; then
            if [ -f "$stage/old-record" ]; then
                if cp -p "$stage/old-record" "$record_temp" && mv -f "$record_temp" "$record_path"; then
                    rm -f "$stage/old-record"
                else result=1; fi
            else rm -f "$record_path" || result=1; fi
        fi
        for saved in "$stage/old/"* "$stage/old/".[!.]* "$stage/old/"..?*; do
            [ -e "$saved" ] || [ -L "$saved" ] || continue
            destination=$install_dir/${saved##*/}
            if [ -e "$destination" ] || [ -L "$destination" ] || ! mv "$saved" "$destination"; then
                result=1
            fi
        done
        for executable_name in $created_links; do
            if owned_link "$executable_name"; then rm -f "$link_dir/$executable_name"; fi
        done
    fi
    if [ -n "$stage" ]; then
        # Preserve a backup if restoring it failed.
        backup=no
        for saved in "$stage/old-record" "$stage/old/"* "$stage/old/".[!.]* "$stage/old/"..?*; do
            if [ -e "$saved" ] || [ -L "$saved" ]; then backup=yes; break; fi
        done
        if [ "$committed" = no ] && [ "$backup" = yes ]; then
            printf 'Backup retained at %s\n' "$stage" >&2
        else rm -rf "$stage" || result=1; fi
    fi
    if [ -n "$record_temp" ]; then rm -f "$record_temp"; fi
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
    if [ -f "$record_path" ] && [ ! -L "$record_path" ] &&
        printf '%s\n' "$install_dir/bin/mcpmd" | cmp -s - "$record_path"; then
        rm -f "$record_path"
    fi
    for name in $manifest_files; do
        if executable_name "$name" && owned_link "$name"; then rm -f "$link_dir/$name"; fi
        [ ! -d "$install_dir/bin/$name" ] || fail "expected a file: $name"
        rm -f "$install_dir/bin/$name"
    done
    rm -f "$install_dir/manifest" "$install_dir/manifest.json"
    rmdir "$install_dir/bin" 2>/dev/null || :
    printf 'Removed mcpm files and owned links. Configuration, caches and logs were kept.\n'
}

main() {
    requested='' proxy=''
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --proxy)
                [ -z "$proxy" ] || fail '--proxy may only be specified once'
                [ "$#" -ge 2 ] && [ -n "$2" ] || fail '--proxy requires a URL prefix'
                proxy=$2
                while [ "${proxy%/}" != "$proxy" ]; do proxy=${proxy%/}; done
                case "$proxy" in http://?*|https://?*) ;; *) fail '--proxy requires an http:// or https:// URL prefix' ;; esac
                shift 2
                ;;
            --uninstall)
                [ -z "$requested" ] || fail 'specify either VERSION or --uninstall'
                requested=$1
                shift
                ;;
            -*)
                fail 'usage: install.sh [--proxy URL] VERSION | --uninstall'
                ;;
            *)
                [ -z "$requested" ] || fail 'specify exactly one VERSION'
                requested=$1
                shift
                ;;
        esac
    done
    [ -n "$requested" ] || fail 'usage: install.sh [--proxy URL] VERSION | --uninstall'
    [ -n "${HOME-}" ] || fail 'HOME is required'
    case "$HOME" in /*) ;; *) fail 'HOME must be an absolute path' ;; esac
    install_dir=$HOME/.local/share/mcpm
    link_dir=$HOME/.local/bin
    record_dir=$HOME/.mcpm
    record_path=$record_dir/mcpmd.path
    stage='' transaction=no committed=no publishing=no created_links=''
    record_temp='' record_changed=no
    if [ "$requested" != --uninstall ]; then validate_version "$requested"; fi
    mkdir -p "$install_dir"
    [ ! -L "$install_dir" ] && [ ! -L "$install_dir/bin" ] || fail 'installation directories must not be symbolic links'
    mkdir "$install_dir/.install-lock" 2>/dev/null || fail "another installer is running (lock: $install_dir/.install-lock)"
    trap cleanup EXIT
    trap 'exit 1' HUP INT TERM
    if [ "$requested" = --uninstall ]; then uninstall; return; fi
    version=$requested
    case "$(uname -s)" in Darwin) platform=macos ;; Linux) platform=linux ;; *) fail 'supported systems: macOS and Linux' ;; esac
    case "$(uname -m)" in x86_64|amd64) architecture=x86_64 ;; arm64|aarch64) architecture=arm64 ;; *) fail 'supported architectures: x86_64 and ARM64' ;; esac
    asset=mcpm-$platform-$architecture
    if [ "$platform" = linux ]; then asset=$asset-musl; fi
    for entry in "$install_dir/"* "$install_dir/".[!.]* "$install_dir/"..?*; do
        [ "$entry" != "$install_dir/.install-lock" ] || continue
        [ -e "$entry" ] || [ -L "$entry" ] || continue
        confirm_replacement || return 0
        break
    done
    command -v unzip >/dev/null 2>&1 || fail 'unzip is required'
    stage=$(mktemp -d "$install_dir/.stage.XXXXXXXX")
    mkdir "$stage/old" "$stage/package" "$stage/package/bin"
    archive=$stage/package.zip
    url=https://github.com/EDGW/mcpm/releases/download/$version/$asset.zip
    if [ -n "$proxy" ]; then url=$proxy/$url; fi
    printf 'Downloading %s...\n' "$url"
    curl --fail --silent --show-error --location "$url" --output "$archive"
    curl --fail --silent --show-error --location "$url.sha256" --output "$stage/checksum"
    expected=$(awk 'NR == 1 {print $1}' "$stage/checksum")
    [ "${#expected}" -eq 64 ] || fail 'invalid SHA-256 checksum'
    case "$expected" in *[!0-9a-fA-F]*) fail 'invalid SHA-256 checksum' ;; esac
    if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$archive"); else actual=$(shasum -a 256 "$archive"); fi
    [ "${actual%% *}" = "$expected" ] || fail 'archive SHA-256 mismatch'
    LC_ALL=C unzip -Z1 "$archive" > "$stage/entries"
    [ -z "$(LC_ALL=C sort -f "$stage/entries" | LC_ALL=C uniq -di)" ] || fail 'duplicate ZIP entries'
    manifest_files='' executables='' has_manifest=no
    while IFS= read -r entry; do
        case "$entry" in
            manifest.json) has_manifest=yes ;;
            bin/*)
                name=${entry#bin/}
                valid_filename "$name" || fail "invalid archive entry: $entry"
                manifest_files="${manifest_files:+$manifest_files }$name"
                if executable_name "$name"; then
                    executables="${executables:+$executables }$name"
                fi ;;
            *) fail "unexpected archive entry: $entry" ;;
        esac
    done < "$stage/entries"
    [ "$has_manifest" = yes ] || fail 'package does not contain manifest.json'
    for name in mcpm mcpm-tools mcpmd; do
        case " $executables " in *" $name "*) ;; *) fail "package does not contain $name" ;; esac
    done
    # Write only the checked paths as regular files; never extract ZIP links or permissions.
    while IFS= read -r entry; do
        LC_ALL=C unzip -p "$archive" "$entry" > "$stage/package/$entry"
    done < "$stage/entries"
    for name in $executables; do chmod 755 "$stage/package/bin/$name"; done
    "$stage/package/bin/mcpm-tools" debug validate --exact
    for name in $executables; do
        executable=$stage/package/bin/$name
        [ "$("$executable" -V)" = "$name ${version#v}" ] || fail "executable version mismatch: $name"
        if [ -e "$link_dir/$name" ] || [ -L "$link_dir/$name" ]; then
            owned_link "$name" || fail "command already belongs to another installation: $link_dir/$name"
        fi
    done
    manifest_version=$version manifest_asset=$asset
    write_manifest > "$stage/new-manifest"
    mkdir -p "$link_dir"
    mkdir -p "$record_dir"
    [ ! -L "$record_dir" ] || fail 'configuration directory must not be a symbolic link'
    if [ -e "$record_path" ] || [ -L "$record_path" ]; then
        [ -f "$record_path" ] && [ ! -L "$record_path" ] || fail 'mcpmd record must be a regular file'
        cp -p "$record_path" "$stage/old-record"
    fi
    record_temp=$(mktemp "$record_dir/.mcpmd-path.XXXXXXXX")
    printf '%s\n' "$install_dir/bin/mcpmd" > "$record_temp"
    transaction=yes
    for entry in "$install_dir/"* "$install_dir/".[!.]* "$install_dir/"..?*; do
        case "$entry" in "$stage"|"$install_dir/.install-lock") continue ;; esac
        [ -e "$entry" ] || [ -L "$entry" ] || continue
        mv "$entry" "$stage/old"
    done
    publishing=yes
    mv "$stage/package/bin" "$install_dir/bin"
    mv "$stage/package/manifest.json" "$install_dir/manifest.json"
    for executable_name in $executables; do
        if ! owned_link "$executable_name"; then
            ln -s "$install_dir/bin/$executable_name" "$link_dir/$executable_name"
            created_links="${created_links:+$created_links }$executable_name"
        fi
    done
    record_changed=yes
    mv -f "$record_temp" "$record_path"
    mv "$stage/new-manifest" "$install_dir/manifest"
    committed=yes
    for link in "$link_dir/"*; do
        name=${link##*/}
        executable_name "$name" || continue
        case " $executables " in *" $name "*) ;; *) if owned_link "$name"; then rm -f "$link"; fi ;; esac
    done
    printf 'Installed %s to %s.\n' "$version" "$install_dir/bin"
    "$install_dir/bin/mcpm-tools" debug hello || printf 'Installed, but could not display the welcome guide.\n' >&2
    # Keep shell variables literal in the command printed for the user.
    # shellcheck disable=SC2016
    case ":${PATH-}:" in *":$link_dir:"*) ;; *) printf 'Add ~/.local/bin to your shell PATH, for example in ~/.zshrc or ~/.bashrc:\n  export PATH="$HOME/.local/bin:$PATH"\n' ;; esac
    resolved=$(command -v mcpm || :)
    if [ -n "$resolved" ] && [ "$resolved" != "$link_dir/mcpm" ]; then
        printf 'An existing command takes precedence: %s. Review PATH or remove the old installation explicitly.\n' "$resolved"
    fi
}

main "$@"
