$repoApiUrl = "https://api.github.com/repos/FirebirdSQL/firebird/releases/latest"
$outputFolder = "C:\Program Files\Firebird"
$assetNamePattern = "*-windows-x64.zip"

New-Item -Path $outputFolder -ItemType "Directory"

try {
    $release = Invoke-RestMethod -Uri $repoApiUrl
} catch {
    Write-Error "Error get releases: $_"
    exit
}

$asset = $release.assets | Where-Object { $_.name -like $assetNamePattern } | Select-Object -First 1

if (-not $asset) {
    Write-Host "File by asset '$assetNamePattern' not found."
    exit
}

$downloadUrl = $asset.browser_download_url
$outputPath = Join-Path $outputFolder $asset.name

Write-Host "Download $($asset.name)..."
try {
    Invoke-WebRequest -Uri $downloadUrl -OutFile $outputPath
    Write-Host "File successfully written: $outputPath"
} catch {
    Write-Error "Error download file: $_"
}

Expand-Archive -Path $outputPath -DestinationPath $outputFolder -Force

Remove-Item $outputPath

Set-Location -Path $outputFolder

$currentPath = Get-Location

$runServiceFilename = "./install_service.bat"
& "$runServiceFilename"

Set-Location $currentPath
