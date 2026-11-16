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
$startupLog = Join-Path $env:RUNNER_TEMP 'BankingCraft-startup.log'
# Hosted Windows VMs do not guarantee a usable OpenGL driver. Use a verified,
# test-only software driver before launch: production diagnostics intentionally
# omit error payloads, so they cannot reliably classify graphics-context failures.
$mesa = Join-Path $env:RUNNER_TEMP 'BankingCraft-mesa'
$archive = "$mesa.7z"
# 26.2.6's MSVC distribution omits OpenGL (documented upstream regression); x64 MinGW includes it.
Invoke-WebRequest 'https://github.com/pal1000/mesa-dist-win/releases/download/26.2.6/mesa3d-26.2.6-release-mingw.7z' -OutFile $archive
if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'da0f71d9181b672c1918f3c342f11118c687f6059a46b16502845160733cc388') {
    throw 'Mesa download checksum mismatch'
}
7z x $archive "-o$mesa" -y | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not extract Mesa' }
$softwareFiles = @()
Get-ChildItem (Join-Path $mesa 'x64') -Filter '*.dll' | ForEach-Object {
    $target = Join-Path $destination $_.Name
    if (-not (Test-Path $target)) {
        Copy-Item $_.FullName $target
        $softwareFiles += $target
    }
}
if (-not (Test-Path (Join-Path $destination 'opengl32.dll'))) { throw 'Mesa OpenGL driver missing' }
$env:GALLIUM_DRIVER = 'llvmpipe'
$env:LIBGL_ALWAYS_SOFTWARE = '1'
try {
    python3 scripts/smoke-desktop.py (Join-Path $destination 'BankingCraft.exe') --log $startupLog
    if ($LASTEXITCODE -ne 0) { throw 'Installed application did not start with software OpenGL' }
} finally {
    $softwareFiles | ForEach-Object { Remove-Item $_ -Force }
    Remove-Item Env:GALLIUM_DRIVER, Env:LIBGL_ALWAYS_SOFTWARE
}
$process = Start-Process -FilePath (Join-Path $destination 'unins000.exe') -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Uninstaller failed: $($process.ExitCode)" }
if (Test-Path (Join-Path $destination 'BankingCraft.exe')) { throw 'Uninstaller left the executable behind' }
Write-Output 'Installation, application startup and uninstallation passed.'
