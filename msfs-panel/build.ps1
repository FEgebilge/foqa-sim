# Builds the FOQA-Sim toolbar panel package into msfs-panel\out\Packages.
# fspackagetool attaches to (or starts) MSFS 2024, which compiles the panel
# definition to .spb. SDK path: $env:MSFS2024_SDK, else the default install.
# The build runs in a sim instance of its own, which cannot start beside a
# running one (the MS Store build then only focuses it and nothing is built).
if (Get-Process FlightSimulator2024 -ErrorAction SilentlyContinue) { throw 'close MSFS 2024 first: fspackagetool builds in its own sim instance' }
$sdk = $env:MSFS2024_SDK
if (-not $sdk) { $sdk = 'C:\MSFS 2024 SDK' }
# The tool can keep waiting on the sim after the .spb is written, so wait for
# the output instead of the tool, then stop it and write layout.json.
$package = Join-Path $PSScriptRoot 'out\Packages\foqa-sim-panel'
$spb = Join-Path $package 'InGamePanels\InGamePanel_FOQASim.spb'
Remove-Item -Recurse -Force (Join-Path $PSScriptRoot 'out') -ErrorAction SilentlyContinue
$tool = Start-Process (Join-Path $sdk 'Tools\bin\fspackagetool.exe') -ArgumentList ('"' + (Join-Path $PSScriptRoot 'foqa-sim-panel.xml') + '"'), '-nopause' -PassThru -NoNewWindow
$deadline = (Get-Date).AddMinutes(10)
while (-not $tool.HasExited -and -not (Test-Path $spb) -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 2 }
Start-Sleep -Seconds 5
if (-not $tool.HasExited) { Stop-Process -Id $tool.Id }
if (-not (Test-Path $spb)) { throw "no panel definition in $package (see out\_PackageInt\_RPTErrors.xml)" }

. (Join-Path $PSScriptRoot 'layout.ps1')
Copy-Item $spb (Join-Path $PSScriptRoot 'prebuilt') -Force  # keep the prebuilt copy (used by assemble.ps1) current
$null = Write-Layout $package
Write-Output "built $package"
