# Authenticode-signs a file when a certificate is configured
# (WINDOWS_CERT_PFX: base64 .pfx, WINDOWS_CERT_PASSWORD); otherwise does
# nothing, and the download stays unsigned (SmartScreen warns once).
param([Parameter(Mandatory)] [string] $File)
$ErrorActionPreference = "Stop"
if (-not $env:WINDOWS_CERT_PFX) {
    Write-Host "No signing certificate configured; $File stays unsigned."
    exit 0
}
$pfx = Join-Path $env:RUNNER_TEMP "sign.pfx"
[IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX))
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object FullName | Select-Object -Last 1
& $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERT_PASSWORD /fd SHA256 `
    /tr http://timestamp.digicert.com /td SHA256 $File
if ($LASTEXITCODE -ne 0) { throw "signing $File failed" }
& $signtool.FullName verify /pa $File
Remove-Item $pfx
