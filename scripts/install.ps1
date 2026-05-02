$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Owner = "himicoswilson"
$Repo = "orgplug"
$BinName = "orgplug.exe"
$InstallDir = Join-Path $HOME "bin"
$StateDir = Join-Path $HOME ".orgplug"
$WorkDir = Join-Path $StateDir "workdir\orgplug"
$ConfigFile = Join-Path $StateDir "config.yaml"
$DefaultConfigUrl = "https://raw.githubusercontent.com/himicoswilson/orgplug/main/config/config.yaml"
$RepoUrlDefault = "https://github.com/himicoswilson/orgplug.git"
$RepoUrl = if ($env:ORG_PLUG_REPO_URL) { $env:ORG_PLUG_REPO_URL } else { $RepoUrlDefault }
$Version = if ($env:ORG_PLUG_VERSION) { $env:ORG_PLUG_VERSION } else { "latest" }

$arch = if ([Environment]::Is64BitOperatingSystem) { "amd64" } else { throw "Unsupported architecture" }
$asset = "orgplug-windows-$arch.zip"

if ($Version -eq "latest") { $releaseBase = "https://github.com/$Owner/$Repo/releases/latest/download" }
else { $releaseBase = "https://github.com/$Owner/$Repo/releases/download/$Version" }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("orgplug-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
  $assetPath = Join-Path $tmp $asset

  Invoke-WebRequest "$releaseBase/$asset" -OutFile $assetPath -UseBasicParsing

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  New-Item -ItemType Directory -Force -Path (Join-Path $StateDir "workdir") | Out-Null
  Expand-Archive -Path $assetPath -DestinationPath $tmp -Force

  $binPath = Join-Path $tmp $BinName
  if (-not (Test-Path $binPath)) { throw "Binary $BinName not found in archive" }
  Copy-Item $binPath (Join-Path $InstallDir $BinName) -Force

  if (Test-Path (Join-Path $WorkDir ".git")) {
    git -C $WorkDir fetch --all --prune
    try { git -C $WorkDir pull --ff-only } catch { }
  } else {
    if (Test-Path $WorkDir) { Remove-Item -Recurse -Force $WorkDir }
    git clone $RepoUrl $WorkDir
  }

  git -C $WorkDir submodule sync --recursive
  git -C $WorkDir submodule update --init --recursive

  New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
  if (-not (Test-Path $ConfigFile)) {
    try {
      Invoke-WebRequest $DefaultConfigUrl -OutFile $ConfigFile -UseBasicParsing
    } catch {
      @"
version: 1

rules:
  repos:
    plugins/anthropics-skills:
      skills:
        deny: []

    plugins/knowledge-work-plugins:
      plugins:
        deny: []

  plugins: {}
"@ | Set-Content -Path $ConfigFile -Encoding UTF8
    }
  }

  $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (-not $userPath.Split(';') -contains $InstallDir) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to User PATH. Restart terminal to apply."
  }

  Write-Host "Installed orgplug to $(Join-Path $InstallDir $BinName)"
  Write-Host "Workdir: $WorkDir"
  Write-Host "Config: $ConfigFile"
  Write-Host "Run: orgplug doctor"
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
