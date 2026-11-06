<#
.SYNOPSIS
  Signs cursorpie.exe with a certificate you create yourself, and trusts that certificate on THIS PC.

.DESCRIPTION
  No outside party is involved. What it does and does not do:
    * On the PC where you run this (and any PC where you import the .cer file), Windows sees a
      valid signature from a publisher you chose, which antivirus and SmartScreen treat better
      than an anonymous file.
    * On every other PC nothing changes: a self-signed certificate is not trusted there. For
      other people's PCs only a certificate from a public authority (or SignPath) helps.

  Run in an elevated PowerShell (the certificate is added to the machine's trusted stores):
      powershell -ExecutionPolicy Bypass -File scripts\sign-local.ps1 -Exe target\release\cursorpie.exe
#>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Subject = 'CN=CursorPie (self-signed), O=Sebastvnd',
    [int]$Years = 5
)
$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Exe)) { throw "File not found: $Exe" }
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw 'Run this in an elevated (administrator) PowerShell.' }

# reuse the certificate from an earlier run, so every build has the same publisher identity
$cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert |
    Where-Object { $_.Subject -eq $Subject -and $_.NotAfter -gt (Get-Date).AddDays(30) } |
    Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject `
        -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
        -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears($Years)
    Write-Host "Created certificate $($cert.Thumbprint)"
}

# trust it on this PC: as a root, and as a trusted publisher
$cer = Join-Path $env:TEMP 'cursorpie-selfsigned.cer'
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null

$sig = Set-AuthenticodeSignature -FilePath $Exe -Certificate $cert -HashAlgorithm SHA256
if ($sig.Status -ne 'Valid') { throw "Signing failed: $($sig.Status) $($sig.StatusMessage)" }
Write-Host "Signed $Exe (publisher: $Subject)."

$out = Join-Path (Split-Path -Parent (Resolve-Path $Exe)) 'cursorpie-selfsigned.cer'
Copy-Item $cer $out -Force
Write-Host "To trust it on another PC of yours, import $out into 'Trusted Root Certification Authorities' and 'Trusted Publishers'."
