# Install a tagged Windows x64 mcpm release for the current user, or uninstall it.

[CmdletBinding()]
param(
    [string]$Version,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

if (-not $env:LOCALAPPDATA) {
    throw 'LOCALAPPDATA is required; run this script in Windows PowerShell.'
}
if ($Uninstall -and $Version) {
    throw 'Specify either -Version or -Uninstall.'
}

$installDir = Join-Path $env:LOCALAPPDATA 'Programs\mcpm'
$executable = Join-Path $installDir 'mcpm.exe'
if (Test-Path -LiteralPath $executable -PathType Container) {
    throw "$executable is a directory."
}

function Update-InstallPath {
    param([switch]$Remove)

    foreach ($scope in @('User', 'Process')) {
        $existingPath = [Environment]::GetEnvironmentVariable('Path', $scope)
        $entries = @()
        if ($existingPath) {
            $entries = @($existingPath -split ';')
        }
        $matchingEntries = @($entries | Where-Object {
            [Environment]::ExpandEnvironmentVariables($_.Trim().Trim('"')).TrimEnd('\') -ieq $installDir
        })
        if ($Remove) {
            $entries = @($entries | Where-Object { $_ -notin $matchingEntries })
        } elseif ($matchingEntries.Count -eq 0) {
            $entries += $installDir
        }
        $updatedPath = $entries -join ';'
        if ($updatedPath -cne $existingPath) {
            [Environment]::SetEnvironmentVariable('Path', $updatedPath, $scope)
        }
    }
}

if ($Uninstall) {
    if (Test-Path -LiteralPath $executable) {
        Remove-Item -LiteralPath $executable -Force
    }
    if ((Test-Path -LiteralPath $installDir -PathType Container) -and
        @(Get-ChildItem -LiteralPath $installDir -Force).Count -eq 0) {
        Remove-Item -LiteralPath $installDir
    }
    Update-InstallPath -Remove
    Write-Output 'Removed mcpm and its PATH entry. Configuration and cache files were kept.'
    return
}

if ($Version -notmatch '^v[0-9][a-zA-Z0-9._-]*$') {
    throw 'Specify -Version with a release tag such as v0.1.0-dev.26w43b.'
}
$architecture = $env:PROCESSOR_ARCHITEW6432
if (-not $architecture) {
    $architecture = $env:PROCESSOR_ARCHITECTURE
}
if ($architecture -ine 'AMD64') {
    throw 'The Windows release requires an x64 system.'
}

$url = "https://github.com/EDGW/mcpm/releases/download/$Version/mcpm-windows-x86_64.exe"
$temporaryDir = Join-Path ([IO.Path]::GetTempPath()) ('mcpm-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporaryDir | Out-Null
$stagedExecutable = $null
try {
    $download = Join-Path $temporaryDir 'mcpm.exe'
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $download
    if ((Get-Item -LiteralPath $download).Length -eq 0) {
        throw 'Downloaded executable is empty.'
    }
    & $download -V
    if ($LASTEXITCODE -ne 0) {
        throw 'Downloaded executable could not run.'
    }
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $stagedExecutable = Join-Path $installDir ('.mcpm-' + [Guid]::NewGuid().ToString('N') + '.exe')
    Copy-Item -LiteralPath $download -Destination $stagedExecutable
    if (Test-Path -LiteralPath $executable) {
        [IO.File]::Replace($stagedExecutable, $executable, $null)
    } else {
        [IO.File]::Move($stagedExecutable, $executable)
    }
    $stagedExecutable = $null
    Update-InstallPath
    Write-Output "Installed $Version to $executable. Run: mcpm --help"
} finally {
    Remove-Item -LiteralPath $temporaryDir -Recurse -Force
    if ($stagedExecutable -and (Test-Path -LiteralPath $stagedExecutable)) {
        Remove-Item -LiteralPath $stagedExecutable -Force
    }
}
