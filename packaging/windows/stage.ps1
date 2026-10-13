# Assembles the portable folder: the exe, FFmpeg's DLLs and licence notices.
# Usage: stage.ps1 -FfmpegDir <dir> -Out <dir>
param(
    [Parameter(Mandatory)] [string] $FfmpegDir,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\.."

New-Item -ItemType Directory -Force -Path $Out | Out-Null
Copy-Item "$root\target\release\reelwright.exe" "$Out\Reelwright.exe"

# FFmpeg is LGPL and must stay replaceable: ship its DLLs as separate files.
Get-ChildItem "$FfmpegDir\bin\*.dll" | Copy-Item -Destination $Out

$lic = New-Item -ItemType Directory -Force -Path "$Out\licenses"
Copy-Item "$root\LICENSE-MIT", "$root\LICENSE-APACHE" $lic
Copy-Item "$root\assets\fonts\ui\Inter-OFL.txt" $lic
# Bundled fonts (OFL) and stickers (CC BY-SA): their licences and the
# per-file attribution table.
$fonts = New-Item -ItemType Directory -Force -Path "$lic\fonts"
Get-ChildItem "$root\assets\fonts\title\*\OFL.txt" | ForEach-Object {
    Copy-Item $_.FullName "$fonts\$($_.Directory.Name)-OFL.txt"
}
Copy-Item "$root\assets\stickers\openmoji\LICENSE.txt" "$lic\OpenMoji-CC-BY-SA-4.0.txt"
Copy-Item "$root\assets\ATTRIBUTION.md" "$lic\ATTRIBUTION.md"
if (Test-Path "$root\THIRD_PARTY_LICENSES.html") { Copy-Item "$root\THIRD_PARTY_LICENSES.html" $lic }
$ff = New-Item -ItemType Directory -Force -Path "$lic\FFmpeg"
# Licence text and exact configure line, written by scripts/ffmpeg.
Get-ChildItem "$FfmpegDir\share\licenses\ffmpeg\*" -ErrorAction SilentlyContinue | Copy-Item -Destination $ff
@"
FFmpeg is used under the GNU Lesser General Public License v2.1 (LGPL build:
no GPL or non-free components). Its DLLs in this folder can be replaced
with any compatible FFmpeg build of the same major version.

Built from the official source release listed in CONFIGURATION.txt
(https://ffmpeg.org/releases/) by scripts/ffmpeg/build-windows.sh in the
Reelwright source repository, with the configure options shown there.
"@ | Set-Content "$ff\SOURCE.txt"

@"
Reelwright — offline video editor

Run Reelwright.exe. Nothing else needs installing.
Projects are saved in your Videos\Reelwright folder.
Logs: %LOCALAPPDATA%\Reelwright\reelwright\data\logs
"@ | Set-Content "$Out\README.txt"
Write-Host "Staged to $Out"
Get-ChildItem $Out | Format-Table Name, Length
