# Tehom installer for Windows. In PowerShell:
#
#   irm https://raw.githubusercontent.com/OnurByte/Tehom/main/install.ps1 | iex
#
# Installs tehom.exe, tehom-desktop.exe and the bundled whisper-cli.exe for the
# current user only into %LOCALAPPDATA%\Programs\Tehom after checking the
# release archive's SHA-256, adds it to the user PATH and creates a Start menu
# shortcut. GStreamer comes with the archive. Missing FFmpeg or yt-dlp are
# listed and installed only if you answer yes.
#
# Options (environment variables): TEHOM_VERSION, TEHOM_YES=1, TEHOM_NO_DEPS=1,
# TEHOM_ARCHIVE=C:\path\tehom-windows-x86_64.zip (with .sha256 beside it).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = if ($env:TEHOM_REPO) { $env:TEHOM_REPO } else { 'OnurByte/Tehom' }
$Version = if ($env:TEHOM_VERSION) { $env:TEHOM_VERSION } else { 'latest' }
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

    # Bundled GStreamer, installed before the programs that need it. A DLL in
    # use cannot be overwritten but can be moved aside; .tehom-old is emptied
    # by the next install.
    $Old = Join-Path $InstallDir '.tehom-old'
    Remove-Item $Old -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $Old | Out-Null
    foreach ($Folder in '.', 'gstreamer-plugins') {
        $Destination = Join-Path $InstallDir $Folder
        New-Item -ItemType Directory -Force -Path $Destination | Out-Null
        Get-ChildItem (Join-Path $Package $Folder) -File |
            Where-Object { $_.Extension -eq '.dll' -or $_.Name -in 'gst-plugin-scanner.exe', 'THIRD-PARTY-NOTICES.txt' } |
            ForEach-Object {
                $Target = Join-Path $Destination $_.Name
                if (Test-Path -LiteralPath $Target) {
                    Move-Item -LiteralPath $Target (Join-Path $Old ([Guid]::NewGuid()))
                }
                Copy-Item -LiteralPath $_.FullName $Target
            }
    }

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

    # Bundled speech-to-text engine, looked up beside tehom-desktop.exe;
    # archives from before it was bundled do not have it. A running copy is
    # moved aside into .tehom-old like the DLLs.
    foreach ($Extra in 'whisper-cli.exe', 'whisper.cpp-LICENSE.txt') {
        $Source = Join-Path $Package $Extra
        if (Test-Path -LiteralPath $Source) {
            $Target = Join-Path $InstallDir $Extra
            if (Test-Path -LiteralPath $Target) {
                Move-Item -LiteralPath $Target (Join-Path $Old ([Guid]::NewGuid()))
            }
            Copy-Item -LiteralPath $Source $Target
        }
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
    $Link = Join-Path $StartMenu 'Tehom.lnk'
    $Desktop = Join-Path $InstallDir 'tehom-desktop.exe'
    $Shell = New-Object -ComObject WScript.Shell
    $Shortcut = $Shell.CreateShortcut($Link)
    $Shortcut.TargetPath = $Desktop
    $Shortcut.WorkingDirectory = $InstallDir
    $Shortcut.IconLocation = "$Desktop,0"
    $Shortcut.Save()

    # The AppUserModelID tehom-desktop gives itself (org.tehom.Tehom), so the
    # taskbar groups its window under this shortcut. WScript.Shell cannot set
    # it; IPropertyStore can. C# 5 so Windows PowerShell 5.1 compiles it too.
    try {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class TehomShortcut {
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    class ShellLink {}

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("0000010b-0000-0000-C000-000000000046")]
    interface IPersistFile {
        void GetClassID(out Guid id);
        [PreserveSig] int IsDirty();
        void Load([MarshalAs(UnmanagedType.LPWStr)] string file, uint mode);
        void Save([MarshalAs(UnmanagedType.LPWStr)] string file, [MarshalAs(UnmanagedType.Bool)] bool remember);
        void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string file);
        void GetCurFile([MarshalAs(UnmanagedType.LPWStr)] out string file);
    }

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99")]
    interface IPropertyStore {
        void GetCount(out uint count);
        void GetAt(uint index, out PropertyKey key);
        void GetValue(ref PropertyKey key, out PropVariant value);
        void SetValue(ref PropertyKey key, ref PropVariant value);
        void Commit();
    }

    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    struct PropertyKey { public Guid Format; public uint Id; }

    // PROPVARIANT holding a VT_LPWSTR; 24 bytes on x64.
    [StructLayout(LayoutKind.Explicit, Size = 24)]
    struct PropVariant { [FieldOffset(0)] public ushort Type; [FieldOffset(8)] public IntPtr Value; }

    public static void SetAppId(string link, string appId) {
        object shellLink = new ShellLink();
        try {
            IPersistFile file = (IPersistFile)shellLink;
            file.Load(link, 2); // STGM_READWRITE
            PropertyKey key = new PropertyKey();
            key.Format = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"); // PKEY_AppUserModel_ID
            key.Id = 5;
            PropVariant value = new PropVariant();
            value.Type = 31; // VT_LPWSTR
            value.Value = Marshal.StringToCoTaskMemUni(appId);
            try {
                IPropertyStore store = (IPropertyStore)shellLink;
                store.SetValue(ref key, ref value);
                store.Commit();
            } finally {
                Marshal.FreeCoTaskMem(value.Value);
            }
            file.Save(link, true);
        } finally {
            Marshal.ReleaseComObject(shellLink);
        }
    }
}
'@
        [TehomShortcut]::SetAppId($Link, 'org.tehom.Tehom')
    } catch {
        Write-Warning "The Start menu shortcut works, but its taskbar grouping ID could not be set: $_"
    }

    # --- Tools Tehom runs: FFmpeg, yt-dlp -----------------------------------
    $Missing = @()
    foreach ($Tool in 'ffmpeg', 'ffprobe', 'yt-dlp') {
        if (-not (Get-Command $Tool -ErrorAction SilentlyContinue)) { $Missing += $Tool }
    }

    if ($Missing.Count -gt 0) {
        Write-Host ''
        Write-Host ('Missing tools: ' + ($Missing -join ', '))
        $Plan = 'FFmpeg and yt-dlp with winget (this user only)'
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
        }
    }

    Write-Host ''
    Write-Host ("Installed " + (& (Join-Path $InstallDir 'tehom.exe') --version) + " in $InstallDir")
    Write-Host 'Open "Tehom" from the Start menu, or run tehom --help in a new terminal'
    Write-Host 'For AI clips, add your own API key in Settings, or run a local model such as Ollama.'
} finally {
    Remove-Item $Temp -Recurse -Force -ErrorAction SilentlyContinue
}
