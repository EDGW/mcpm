# Install, upgrade or uninstall a complete mcpm suite for the current Windows user.

[CmdletBinding()]
param(
    [string]$Version,
    [switch]$Uninstall,
    [ValidateNotNullOrEmpty()]
    [string]$Proxy
)

$ErrorActionPreference = 'Stop'

function Assert-SupportedVersion {
    param([string]$Tag)
    if ($Tag -cnotmatch '^v([0-9]+\.[0-9]+\.[0-9]+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$') {
        throw 'Specify a SemVer release tag at or after v0.1.0-rc1.'
    }
    $core = [version]$Matches[1]
    $prerelease = $Matches[2]
    $minimum = [version]'0.1.0'
    if ($core -gt $minimum) { return }
    if ($core -eq $minimum) {
        if (-not $prerelease) { return }
        $parts = $prerelease.Split('.')
        $lower = @('rc1')
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
    throw 'Supported versions start at v0.1.0-rc1; older releases cannot be installed by this script.'
}

# Local ownership record. The package's manifest.json is validated by mcpm-tools.
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
            if (-not (Test-ArtifactName $value) -or $value -in $files) {
                throw 'Invalid or duplicate manifest filename.'
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

function Test-ArtifactName {
    param([string]$Name)
    return ($Name -cmatch '\A[A-Za-z0-9][A-Za-z0-9._-]*\z' -and
        -not $Name.EndsWith('.') -and
        $Name -notmatch '^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(\.|$)')
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

# Both files live in the configuration directory so replacement stays on one filesystem.
function Publish-DaemonRecord {
    param([string]$Temporary, [string]$Destination)
    if ([IO.File]::Exists($Destination)) { [IO.File]::Replace($Temporary, $Destination, [NullString]::Value) }
    else { [IO.File]::Move($Temporary, $Destination) }
}

function Invoke-Install {
    if ($Proxy) {
        $Proxy = $Proxy.TrimEnd('/')
        if ($Proxy -notmatch '^https?://.+') { throw '-Proxy requires an http:// or https:// URL prefix.' }
    }
    if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is required.' }
    if (-not ([IO.Path]::IsPathRooted($env:LOCALAPPDATA))) { throw 'LOCALAPPDATA must be an absolute path.' }
    if ($Uninstall -and $Version) { throw 'Specify either -Version or -Uninstall.' }
    if (-not $Uninstall) { Assert-SupportedVersion $Version }
    $installDir = Join-Path $env:LOCALAPPDATA 'Programs\mcpm'
    $binDir = Join-Path $installDir 'bin'
    $manifestPath = Join-Path $installDir 'manifest'
    $releaseManifestPath = Join-Path $installDir 'manifest.json'
    $recordDir = Join-Path $env:LOCALAPPDATA 'mcpm'
    $recordPath = Join-Path $recordDir 'mcpmd.path'
    $daemonPath = Join-Path $binDir 'mcpmd.exe'
    Assert-RegularDirectory $installDir
    Assert-RegularDirectory $binDir
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $lock = $null
    $stage = $null
    $committed = $false
    $transaction = $false
    $publishing = $false
    $recordChanged = $false
    $recordTemp = $null
    $originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $originalProcessPath = [Environment]::GetEnvironmentVariable('Path', 'Process')
    try {
        $lockPath = Join-Path $installDir '.install-lock'
        try { $lock = [IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None') }
        catch { throw "Another installer is running, or installation lock cannot be opened: $lockPath" }
        $old = $null
        if (Test-Path -LiteralPath $manifestPath) {
            if ($Uninstall) { $old = Read-Manifest $manifestPath }
            else {
                try { $old = Read-Manifest $manifestPath }
                catch { Write-Warning 'Old PATH ownership could not be read; preexisting entries will be kept.' }
            }
        }
        if ($Uninstall) {
            if (-not $old) {
                Write-Output "No managed mcpm installation at $installDir. Legacy installations are not removed."
                return
            }
            if ((Test-Path -LiteralPath $recordPath -PathType Leaf) -and
                -not ((Get-Item -LiteralPath $recordPath -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -and
                ([IO.File]::ReadAllText($recordPath) -ceq "$daemonPath`n")) {
                Remove-Item -LiteralPath $recordPath
            }
            foreach ($name in $old.Files) {
                $path = Join-Path $binDir $name
                if (Test-Path -LiteralPath $path -PathType Container) { throw "Expected a file: $path" }
                if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
            }
            if ($old.UserPathAdded) { Set-InstallPath User $binDir -Remove | Out-Null }
            if ($old.ProcessPathAdded) { Set-InstallPath Process $binDir -Remove | Out-Null }
            Remove-Item -LiteralPath $manifestPath
            if (Test-Path -LiteralPath $releaseManifestPath -PathType Leaf) {
                Remove-Item -LiteralPath $releaseManifestPath -Force
            }
            if ((Test-Path -LiteralPath $binDir) -and @(Get-ChildItem -LiteralPath $binDir -Force).Count -eq 0) { Remove-Item -LiteralPath $binDir }
            Write-Output 'Removed mcpm files and owned PATH entries. Configuration, caches and logs were kept.'
            return
        }
        $architecture = $env:PROCESSOR_ARCHITEW6432
        if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
        if ($architecture -ine 'AMD64') { throw 'The Windows release requires an x64 system.' }
        $previous = @(Get-ChildItem -LiteralPath $installDir -Force | Where-Object { $_.Name -ne '.install-lock' })
        if ($previous.Count -gt 0) {
            $answer = Read-Host "Existing mcpm installation at $installDir. Replace and delete all previous contents? [y/N]"
            if ($answer -notmatch '\A(?:y|yes)\z') {
                Write-Output 'Installation cancelled; existing files were not changed.'
                return
            }
        }
        $stage = Join-Path $installDir ('.stage-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $stage | Out-Null
        $backup = Join-Path $stage 'old'
        $package = Join-Path $stage 'package'
        $packageBin = Join-Path $package 'bin'
        New-Item -ItemType Directory -Path $backup | Out-Null
        New-Item -ItemType Directory -Path $packageBin -Force | Out-Null
        $asset = 'mcpm-windows-x86_64'
        $url = "https://github.com/hitszosa/mcpm/releases/download/$Version/$asset.zip"
        if ($Proxy) { $url = "$Proxy/$url" }
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
            $names = @()
            $hasManifest = $false
            foreach ($entry in $archive.Entries) {
                if ($entry.FullName -ceq 'manifest.json' -and -not $hasManifest) {
                    $hasManifest = $true
                    $destination = Join-Path $package 'manifest.json'
                } elseif ($entry.FullName.StartsWith('bin/', [StringComparison]::Ordinal) -and
                    (Test-ArtifactName $entry.Name) -and $entry.FullName -ceq "bin/$($entry.Name)" -and
                    $entry.Name -notin $names) {
                    $names += $entry.Name
                    $destination = Join-Path $packageBin $entry.Name
                } else { throw "Unexpected or duplicate ZIP entry: $($entry.FullName)" }
                # Write the selected entry's bytes as a regular file, not ZIP link metadata.
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $destination)
            }
        } finally { $archive.Dispose() }
        if (-not $hasManifest) { throw 'Package does not contain manifest.json.' }
        foreach ($required in @('mcpm.exe', 'mcpm-tools.exe', 'mcpmd.exe')) {
            if ($required -cnotin $names) { throw "Package does not contain $required." }
        }
        & (Join-Path $packageBin 'mcpm-tools.exe') debug validate --exact
        if ($LASTEXITCODE -ne 0) { throw 'Installation validation failed.' }
        foreach ($name in $names) {
            if ($name -cnotmatch '^(mcpm|mcpmc|mcpmd|mcpm-[a-zA-Z0-9-]+)\.exe$') { continue }
            $actual = & (Join-Path $packageBin $name) -V
            if ($LASTEXITCODE -ne 0 -or $actual -cne "$([IO.Path]::GetFileNameWithoutExtension($name)) $($Version.Substring(1))") {
                throw "Executable version mismatch: $name"
            }
        }
        $userHasBin = @((Get-PathEntries $originalUserPath) | Where-Object { Test-PathEntry $_ $binDir }).Count -gt 0
        $processHasBin = @((Get-PathEntries $originalProcessPath) | Where-Object { Test-PathEntry $_ $binDir }).Count -gt 0
        $manifest = [pscustomobject]@{
            Version = $Version
            Asset = $asset
            Files = $names
            UserPathAdded = (-not $userHasBin) -or ($old -and $old.UserPathAdded)
            ProcessPathAdded = (-not $processHasBin) -or ($old -and $old.ProcessPathAdded)
        }
        Write-Manifest (Join-Path $stage 'new-manifest') $manifest
        Assert-RegularDirectory $recordDir
        New-Item -ItemType Directory -Path $recordDir -Force | Out-Null
        if (Test-Path -LiteralPath $recordPath) {
            $item = Get-Item -LiteralPath $recordPath -Force
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "Expected a regular mcpmd record: $recordPath"
            }
            Copy-Item -LiteralPath $recordPath -Destination (Join-Path $stage 'old-record')
        }
        $recordTemp = Join-Path $recordDir ('.mcpmd-path-' + [guid]::NewGuid().ToString('N'))
        [IO.File]::WriteAllText($recordTemp, "$daemonPath`n", (New-Object Text.UTF8Encoding($false)))
        $transaction = $true
        foreach ($entry in $previous) { Move-Item -LiteralPath $entry.FullName -Destination $backup }
        $publishing = $true
        Move-Item -LiteralPath $packageBin -Destination $binDir
        Move-Item -LiteralPath (Join-Path $package 'manifest.json') -Destination $releaseManifestPath
        Set-InstallPath User $binDir | Out-Null
        Set-InstallPath Process $binDir | Out-Null
        $recordChanged = $true
        Publish-DaemonRecord $recordTemp $recordPath
        Move-Item -LiteralPath (Join-Path $stage 'new-manifest') -Destination $manifestPath
        $committed = $true
        Write-Output "Installed $Version to $binDir."
        try {
            & (Join-Path $binDir 'mcpm-tools.exe') debug hello
            if ($LASTEXITCODE -ne 0) { Write-Warning 'Installed, but could not display the welcome guide.' }
        } catch { Write-Warning "Installed, but could not display the welcome guide: $_" }
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
                if ($recordChanged) {
                    $oldRecord = Join-Path $stage 'old-record'
                    if (Test-Path -LiteralPath $oldRecord) {
                        Copy-Item -LiteralPath $oldRecord -Destination $recordTemp -Force
                        Publish-DaemonRecord $recordTemp $recordPath
                        Remove-Item -LiteralPath $oldRecord
                    } elseif (Test-Path -LiteralPath $recordPath) { Remove-Item -LiteralPath $recordPath }
                }
                if ($publishing) {
                    foreach ($path in @($binDir, $manifestPath, $releaseManifestPath)) {
                        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
                    }
                }
                foreach ($entry in Get-ChildItem -LiteralPath $backup -Force) {
                    Move-Item -LiteralPath $entry.FullName -Destination $installDir
                }
            } catch { Write-Warning "Rollback incomplete; backup retained at $stage. Close running mcpm processes before retrying. $_" }
        }
        throw $failure
    } finally {
        try {
            if ($recordTemp -and (Test-Path -LiteralPath $recordTemp)) { Remove-Item -LiteralPath $recordTemp -Force }
            if ($stage -and (Test-Path -LiteralPath $stage)) {
                $hasBackup = (Test-Path -LiteralPath (Join-Path $stage 'old-record')) -or
                    @(Get-ChildItem -LiteralPath (Join-Path $stage 'old') -Force -ErrorAction SilentlyContinue).Count -gt 0
                if ($committed -or -not $hasBackup) { Remove-Item -LiteralPath $stage -Recurse -Force }
            }
        } finally {
            if ($lock) { $lock.Dispose() }
            # Keep the lock file to avoid racing another installer opening it during cleanup.
        }
    }
}

Invoke-Install
