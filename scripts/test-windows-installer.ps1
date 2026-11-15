$ErrorActionPreference = 'Stop'
$version = python3 scripts/release-version.py
if ($LASTEXITCODE -ne 0) { throw 'Invalid release version' }
$destination = Join-Path $env:RUNNER_TEMP 'BankingCraft-install-test'
$installer = Join-Path $PWD "dist/BankingCraft-$version-windows-x86_64-setup.exe"
$log = Join-Path $env:RUNNER_TEMP 'BankingCraft-setup.log'
$process = Start-Process -FilePath $installer -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$destination`"", "/LOG=`"$log`"") -Wait -PassThru
if ($process.ExitCode -ne 0) {
    Get-Content $log
    throw "Installer failed: $($process.ExitCode)"
}
foreach ($file in @('BankingCraft.exe', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'licenses/manifest.json', 'vcruntime140.dll', 'unins000.exe')) {
    if (-not (Test-Path (Join-Path $destination $file))) { throw "Installed file missing: $file" }
}
python3 scripts/smoke-desktop.py (Join-Path $destination 'BankingCraft.exe')
if ($LASTEXITCODE -ne 0) { throw 'Installed application did not start' }
$process = Start-Process -FilePath (Join-Path $destination 'unins000.exe') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Uninstaller failed: $($process.ExitCode)" }
if (Test-Path (Join-Path $destination 'BankingCraft.exe')) { throw 'Uninstaller left the executable behind' }
Write-Output 'Installation, application startup and uninstallation passed.'
