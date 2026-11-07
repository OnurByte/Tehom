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
$InstallDir = if ($env:TEHOM_BIN_DIR) { $env:TEHOM_BIN_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\Tehom' }

function Fail($Message) {
    Write-Host "Tehom install stopped: $Message" -ForegroundColor Red
    throw $Message
}

if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    Fail 'Only 64-bit x86 Windows is supported for now.'
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
    Write-Host 'Downloading Tehom (Windows, x86_64)…'
    $Zip = Join-Path $Temp $Asset
    if ($env:TEHOM_ARCHIVE) {
        Copy-Item $env:TEHOM_ARCHIVE $Zip
        Copy-Item "$($env:TEHOM_ARCHIVE).sha256" "$Zip.sha256"
    } else {
        try {
            Invoke-WebRequest "$Base/$Asset" -OutFile $Zip -UseBasicParsing
            Invoke-WebRequest "$Base/$Asset.sha256" -OutFile "$Zip.sha256" -UseBasicParsing
        } catch {
            Fail "Could not download $Asset. There may be no release yet: https://github.com/$Repo/releases"
        }
    }
    $Expected = ((Get-Content "$Zip.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
    if (-not $Expected -or $Expected -ne $Actual) {
        Fail 'SHA-256 mismatch; the download is damaged or altered. Nothing was installed.'
    }

    Expand-Archive $Zip -DestinationPath $Temp -Force
    $Package = Join-Path $Temp 'tehom'
    foreach ($Binary in 'tehom.exe', 'tehom-desktop.exe') {
        if (-not (Test-Path (Join-Path $Package $Binary))) { Fail "$Binary is missing from the archive" }
    }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $Previous = Join-Path $InstallDir '.tehom-previous'
    foreach ($Binary in 'tehom.exe', 'tehom-desktop.exe') {
        $Target = Join-Path $InstallDir $Binary
        if (Test-Path $Target) {
            # Keep the replaced version for `tehom update --rollback`; a running
            # .exe cannot be overwritten but can be renamed aside.
            New-Item -ItemType Directory -Force -Path $Previous | Out-Null
            Copy-Item $Target (Join-Path $Previous $Binary) -Force
            $Aside = Join-Path $InstallDir ".$Binary.old"
            Remove-Item $Aside -Force -ErrorAction SilentlyContinue
            Rename-Item $Target $Aside
        }
        Copy-Item (Join-Path $Package $Binary) $Target -Force
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
        Write-Host ('Missing tools: ' + ($Missing -join ', '))
        $Plan = 'FFmpeg and yt-dlp with winget, GStreamer from gstreamer.freedesktop.org (this user only)'
        $Answer = 'h'
        if ($env:TEHOM_NO_DEPS) {
            Write-Host "To install them: $Plan"
        } elseif ($env:TEHOM_YES) {
            $Answer = 'y'
        } else {
            Write-Host "Will install: $Plan"
            $Answer = Read-Host 'Install them now? [Y/n]'
            if (-not $Answer) { $Answer = 'y' }
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
                for ($Try = 1; $Try -le 5; $Try++) {
                    try {
                        Invoke-WebRequest "https://gstreamer.freedesktop.org/data/pkg/windows/$GstVersion/msvc/gstreamer-1.0-msvc-x86_64-$GstVersion.exe" -OutFile $Installer -UseBasicParsing
                        break
                    } catch {
                        if ($Try -eq 5) { throw }
                        Start-Sleep -Seconds (5 * $Try)
                    }
                }
                $Process = Start-Process $Installer -ArgumentList @('/VERYSILENT', '/NORESTART', '/CURRENTUSER', "/DIR=$GstRoot") -PassThru -Wait
                if ($Process.ExitCode -ne 0) { Write-Host "GStreamer could not be installed (code $($Process.ExitCode))." }
            }
        }
    }
    if (Test-Path (Join-Path $GstRoot 'bin')) { Add-UserPath (Join-Path $GstRoot 'bin') }

    Write-Host ''
    Write-Host ("Installed " + (& (Join-Path $InstallDir 'tehom.exe') --version) + " in $InstallDir")
    Write-Host 'Open "Tehom" from the Start menu, or run tehom --help in a new terminal'
    Write-Host 'For AI clips, add your own API key in Settings, or run a local model such as Ollama.'
} finally {
    Remove-Item $Temp -Recurse -Force -ErrorAction SilentlyContinue
}
