# Install, upgrade or uninstall a complete mcpm suite for the current Windows user.

[CmdletBinding()]
param(
    [string]$Version,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

function Assert-SupportedVersion {
    param([string]$Tag)
    if ($Tag -cnotmatch '^v([0-9]+\.[0-9]+\.[0-9]+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$') {
        throw 'Specify a SemVer release tag at or after v0.1.0-dev.26w43c.'
    }
    $core = [version]$Matches[1]
    $prerelease = $Matches[2]
    $minimum = [version]'0.1.0'
    if ($core -gt $minimum) { return }
    if ($core -eq $minimum) {
        if (-not $prerelease) { return }
        $parts = $prerelease.Split('.')
        $lower = @('dev', '26w43c')
        for ($index = 0; $index -lt $lower.Count; $index++) {
            if ($index -ge $parts.Count) { break }
            $numeric = $parts[$index] -match '^[0-9]+$'
            $lowerNumeric = $lower[$index] -match '^[0-9]+$'
            if ($numeric -ne $lowerNumeric) {
                if (-not $numeric) { return }
                break
            }
            $comparison = if ($numeric) {
                ([decimal]$parts[$index]).CompareTo([decimal]$lower[$index])
            } else {
                [string]::CompareOrdinal($parts[$index], $lower[$index])
            }
            if ($comparison -gt 0) { return }
            if ($comparison -lt 0) { break }
            if ($index -eq $lower.Count - 1) { return }
        }
    }
    throw 'Supported versions start at v0.1.0-dev.26w43c; older releases cannot be installed by this script.'
}

# The sole interpreter of package and installation manifests on Windows.
function Read-Manifest {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf) -or
        ((Get-Item -LiteralPath $Path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Missing regular installation manifest: $Path"
    }
    $fields = @{}
    $files = @()
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        $pair = $line -split '=', 2
        if ($pair.Count -ne 2) { throw 'Invalid manifest record.' }
        $key, $value = $pair
        if ($key -eq 'file') {
            if ($value -cnotmatch '^(mcpm|mcpmc|mcpm-[a-zA-Z0-9-]+)\.exe$' -or $value -in $files) {
                throw 'Invalid or duplicate manifest executable.'
            }
            $files += $value
        } elseif ($key -in @('format', 'version', 'asset', 'path_user', 'path_process')) {
            if ($fields.ContainsKey($key)) { throw "Duplicate manifest field: $key" }
            $fields[$key] = $value
        } else { throw "Unknown manifest field: $key" }
    }
    if ($fields.format -ne '1' -or -not $fields.asset -or -not $files.Count) { throw 'Invalid installation manifest.' }
    foreach ($key in @('path_user', 'path_process')) {
        if ($fields.ContainsKey($key) -and $fields[$key] -notin @('0', '1')) { throw 'Invalid PATH ownership record.' }
    }
    Assert-SupportedVersion $fields.version
    [pscustomobject]@{
        Version = $fields.version
        Asset = $fields.asset
        Files = $files
        UserPathAdded = $fields.path_user -eq '1'
        ProcessPathAdded = $fields.path_process -eq '1'
    }
}

function Write-Manifest {
    param([string]$Path, $Manifest)
    $lines = @('format=1', "version=$($Manifest.Version)", "asset=$($Manifest.Asset)")
    $lines += @($Manifest.Files | ForEach-Object { "file=$_" })
    $lines += "path_user=$([int]$Manifest.UserPathAdded)", "path_process=$([int]$Manifest.ProcessPathAdded)"
    [IO.File]::WriteAllLines($Path, [string[]]$lines, (New-Object Text.UTF8Encoding($false)))
}

function Assert-RegularDirectory {
    param([string]$Path)
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path -Force
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Expected a regular directory: $Path"
        }
    }
}

function Get-PathEntries {
    param([string]$Value)
    if ($Value) { $Value -split ';' }
}

function Test-PathEntry {
    param([string]$Entry, [string]$Directory)
    [Environment]::ExpandEnvironmentVariables($Entry.Trim().Trim('"')).TrimEnd('\') -ieq $Directory.TrimEnd('\')
}

function Set-InstallPath {
    param([string]$Scope, [string]$Directory, [switch]$Remove)
    $existing = [Environment]::GetEnvironmentVariable('Path', $Scope)
    $entries = @(Get-PathEntries $existing)
    $contains = @($entries | Where-Object { Test-PathEntry $_ $Directory }).Count -gt 0
    if ($Remove) {
        if ($contains) {
            $updated = @($entries | Where-Object { -not (Test-PathEntry $_ $Directory) }) -join ';'
            [Environment]::SetEnvironmentVariable('Path', $updated, $Scope)
        }
    } elseif (-not $contains) {
        $updated = (@($entries) + $Directory) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $updated, $Scope)
    }
    return (-not $contains -and -not $Remove)
}

function Invoke-Install {
    if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is required.' }
    if ($Uninstall -and $Version) { throw 'Specify either -Version or -Uninstall.' }
    if (-not $Uninstall) { Assert-SupportedVersion $Version }
    $installDir = Join-Path $env:LOCALAPPDATA 'Programs\mcpm'
    $binDir = Join-Path $installDir 'bin'
    $manifestPath = Join-Path $installDir 'manifest'
    Assert-RegularDirectory $installDir
    Assert-RegularDirectory $binDir
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $lock = $null
    $stage = $null
    $committed = $false
    $transaction = $false
    $newBin = $false
    $newManifest = $false
    $originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $originalProcessPath = [Environment]::GetEnvironmentVariable('Path', 'Process')
    try {
        $lockPath = Join-Path $installDir '.install-lock'
        try { $lock = [IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None') }
        catch { throw "Another installer is running, or installation lock cannot be opened: $lockPath" }
        $old = $null
        if (Test-Path -LiteralPath $manifestPath) { $old = Read-Manifest $manifestPath }
        if ($Uninstall) {
            if (-not $old) {
                Write-Output "No managed mcpm installation at $installDir. Legacy installations are not removed."
                return
            }
            foreach ($name in $old.Files) {
                $path = Join-Path $binDir $name
                if (Test-Path -LiteralPath $path -PathType Container) { throw "Expected an executable file: $path" }
                if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
            }
            if ($old.UserPathAdded) { Set-InstallPath User $binDir -Remove | Out-Null }
            if ($old.ProcessPathAdded) { Set-InstallPath Process $binDir -Remove | Out-Null }
            Remove-Item -LiteralPath $manifestPath
            if ((Test-Path -LiteralPath $binDir) -and @(Get-ChildItem -LiteralPath $binDir -Force).Count -eq 0) { Remove-Item -LiteralPath $binDir }
            Write-Output 'Removed mcpm executables and owned PATH entries. Configuration, caches and logs were kept.'
            return
        }
        $architecture = $env:PROCESSOR_ARCHITEW6432
        if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
        if ($architecture -ine 'AMD64') { throw 'The Windows release requires an x64 system.' }
        if (Test-Path -LiteralPath $binDir) {
            if (-not $old) { throw 'Existing bin directory has no installation manifest.' }
            foreach ($file in Get-ChildItem -LiteralPath $binDir -Force) {
                if ($file.Name -notin $old.Files -or $file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                    throw "Unmanaged file in installation bin directory: $($file.FullName)"
                }
            }
        }
        $stage = Join-Path $installDir ('.stage-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $stage | Out-Null
        $asset = 'mcpm-windows-x86_64'
        $url = "https://github.com/EDGW/mcpm/releases/download/$Version/$asset.zip"
        $archivePath = Join-Path $stage 'package.zip'
        $checksumPath = Join-Path $stage 'checksum'
        Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $archivePath
        Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256" -OutFile $checksumPath
        $expected = ([IO.File]::ReadAllText($checksumPath).Trim() -split '\s+')[0]
        if ($expected -notmatch '^[0-9a-fA-F]{64}$' -or (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ine $expected) {
            throw 'Archive SHA-256 mismatch or invalid checksum.'
        }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
        try {
            $entries = @()
            foreach ($entry in $archive.Entries) {
                if ($entry.FullName -cnotmatch '^(manifest|bin/(mcpm|mcpmc|mcpm-[a-zA-Z0-9-]+)\.exe)$' -or $entry.FullName -in $entries) {
                    throw "Unexpected or duplicate archive entry: $($entry.FullName)"
                }
                $entries += $entry.FullName
            }
        } finally { $archive.Dispose() }
        $package = Join-Path $stage 'package'
        [IO.Compression.ZipFile]::ExtractToDirectory($archivePath, $package)
        $manifest = Read-Manifest (Join-Path $package 'manifest')
        if ($manifest.Version -cne $Version -or $manifest.Asset -cne $asset) { throw 'Archive version or platform does not match request.' }
        if ($entries.Count -ne $manifest.Files.Count + 1) { throw 'Archive files do not match manifest.' }
        foreach ($name in $manifest.Files) {
            $binary = Join-Path (Join-Path $package 'bin') $name
            if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "Missing executable: $name" }
            $actual = & $binary -V
            if ($LASTEXITCODE -ne 0 -or $actual -cne "$([IO.Path]::GetFileNameWithoutExtension($name)) $($Version.Substring(1))") { throw "Executable version mismatch: $name" }
            & $binary --help | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "Executable cannot run: $name" }
        }
        # Legacy layouts have no ownership record: preserve their executable and PATH.
        $legacy = Join-Path $installDir 'mcpm.exe'
        if (Test-Path -LiteralPath $legacy) {
            Write-Warning "Legacy executable kept at $legacy. Remove it explicitly after checking the new installation."
        }
        $userHasBin = @((Get-PathEntries $originalUserPath) | Where-Object { Test-PathEntry $_ $binDir }).Count -gt 0
        $processHasBin = @((Get-PathEntries $originalProcessPath) | Where-Object { Test-PathEntry $_ $binDir }).Count -gt 0
        $manifest.UserPathAdded = (-not $userHasBin) -or ($old -and $old.UserPathAdded)
        $manifest.ProcessPathAdded = (-not $processHasBin) -or ($old -and $old.ProcessPathAdded)
        Write-Manifest (Join-Path $stage 'new-manifest') $manifest
        $transaction = $true
        if (Test-Path -LiteralPath $binDir) { Move-Item -LiteralPath $binDir -Destination (Join-Path $stage 'old-bin') }
        if ($old) { Move-Item -LiteralPath $manifestPath -Destination (Join-Path $stage 'old-manifest') }
        $newBin = $true
        Move-Item -LiteralPath (Join-Path $package 'bin') -Destination $binDir
        Set-InstallPath User $binDir | Out-Null
        Set-InstallPath Process $binDir | Out-Null
        $newManifest = $true
        Move-Item -LiteralPath (Join-Path $stage 'new-manifest') -Destination $manifestPath
        $committed = $true
        Write-Output "Installed $Version to $binDir. Run: mcpm --help"
        $resolved = Get-Command mcpm -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($resolved -and $resolved.Source -ine (Join-Path $binDir 'mcpm.exe')) {
            Write-Warning "An existing command takes precedence: $($resolved.Source). Review PATH or remove the old installation explicitly."
        }
    } catch {
        $failure = $_
        if ($transaction -and -not $committed) {
            try {
                [Environment]::SetEnvironmentVariable('Path', $originalUserPath, 'User')
                [Environment]::SetEnvironmentVariable('Path', $originalProcessPath, 'Process')
                if ($newBin -and (Test-Path -LiteralPath $binDir)) { Remove-Item -LiteralPath $binDir -Recurse -Force }
                if ($newManifest -and (Test-Path -LiteralPath $manifestPath)) { Remove-Item -LiteralPath $manifestPath -Force }
                if (Test-Path -LiteralPath (Join-Path $stage 'old-bin')) { Move-Item -LiteralPath (Join-Path $stage 'old-bin') -Destination $binDir }
                if (Test-Path -LiteralPath (Join-Path $stage 'old-manifest')) { Move-Item -LiteralPath (Join-Path $stage 'old-manifest') -Destination $manifestPath }
            } catch { Write-Warning "Rollback incomplete; backup retained at $stage. Close running mcpm processes before retrying. $_" }
        }
        throw $failure
    } finally {
        try {
            if ($stage -and (Test-Path -LiteralPath $stage)) {
                $hasBackup = (Test-Path -LiteralPath (Join-Path $stage 'old-bin')) -or (Test-Path -LiteralPath (Join-Path $stage 'old-manifest'))
                if ($committed -or -not $hasBackup) { Remove-Item -LiteralPath $stage -Recurse -Force }
            }
        } finally {
            if ($lock) { $lock.Dispose() }
            # Keep the lock file to avoid racing another installer opening it during cleanup.
        }
    }
}

Invoke-Install
