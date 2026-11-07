# Tehom installer for Windows. In PowerShell:
#
#   irm https://raw.githubusercontent.com/OnurByte/Tehom/main/install.ps1 | iex
#
# Installs tehom.exe and tehom-desktop.exe for the current user only into
# %LOCALAPPDATA%\Programs\Tehom after checking the release archive's SHA-256,
# adds it to the user PATH and creates a Start menu shortcut. Missing FFmpeg,
# yt-dlp or GStreamer are listed and installed only if you answer yes.
#
# Options (environment variables): TEHOM_VERSION, TEHOM_YES=1, TEHOM_NO_DEPS=1,
# TEHOM_ARCHIVE=C:\path\tehom-windows-x86_64.zip (with .sha256 beside it).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = if ($env:TEHOM_REPO) { $env:TEHOM_REPO } else { 'OnurByte/Tehom' }
$Version = if ($env:TEHOM_VERSION) { $env:TEHOM_VERSION } else { 'latest' }
$GstVersion = '1.28.8'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\Tehom'

function Fail($Message) {
    Write-Host "Tehom kurulumu durdu: $Message" -ForegroundColor Red
    throw $Message
}

if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    Fail 'Şimdilik yalnızca 64 bit x86 Windows destekleniyor.'
}

$Asset = 'tehom-windows-x86_64.zip'
if ($Version -eq 'latest') {
    $Base = "https://github.com/$Repo/releases/latest/download"
} else {
    $Base = "https://github.com/$Repo/releases/download/$Version"
}
$Temp = Join-Path ([IO.Path]::GetTempPath()) ("tehom-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $Temp | Out-Null

try {
    Write-Host 'Tehom indiriliyor (Windows, x86_64)…'
    $Zip = Join-Path $Temp $Asset
    if ($env:TEHOM_ARCHIVE) {
        Copy-Item $env:TEHOM_ARCHIVE $Zip
        Copy-Item "$($env:TEHOM_ARCHIVE).sha256" "$Zip.sha256"
    } else {
        try {
            Invoke-WebRequest "$Base/$Asset" -OutFile $Zip -UseBasicParsing
            Invoke-WebRequest "$Base/$Asset.sha256" -OutFile "$Zip.sha256" -UseBasicParsing
        } catch {
            Fail "$Asset indirilemedi. Henüz yayımlanmış bir sürüm olmayabilir: https://github.com/$Repo/releases"
        }
    }
    $Expected = ((Get-Content "$Zip.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
    if (-not $Expected -or $Expected -ne $Actual) {
        Fail 'SHA-256 tutmuyor; dosya bozuk ya da değiştirilmiş, hiçbir şey kurulmadı.'
    }

    Expand-Archive $Zip -DestinationPath $Temp -Force
    $Package = Join-Path $Temp 'tehom'
    foreach ($Binary in 'tehom.exe', 'tehom-desktop.exe') {
        if (-not (Test-Path (Join-Path $Package $Binary))) { Fail "arşivde $Binary yok" }
    }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($Binary in 'tehom.exe', 'tehom-desktop.exe') {
        Copy-Item (Join-Path $Package $Binary) (Join-Path $InstallDir $Binary) -Force
    }

    function Add-UserPath($Directory) {
        $Current = [Environment]::GetEnvironmentVariable('Path', 'User')
        if (-not $Current) { $Current = '' }
        if (($Current -split ';') -notcontains $Directory) {
            [Environment]::SetEnvironmentVariable('Path', ($Current.TrimEnd(';') + ";$Directory").TrimStart(';'), 'User')
        }
        if (($env:Path -split ';') -notcontains $Directory) { $env:Path += ";$Directory" }
    }
    Add-UserPath $InstallDir

    $StartMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
    $Shell = New-Object -ComObject WScript.Shell
    $Shortcut = $Shell.CreateShortcut((Join-Path $StartMenu 'Tehom.lnk'))
    $Shortcut.TargetPath = Join-Path $InstallDir 'tehom-desktop.exe'
    $Shortcut.WorkingDirectory = $InstallDir
    $Shortcut.Save()

    # --- Tools Tehom runs: FFmpeg, yt-dlp, GStreamer -----------------------
    $GstRoot = Join-Path $env:LOCALAPPDATA 'Programs\GStreamer'
    $Missing = @()
    foreach ($Tool in 'ffmpeg', 'ffprobe', 'yt-dlp') {
        if (-not (Get-Command $Tool -ErrorAction SilentlyContinue)) { $Missing += $Tool }
    }
    $HasGst = (Get-Command 'gst-inspect-1.0' -ErrorAction SilentlyContinue) -or (Test-Path (Join-Path $GstRoot 'bin'))
    if (-not $HasGst) { $Missing += 'gstreamer' }

    if ($Missing.Count -gt 0) {
        Write-Host ''
        Write-Host ('Eksik araçlar: ' + ($Missing -join ', '))
        $Plan = 'winget ile FFmpeg ve yt-dlp, gstreamer.freedesktop.org üzerinden GStreamer (yalnız bu kullanıcı için)'
        $Answer = 'h'
        if ($env:TEHOM_NO_DEPS) {
            Write-Host "Kurmak için: $Plan"
        } elseif ($env:TEHOM_YES) {
            $Answer = 'e'
        } else {
            Write-Host "Kurulacak: $Plan"
            $Answer = Read-Host 'Şimdi kurayım mı? [E/h]'
            if (-not $Answer) { $Answer = 'e' }
        }
        if ($Answer -match '^[EeYy]') {
            if (($Missing -contains 'ffmpeg') -or ($Missing -contains 'ffprobe')) {
                winget install --id Gyan.FFmpeg -e --scope user --accept-package-agreements --accept-source-agreements
            }
            if ($Missing -contains 'yt-dlp') {
                winget install --id yt-dlp.yt-dlp -e --scope user --accept-package-agreements --accept-source-agreements
            }
            if ($Missing -contains 'gstreamer') {
                $Installer = Join-Path $Temp 'gstreamer.exe'
                Invoke-WebRequest "https://gstreamer.freedesktop.org/data/pkg/windows/$GstVersion/msvc/gstreamer-1.0-msvc-x86_64-$GstVersion.exe" -OutFile $Installer -UseBasicParsing
                $Process = Start-Process $Installer -ArgumentList @('/VERYSILENT', '/NORESTART', '/CURRENTUSER', "/DIR=$GstRoot") -PassThru -Wait
                if ($Process.ExitCode -ne 0) { Write-Host "GStreamer kurulamadı (kod $($Process.ExitCode))." }
            }
        }
    }
    if (Test-Path (Join-Path $GstRoot 'bin')) { Add-UserPath (Join-Path $GstRoot 'bin') }

    Write-Host ''
    Write-Host ("Kuruldu: " + (& (Join-Path $InstallDir 'tehom.exe') --version) + " → $InstallDir")
    Write-Host 'Başlat menüsünde "Tehom"; yeni bir terminalde: tehom --help'
    Write-Host 'Yerel bir AI modeli gerekir (ör. Ollama): https://ollama.com'
} finally {
    Remove-Item $Temp -Recurse -Force -ErrorAction SilentlyContinue
}
