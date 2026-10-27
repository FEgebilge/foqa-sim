# Write-Layout: writes an MSFS package's layout.json (every file but
# layout.json and manifest.json) and returns their total size. Dot-source it.
function Write-Layout($package) {
    $package = (Resolve-Path $package).Path
    $files = Get-ChildItem $package -Recurse -File | Where-Object { $_.Name -notin 'layout.json', 'manifest.json' } | Sort-Object FullName
    $content = $files | ForEach-Object {
        [ordered]@{ path = $_.FullName.Substring($package.Length + 1).Replace('\', '/'); size = $_.Length; date = $_.LastWriteTimeUtc.ToFileTimeUtc() }
    }
    [IO.File]::WriteAllText((Join-Path $package 'layout.json'), (ConvertTo-Json @{ content = @($content) } -Depth 4))
    ($files | Measure-Object Length -Sum).Sum
}

# Get-AppVersion: the app version (tauri.conf.json), used as the packages' package_version.
function Get-AppVersion {
    (Get-Content (Join-Path $PSScriptRoot '..\app\src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
}

# Write-Manifest: writes manifest.json for a Community package.
function Write-Manifest($package, $title, $size) {
    $manifest = [ordered]@{
        dependencies = @(); content_type = 'MISC'; title = $title; manufacturer = 'FOQA-Sim'; creator = 'Ege Bilge'
        package_version = Get-AppVersion; minimum_game_version = '1.9.21'; minimum_compatibility_version = '9.21.0.251'
        export_type = 'Community'; builder = 'Microsoft Flight Simulator 2024'; package_order_hint = ''
        release_notes = [ordered]@{ neutral = [ordered]@{ LastUpdate = ''; OlderHistory = '' } }
        total_package_size = [string]$size
    }
    [IO.File]::WriteAllText((Join-Path $package 'manifest.json'), (ConvertTo-Json $manifest -Depth 4))
}
