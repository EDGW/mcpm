# Resolve executable data under remote Windows PowerShell, invoked through Windows cmd.
# $daemon is supplied by the encoded bootstrap; null requests installer discovery.
$ErrorActionPreference = 'Stop'
try {
    $status = 'explicit'
    if ($null -eq $daemon) {
        if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is unavailable.' }
        $record = Join-Path $env:LOCALAPPDATA 'mcpm\mcpmd.path'
        try {
            $text = [IO.File]::ReadAllText($record, (New-Object Text.UTF8Encoding($false, $true)))
        } catch [IO.FileNotFoundException] {
            $text = $null
        } catch [IO.DirectoryNotFoundException] {
            $text = $null
        }
        if ($null -eq $text) {
            $daemon = 'mcpmd'
            $status = 'fallback'
            [Console]::Error.WriteLine("warning: mcpmd.path not found: $record; trying mcpmd on remote PATH")
        } else {
            $daemon = $text -replace '\r?\n\z', ''
            if (-not $daemon -or $daemon.IndexOfAny([char[]]@([char]0, [char]10, [char]13)) -ge 0 -or
                $daemon -notmatch '^(?:[a-zA-Z]:[\\/]|[\\/]{2}[^\\/]+[\\/][^\\/]+[\\/])') {
                throw "Invalid mcpmd record: $record"
            }
            $status = 'record'
        }
    }
    if ($daemon.Contains('\') -or $daemon.Contains('/')) {
        $found = Test-Path -LiteralPath $daemon -PathType Leaf
    } else {
        $found = $null -ne (Get-Command -Name ([WildcardPattern]::Escape($daemon)) -CommandType Application -ErrorAction SilentlyContinue)
    }
    if (-not $found) {
        [Console]::Out.WriteLine("mcpm-bootstrap:$status-missing")
        [Console]::Error.WriteLine("Remote mcpmd executable was not found: $daemon")
        exit 127
    }
    [Console]::Out.WriteLine("mcpm-bootstrap:$status")
    & $daemon serve
    exit $LASTEXITCODE
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
