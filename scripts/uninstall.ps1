$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$BinName = "orgplug.exe"
$InstallDir = Join-Path $HOME ".local\bin"
$StateDir = Join-Path $HOME ".orgplug"
$BinPath = Join-Path $InstallDir $BinName
$Purge = $true

if ($args.Length -gt 0 -and $args[0] -eq "--keep-state") {
  $Purge = $false
}

if (Test-Path $BinPath) {
  Remove-Item -Force $BinPath
  Write-Host "Removed $BinPath"
} else {
  Write-Host "Binary not found: $BinPath"
}

if ($Purge) {
  if (Test-Path $StateDir) {
    Remove-Item -Recurse -Force $StateDir
    Write-Host "Removed $StateDir"
  } else {
    Write-Host "State directory not found: $StateDir"
  }
} else {
  Write-Host "Kept $StateDir"
  Write-Host "Run with --keep-state to keep managed state and config"
}

Write-Host "Uninstall complete"
