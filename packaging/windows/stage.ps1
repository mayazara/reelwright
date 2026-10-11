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
if (Test-Path "$root\THIRD_PARTY_LICENSES.html") { Copy-Item "$root\THIRD_PARTY_LICENSES.html" $lic }
$ff = New-Item -ItemType Directory -Force -Path "$lic\FFmpeg"
Get-ChildItem "$FfmpegDir\LICENSE*" -ErrorAction SilentlyContinue | Copy-Item -Destination $ff
@"
FFmpeg is used under the GNU Lesser General Public License v2.1 (LGPL build:
no GPL or non-free components). Its DLLs in this folder can be replaced
with any compatible FFmpeg build.

Build used: $(Split-Path $FfmpegDir -Leaf)
Built by:   https://github.com/BtbN/FFmpeg-Builds (build scripts and exact
            source references are published there)
Source:     https://ffmpeg.org/download.html
"@ | Set-Content "$ff\SOURCE.txt"

@"
Reelwright — offline video editor

Run Reelwright.exe. Nothing else needs installing.
Projects are saved in your Videos\Reelwright folder.
Logs: %LOCALAPPDATA%\Reelwright\reelwright\data\logs
"@ | Set-Content "$Out\README.txt"
Write-Host "Staged to $Out"
Get-ChildItem $Out | Format-Table Name, Length
