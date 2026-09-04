# ==============================================================================
#  VITL Piano Desktop - Windows Native Auto-Installer & Updater
#  Usage: irm https://raw.githubusercontent.com/RandomGuy-VN/VITL-PIANO-DESKTOP/main/web/install.ps1 | iex
# ==============================================================================
$ErrorActionPreference = "Stop"

[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12 -bor [System.Net.SecurityProtocolType]::Tls13

Write-Host "  ==========================================================" -ForegroundColor Cyan
Write-Host "         VITL Piano Desktop - Windows Installer             " -ForegroundColor Cyan
Write-Host "    High-Performance Virtual Piano Autoplayer & Synth       " -ForegroundColor Cyan
Write-Host "  ==========================================================" -ForegroundColor Cyan
Write-Host ""

$installDir = "$env:LOCALAPPDATA\Programs\VITL Piano"
$exePath = "$installDir\vitl-piano.exe"
$tempZip = "$env:TEMP\vitl_piano_latest.zip"

# 1. Close any running instances
Write-Host "[1/4] Closing any active VITL Piano instances..." -ForegroundColor Yellow
Stop-Process -Name "vitl-piano" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

# 2. Determine Remote Mirrors & Claim Latest Update
Write-Host "[2/4] Claiming latest Windows release from GitHub..." -ForegroundColor Cyan

$mirrors = @(
    "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest/download/vitl-piano-windows-portable.zip",
    "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/download/v1.0-beta.2/vitl-piano-windows-portable.zip",
    "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/download/v1.0.0/vitl-piano-windows-portable.zip",
    "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/download/v1.0-beta/vitl-piano-v1.0.0-windows-portable.zip"
)

# Also dynamically check GitHub API for newest tag and asset URL
$headers = @{ "User-Agent" = "VITL-Piano-Installer" }
try {
    $latestRel = Invoke-RestMethod -Uri "https://api.github.com/repos/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest" -Headers $headers -TimeoutSec 5
    if ($latestRel -and $latestRel.assets) {
        $foundAsset = $latestRel.assets | Where-Object { $_.name -like "*portable*.zip" -or $_.name -like "*windows*.zip" } | Select-Object -First 1
        if ($foundAsset) {
            $mirrors = @($foundAsset.browser_download_url) + $mirrors
            Write-Host "Detected latest online release: $($latestRel.name) ($($latestRel.tag_name))" -ForegroundColor Green
        }
    }
} catch {
    Write-Host "Checking release mirrors directly..." -ForegroundColor DarkGray
}

# 3. Download from mirrors
Write-Host "[3/4] Downloading latest release package..." -ForegroundColor Cyan
if (Test-Path $tempZip) { Remove-Item -Force $tempZip -ErrorAction SilentlyContinue }

$downloaded = $false
foreach ($url in $mirrors) {
    if (-not $url) { continue }
    Write-Host "Attempting download from: $url" -ForegroundColor DarkGray
    
    # Try curl.exe if available (fastest and handles redirects automatically)
    if (Get-Command curl.exe -ErrorAction SilentlyContinue) {
        & curl.exe -sSfL --connect-timeout 10 -o "$tempZip" "$url"
        if ((Test-Path $tempZip) -and (Get-Item $tempZip).Length -gt 1000000) {
            $downloaded = $true
            break
        }
    }
    
    # Fallback to Invoke-WebRequest
    try {
        Invoke-WebRequest -Uri $url -OutFile $tempZip -UseBasicParsing -TimeoutSec 60
        if ((Test-Path $tempZip) -and (Get-Item $tempZip).Length -gt 1000000) {
            $downloaded = $true
            break
        }
    } catch {}
}

if (-not $downloaded -or -not (Test-Path $tempZip)) {
    Write-Host "Error: Could not download the release package from any mirror." -ForegroundColor Red
    exit 1
}

# Unpack
Write-Host "Extracting binaries into $installDir..." -ForegroundColor Cyan
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

# Safe rename in case vitl-piano.exe or WebView2Loader.dll is running or locked by another process
foreach ($fName in @("vitl-piano.exe", "WebView2Loader.dll")) {
    $fPath = "$installDir\$fName"
    if (Test-Path $fPath) {
        try {
            Remove-Item -Force $fPath -ErrorAction Stop
        } catch {
            $oldFile = "$fPath.old"
            if (Test-Path $oldFile) { Remove-Item -Force $oldFile -ErrorAction SilentlyContinue }
            Rename-Item -Path $fPath -NewName "$fName.old" -Force -ErrorAction SilentlyContinue
        }
    }
}

$prevErr = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
if (Get-Command tar.exe -ErrorAction SilentlyContinue) {
    & tar.exe -xf "$tempZip" -C "$installDir" 2>$null
}
if (-not (Test-Path $exePath)) {
    Expand-Archive -Path $tempZip -DestinationPath $installDir -Force
}
$ErrorActionPreference = $prevErr
Remove-Item -Force $tempZip -ErrorAction SilentlyContinue

# 4. Shortcuts & Registry Registration
Write-Host "[4/4] Configuring shortcuts and system registration..." -ForegroundColor Cyan
$wsh = New-Object -ComObject WScript.Shell

# Desktop shortcut
$desktopPath = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::Desktop)
$shortcutDesktop = $wsh.CreateShortcut("$desktopPath\VITL Piano.lnk")
$shortcutDesktop.TargetPath = $exePath
$shortcutDesktop.WorkingDirectory = $installDir
$shortcutDesktop.IconLocation = "$exePath,0"
$shortcutDesktop.Description = "VITL Piano Autoplayer and Audio Synthesizer"
$shortcutDesktop.Save()

# Start Menu shortcut
$startMenuPath = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::Programs)
$shortcutStart = $wsh.CreateShortcut("$startMenuPath\VITL Piano.lnk")
$shortcutStart.TargetPath = $exePath
$shortcutStart.WorkingDirectory = $installDir
$shortcutStart.IconLocation = "$exePath,0"
$shortcutStart.Description = "VITL Piano Autoplayer and Audio Synthesizer"
$shortcutStart.Save()

# Uninstaller Registry entry
$regPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\VITLPiano"
New-Item -Path $regPath -Force | Out-Null
Set-ItemProperty -Path $regPath -Name "DisplayName" -Value "VITL Piano"
Set-ItemProperty -Path $regPath -Name "DisplayVersion" -Value "1.0.0-beta.2"
Set-ItemProperty -Path $regPath -Name "Publisher" -Value "VITL Piano Team"
Set-ItemProperty -Path $regPath -Name "DisplayIcon" -Value "$exePath,0"
Set-ItemProperty -Path $regPath -Name "InstallLocation" -Value "$installDir"
$uninstallCmd = "powershell.exe -WindowStyle Hidden -Command Remove-Item -Recurse -Force '$installDir'; Remove-ItemProperty -Path '$regPath' -ErrorAction SilentlyContinue"
Set-ItemProperty -Path $regPath -Name "UninstallString" -Value $uninstallCmd

Write-Host ""
Write-Host "Installation Complete! Launching VITL Piano..." -ForegroundColor Green
Start-Process -FilePath $exePath -WorkingDirectory $installDir
