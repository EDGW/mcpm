#!/bin/sh
# Resolve executable data and report discovery before handing stdout to the daemon protocol.
set -eu
status=explicit
if [ "$#" -gt 0 ]; then
    daemon=$1
else
    record=${HOME:?HOME is unavailable}/.mcpm/mcpmd.path
    invalid_record() {
        printf 'Invalid or unreadable mcpmd record: %s\n' "$record" >&2
        exit 1
    }
    if [ -e "$record" ] || [ -L "$record" ]; then
        [ -f "$record" ] || invalid_record
        daemon=''
        IFS= read -r daemon < "$record" || [ -n "$daemon" ] || invalid_record
        cr=$(printf '\r')
        daemon=${daemon%"$cr"}
        case "$daemon" in *"$cr"*) invalid_record ;; /*) ;; *) invalid_record ;; esac
        # Reject extra lines and NULs omitted by shell read.
        printf '%s\n' "$daemon" | cmp -s - "$record" ||
            printf '%s' "$daemon" | cmp -s - "$record" ||
            printf '%s\r\n' "$daemon" | cmp -s - "$record" || invalid_record
        status=record
    else
        daemon=mcpmd
        status=fallback
        printf 'warning: mcpmd.path not found: %s; trying mcpmd on remote PATH\n' "$record" >&2
    fi
fi
if ! command -v "$daemon" >/dev/null 2>&1; then
    printf 'mcpm-bootstrap:%s-missing\n' "$status"
    printf 'Remote mcpmd executable was not found: %s\n' "$daemon" >&2
    exit 127
fi
printf 'mcpm-bootstrap:%s\n' "$status"
exec "$daemon" serve
