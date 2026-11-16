$archiveName = "Firebird-5.0.4.1812-0-windows-x64.zip"
$downloadUrl = "https://github.com/FirebirdSQL/firebird/releases/download/v5.0.4/$archiveName"
$outputFolder = "C:\Program Files\Firebird"
$outputPath = Join-Path $outputFolder $archiveName

New-Item -Path $outputFolder -ItemType "Directory"

Write-Host "Download $archiveName..."
try {
    Invoke-WebRequest -Uri $downloadUrl -OutFile $outputPath -SslProtocol Tls12
    Write-Host "File successfully written: $outputPath"
} catch {
    Write-Host "Download with cert check failed: $_"
    Write-Host "Retrying with -SkipCertificateCheck..."
    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $outputPath -SslProtocol Tls12 -SkipCertificateCheck
        Write-Host "File successfully written: $outputPath"
    } catch {
        Write-Error "Error download file: $_"
        exit
    }
}

Expand-Archive -Path $outputPath -DestinationPath $outputFolder -Force

Remove-Item $outputPath

Set-Location -Path $outputFolder

$currentPath = Get-Location

$runServiceFilename = "./install_service.bat"
& "$runServiceFilename"

Set-Location $currentPath
