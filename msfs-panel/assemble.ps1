# Assembles both Community packages into msfs-panel\out\Packages without
# fspackagetool (the compiled panel definition comes from prebuilt\; build.ps1
# recompiles it). The Tauri build bundles the result.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'layout.ps1')
$out = Join-Path $PSScriptRoot 'out\Packages'
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
$package = Join-Path $out 'foqa-sim-panel'
New-Item -ItemType Directory -Force (Join-Path $package 'InGamePanels') | Out-Null
Copy-Item (Join-Path $PSScriptRoot 'prebuilt\InGamePanel_FOQASim.spb') (Join-Path $package 'InGamePanels')
Copy-Item (Join-Path $PSScriptRoot 'PackageSources\html_ui') $package -Recurse
Write-Manifest $package 'FOQA-Sim Panel' (Write-Layout $package)
Write-Output "assembled $package"
& (Join-Path $PSScriptRoot 'efb\build.ps1')
