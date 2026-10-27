# Builds the FOQA-Sim EFB app package into msfs-panel\out\Packages\foqa-sim-efb.
# An EFB app is plain files (no fspackagetool), so the sim may stay running.
# The EFB OS loads one script per app folder, so the board (from the toolbar
# panel's sources, never forked), the history and the weather setter go ahead of the app in it;
# the stylesheet likewise.
$panel = Join-Path $PSScriptRoot '..\PackageSources\html_ui\InGamePanels\FOQASim'
$package = Join-Path $PSScriptRoot '..\out\Packages\foqa-sim-efb'
$app = Join-Path $package 'html_ui\efb_ui\efb_apps\FoqaSimApp'
Remove-Item -Recurse -Force $package -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $app 'Assets') | Out-Null

function Join-Files($out, $files) {
    [IO.File]::WriteAllText((Join-Path $app $out), (($files | ForEach-Object { [IO.File]::ReadAllText($_) }) -join "`n"))
}
Join-Files 'FoqaSimApp.js' @((Join-Path $panel 'foqa-board.js'), (Join-Path $PSScriptRoot 'foqa-history.js'), (Join-Path $PSScriptRoot 'foqa-weather.js'), (Join-Path $PSScriptRoot 'FoqaSimApp.js'))
Join-Files 'FoqaSimApp.css' @((Join-Path $panel 'FOQASim.css'), (Join-Path $PSScriptRoot 'FoqaSimApp.css'))
Copy-Item (Join-Path $PSScriptRoot '..\PackageSources\html_ui\icons\toolbar\ICON_TOOLBAR_FOQA_SIM.svg') (Join-Path $app 'Assets\app-icon.svg')

. (Join-Path $PSScriptRoot '..\layout.ps1')
$size = Write-Layout $package
Write-Manifest $package 'FOQA-Sim EFB' $size
Write-Output "built $package"
