# Builds target\bundle\CursorPie-windows.zip containing cursorpie.exe (tray app, no console window)
# and, when Inno Setup 6 is installed, the Setup wizard target\bundle\CursorPie-<version>-setup.exe.
# Usage (PowerShell, MSVC build tools installed):
#   scripts\package-windows.ps1                 zip, plus the installer if Inno Setup is found
#   scripts\package-windows.ps1 -NoInstaller    zip only
# Optional code signing (removes the SmartScreen warning once the certificate has a reputation):
#   $env:SIGN_CERT_THUMBPRINT = "<SHA-1 thumbprint of a code signing certificate in your store>"
#   (needs signtool.exe from the Windows SDK on PATH)
param([switch]$NoInstaller)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.*)"' | Select-Object -First 1).Matches.Groups[1].Value

function Sign-File([string]$path) {
    if (-not $env:SIGN_CERT_THUMBPRINT) { return }
    signtool sign /sha1 $env:SIGN_CERT_THUMBPRINT /fd sha256 /tr http://timestamp.digicert.com /td sha256 $path
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $path" }
}

# CURSORPIE_CARGO_ARGS: more arguments for cargo, e.g. "--features debug-log" (the build with the log)
$cargoArgs = @()
if ($env:CURSORPIE_CARGO_ARGS) { $cargoArgs = $env:CURSORPIE_CARGO_ARGS -split "\s+" | Where-Object { $_ } }
cargo build --release @cargoArgs
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
Sign-File "target\release\cursorpie.exe"

$out = "target\bundle"
$stage = Join-Path $out "CursorPie"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "target\release\cursorpie.exe" (Join-Path $stage "cursorpie.exe")
Copy-Item LICENSE, README.md $stage

$zip = Join-Path $out "CursorPie-windows.zip"
Remove-Item -Force $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
Write-Host "Built $zip (version $version)"

if (-not $NoInstaller) {
    $candidates = @(
        (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
    )
    $iscc = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if ($iscc) {
        & $iscc "/DAppVersion=$version" "installer\cursorpie.iss"
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
        $setup = Join-Path $out "CursorPie-$version-setup.exe"
        Sign-File $setup
        Write-Host "Built $setup"
    } else {
        Write-Host "Inno Setup 6 not found, so no installer was built (winget install JRSoftware.InnoSetup)."
    }
}
