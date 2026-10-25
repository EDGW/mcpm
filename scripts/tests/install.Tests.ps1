# Exercise the Windows installer against an offline suite, including rollback and PATH ownership.
param([Parameter(Mandatory = $true)][string]$BinaryDir)

$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$installer = Join-Path $repo 'scripts/install.ps1'
$packageScript = Join-Path $repo 'scripts/package.py'
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('mcpm-installer-test-' + [guid]::NewGuid().ToString('N'))
$originalLocal = $env:LOCALAPPDATA
$originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$originalProcessPath = $env:Path
$testState = [pscustomobject]@{
    FailCommit = $false
    Downloads = 0
    ReleaseDir = ''
    ReleaseVersion = 'v' + ((& (Join-Path $BinaryDir 'mcpm.exe') -V) -split ' ')[1]
}

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Invoke-WebRequest {
    param([switch]$UseBasicParsing, [string]$Uri, [string]$OutFile)
    $testState.Downloads++
    if (-not $Uri.StartsWith("https://github.com/EDGW/mcpm/releases/download/$($testState.ReleaseVersion)/")) { throw "Unexpected download: $Uri" }
    Copy-Item -LiteralPath (Join-Path $testState.ReleaseDir ($Uri.Split('/')[-1])) -Destination $OutFile
}

function Move-Item {
    param([string]$LiteralPath, [string]$Destination)
    if ($testState.FailCommit -and (Split-Path $LiteralPath -Leaf) -eq 'new-manifest') {
        $testState.FailCommit = $false
        throw 'Injected manifest commit failure'
    }
    Microsoft.PowerShell.Management\Move-Item -LiteralPath $LiteralPath -Destination $Destination
}

function Invoke-Installer {
    param([switch]$ExpectFailure, [switch]$Uninstall, [string]$Version = $testState.ReleaseVersion)
    $failed = $false
    try {
        if ($Uninstall) { & $installer -Uninstall | Out-Null }
        else { & $installer -Version $Version | Out-Null }
    } catch {
        $failed = $true
        if (-not $ExpectFailure) { throw }
    }
    Assert-True ($failed -eq [bool]$ExpectFailure) "Unexpected installer outcome (Version=$Version, Uninstall=$Uninstall)"
}

try {
    New-Item -ItemType Directory -Path $testRoot | Out-Null
    $testState.ReleaseDir = Join-Path $testRoot 'release'
    python $packageScript --binary-dir $BinaryDir --output-dir $testState.ReleaseDir --asset mcpm-windows-x86_64 --target x86_64-pc-windows-msvc --tag $testState.ReleaseVersion
    if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare Windows fixture archive.' }
    $env:LOCALAPPDATA = Join-Path $testRoot 'local app data'
    $installDir = Join-Path $env:LOCALAPPDATA 'Programs\mcpm'
    $binDir = Join-Path $installDir 'bin'
    $manifestPath = Join-Path $installDir 'manifest'
    foreach ($version in @('v0.1.0-dev.26w43b', 'v0.1.0-dev.26w42z', 'v0.1.0-alpha', 'v0.0.9')) {
        Invoke-Installer -Version $version -ExpectFailure
    }
    Assert-True ($testState.Downloads -eq 0) 'Unsupported versions reached the download step.'
    # Preserve a legacy executable and its preexisting PATH entry.
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $legacy = Join-Path $installDir 'mcpm.exe'
    [IO.File]::WriteAllText($legacy, 'legacy executable')
    [Environment]::SetEnvironmentVariable('Path', "$originalUserPath;$installDir", 'User')
    $env:Path = "$originalProcessPath;$installDir"
    Invoke-Installer
    $files = @(Get-ChildItem -LiteralPath $binDir)
    Assert-True ($files.Count -eq 5) 'The complete suite was not installed.'
    Assert-True (Test-Path -LiteralPath $legacy) 'Legacy executable was removed.'
    $originalManifest = [IO.File]::ReadAllText($manifestPath)
    Remove-Item -LiteralPath $files[0].FullName
    Invoke-Installer
    Assert-True ((Get-ChildItem -LiteralPath $binDir).Count -eq 5) 'Repair did not restore a missing binary.'
    $testState.FailCommit = $true
    Invoke-Installer -ExpectFailure
    Assert-True ([IO.File]::ReadAllText($manifestPath) -ceq $originalManifest) 'Rollback did not restore the manifest.'
    Assert-True ((Get-ChildItem -LiteralPath $binDir).Count -eq 5) 'Rollback did not restore all binaries.'
    $lock = [IO.File]::Open((Join-Path $installDir '.install-lock'), 'Open', 'ReadWrite', 'None')
    try { Invoke-Installer -ExpectFailure } finally { $lock.Dispose() }
    $checksum = Join-Path $testState.ReleaseDir 'mcpm-windows-x86_64.zip.sha256'
    $savedChecksum = [IO.File]::ReadAllText($checksum)
    [IO.File]::WriteAllText($checksum, ('0' * 64) + '  mcpm-windows-x86_64.zip')
    Invoke-Installer -ExpectFailure
    Assert-True ([IO.File]::ReadAllText($manifestPath) -ceq $originalManifest) 'Bad download changed the installation.'
    [IO.File]::WriteAllText($checksum, $savedChecksum)
    $archivePath = Join-Path $testState.ReleaseDir 'mcpm-windows-x86_64.zip'
    $savedArchive = Join-Path $testRoot 'complete.zip'
    Copy-Item -LiteralPath $archivePath -Destination $savedArchive
    $zip = [IO.Compression.ZipFile]::Open($archivePath, 'Update')
    try { $zip.GetEntry('bin/mcpm-tools.exe').Delete() } finally { $zip.Dispose() }
    [IO.File]::WriteAllText($checksum, (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash + '  mcpm-windows-x86_64.zip')
    Invoke-Installer -ExpectFailure
    Assert-True ([IO.File]::ReadAllText($manifestPath) -ceq $originalManifest) 'Incomplete package changed the installation.'
    Copy-Item -LiteralPath $savedArchive -Destination $archivePath -Force
    [IO.File]::WriteAllText($checksum, $savedChecksum)
    $foreign = Join-Path $binDir 'personal.txt'
    [IO.File]::WriteAllText($foreign, 'keep')
    Invoke-Installer -ExpectFailure
    Invoke-Installer -Uninstall
    Assert-True (Test-Path -LiteralPath $foreign) 'Uninstall removed an unmanaged file.'
    Assert-True (-not (Test-Path -LiteralPath $manifestPath)) 'Uninstall retained the manifest.'
    Assert-True ([Environment]::GetEnvironmentVariable('Path', 'User') -notlike "*$binDir*") 'Owned PATH entry was retained.'
    Assert-True ([Environment]::GetEnvironmentVariable('Path', 'User') -like "*$installDir*") 'Legacy PATH entry was removed.'
    Remove-Item -LiteralPath $foreign
    Remove-Item -LiteralPath $binDir
    # A preexisting bin PATH entry must survive installation and uninstallation.
    [Environment]::SetEnvironmentVariable('Path', "$originalUserPath;$binDir", 'User')
    $env:Path = "$originalProcessPath;$binDir"
    Invoke-Installer
    Invoke-Installer -Uninstall
    Assert-True ([Environment]::GetEnvironmentVariable('Path', 'User') -like "*$binDir*") 'Uninstall removed a preexisting PATH entry.'
    Write-Output 'Windows installer lifecycle checks passed.'
} finally {
    $env:LOCALAPPDATA = $originalLocal
    [Environment]::SetEnvironmentVariable('Path', $originalUserPath, 'User')
    $env:Path = $originalProcessPath
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
