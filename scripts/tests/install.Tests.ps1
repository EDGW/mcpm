# Exercise the Windows installer against an offline suite, including rollback and PATH ownership.
param([Parameter(Mandatory = $true)][string]$BinaryDir)

$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$installer = Join-Path $repo 'scripts/install.ps1'
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('mcpm-installer-test-' + [guid]::NewGuid().ToString('N'))
$originalLocal = $env:LOCALAPPDATA
$originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$originalProcessPath = $env:Path
$testState = [pscustomobject]@{
    FailCommit = $false
    Answer = 'yes'
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

function Read-Host {
    param([string]$Prompt)
    if ($Prompt -notlike '*Replace and delete all previous contents?*') { throw "Unexpected prompt: $Prompt" }
    return $testState.Answer
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
    python (Join-Path $repo 'scripts/package.py') --binary-dir $BinaryDir --output-dir $testState.ReleaseDir --asset mcpm-windows-x86_64 --target x86_64-pc-windows-msvc --tag $testState.ReleaseVersion
    if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare Windows fixture archive.' }
    $archivePath = Join-Path $testState.ReleaseDir 'mcpm-windows-x86_64.zip'
    $expectedFiles = (Get-Content -LiteralPath (Join-Path $testState.ReleaseDir 'manifest.json') -Raw | ConvertFrom-Json).files.Count
    $env:LOCALAPPDATA = Join-Path $testRoot 'local app data'
    $installDir = Join-Path $env:LOCALAPPDATA 'Programs\mcpm'
    $binDir = Join-Path $installDir 'bin'
    $manifestPath = Join-Path $installDir 'manifest'
    $recordPath = Join-Path $env:LOCALAPPDATA 'mcpm\mcpmd.path'
    $daemonPath = Join-Path $binDir 'mcpmd.exe'
    foreach ($version in @('v0.1.0-dev.26w45a', 'v0.1.0-rc0', 'v0.1.0-rc.1', 'v0.1.0-alpha', 'v0.0.9')) {
        Invoke-Installer -Version $version -ExpectFailure
    }
    Assert-True ($testState.Downloads -eq 0) 'Unsupported versions reached the download step.'
    # Declining replacement keeps a legacy executable and does not download.
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    $legacy = Join-Path $installDir 'mcpm.exe'
    [IO.File]::WriteAllText($legacy, 'legacy executable')
    [Environment]::SetEnvironmentVariable('Path', "$originalUserPath;$installDir", 'User')
    $env:Path = "$originalProcessPath;$installDir"
    $testState.Answer = 'no'
    Invoke-Installer
    Assert-True ((Test-Path -LiteralPath $legacy) -and $testState.Downloads -eq 0) 'Declining replacement changed the old installation or reached the download.'
    $testState.Answer = 'yes'
    $output = (& $installer -Version $testState.ReleaseVersion) -join "`n"
    $validated = $output.IndexOf('Validated ')
    $installed = $output.IndexOf('Installed ')
    Assert-True ($validated -ge 0 -and $installed -gt $validated -and $output.IndexOf('Welcome to mcpm ') -gt $installed) 'Install did not validate before displaying the welcome guide.'
    $files = @(Get-ChildItem -LiteralPath $binDir)
    Assert-True ($files.Count -eq $expectedFiles) 'The complete suite was not installed.'
    Assert-True (-not (Test-Path -LiteralPath $legacy)) 'Approved replacement retained the legacy executable.'
    Assert-True (Test-Path -LiteralPath (Join-Path $binDir 'mcloader-injector-agent.jar')) 'Loader agent was not installed.'
    Assert-True ([IO.File]::ReadAllText($recordPath) -ceq "$daemonPath`n") 'Daemon registration does not point at the installed daemon.'
    $originalManifest = [IO.File]::ReadAllText($manifestPath)
    Remove-Item -LiteralPath $files[0].FullName
    Invoke-Installer
    Assert-True ((Get-ChildItem -LiteralPath $binDir).Count -eq $expectedFiles) 'Repair did not restore a missing binary.'
    $foreignRecord = "C:\custom path\mcpmd.exe`n"
    [IO.File]::WriteAllText($recordPath, $foreignRecord)
    $testState.FailCommit = $true
    Invoke-Installer -ExpectFailure
    Assert-True ([IO.File]::ReadAllText($manifestPath) -ceq $originalManifest) 'Rollback did not restore the manifest.'
    Assert-True ((Get-ChildItem -LiteralPath $binDir).Count -eq $expectedFiles) 'Rollback did not restore all binaries.'
    Assert-True ([IO.File]::ReadAllText($recordPath) -ceq $foreignRecord) 'Rollback did not restore the daemon registration.'
    $lock = [IO.File]::Open((Join-Path $installDir '.install-lock'), 'Open', 'ReadWrite', 'None')
    try { Invoke-Installer -ExpectFailure } finally { $lock.Dispose() }
    $checksum = Join-Path $testState.ReleaseDir 'mcpm-windows-x86_64.zip.sha256'
    $savedChecksum = [IO.File]::ReadAllText($checksum)
    [IO.File]::WriteAllText($checksum, ('0' * 64) + '  mcpm-windows-x86_64.zip')
    Invoke-Installer -ExpectFailure
    Assert-True ([IO.File]::ReadAllText($manifestPath) -ceq $originalManifest) 'Bad download changed the installation.'
    [IO.File]::WriteAllText($checksum, $savedChecksum)
    $savedArchive = Join-Path $testRoot 'complete.zip'
    Copy-Item -LiteralPath $archivePath -Destination $savedArchive
    $zip = [IO.Compression.ZipFile]::Open($archivePath, 'Update')
    try { $zip.GetEntry('bin/mcloader-injector-agent.jar').Delete() } finally { $zip.Dispose() }
    [IO.File]::WriteAllText($checksum, (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash + '  mcpm-windows-x86_64.zip')
    Invoke-Installer -ExpectFailure
    Assert-True ([IO.File]::ReadAllText($manifestPath) -ceq $originalManifest) 'Incomplete package changed the installation.'
    Copy-Item -LiteralPath $savedArchive -Destination $archivePath -Force
    [IO.File]::WriteAllText($checksum, $savedChecksum)
    $foreign = Join-Path $binDir 'personal.txt'
    [IO.File]::WriteAllText($foreign, 'keep')
    [IO.File]::WriteAllText($recordPath, $foreignRecord)
    Invoke-Installer -Uninstall
    Assert-True (Test-Path -LiteralPath $foreign) 'Uninstall removed an unmanaged file.'
    Assert-True ([IO.File]::ReadAllText($recordPath) -ceq $foreignRecord) 'Uninstall removed a foreign daemon registration.'
    Assert-True (-not (Test-Path -LiteralPath $manifestPath)) 'Uninstall retained the manifest.'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $installDir 'manifest.json'))) 'Uninstall retained the release manifest.'
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
    Assert-True (-not (Test-Path -LiteralPath $recordPath)) 'Uninstall retained its own daemon registration.'
    Write-Output 'Windows installer lifecycle checks passed.'
} finally {
    $env:LOCALAPPDATA = $originalLocal
    [Environment]::SetEnvironmentVariable('Path', $originalUserPath, 'User')
    $env:Path = $originalProcessPath
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
