use std::fs;
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::os::windows::process::CommandExt;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

const CREATE_NO_WINDOW: u32 = 0x08000000;
static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin.gz"));

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallInfo {
    pub default_path: String,
    pub is_installed: bool,
    pub existing_version: Option<String>,
    pub app_version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OnlineUpdateInfo {
    pub available: bool,
    pub tag: String,
    pub name: String,
    pub download_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallOptions {
    pub target_dir: String,
    pub create_desktop_shortcut: bool,
    pub create_start_menu_shortcut: bool,
    pub launch_after: bool,
    pub use_online_latest: bool,
    pub online_download_url: Option<String>,
}

pub fn get_default_install_dir() -> PathBuf {
    if let Some(mut p) = dirs::data_local_dir() {
        p.push("Programs");
        p.push("VITL Piano");
        p
    } else {
        PathBuf::from(r"C:\Program Files\VITL Piano")
    }
}

fn to_base64(bytes: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(CHARSET[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(CHARSET[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn run_powershell(script: &str) -> std::io::Result<std::process::Output> {
    let utf16: Vec<u16> = script.encode_utf16().collect();
    let mut bytes = Vec::with_capacity(utf16.len() * 2);
    for u in utf16 {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let encoded = to_base64(&bytes);
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

pub fn get_install_info() -> InstallInfo {
    let default_dir = get_default_install_dir();
    let exe_path = default_dir.join("vitl-piano.exe");
    let is_installed = exe_path.exists();

    InstallInfo {
        default_path: default_dir.to_string_lossy().to_string(),
        is_installed,
        existing_version: if is_installed { Some("1.0.0".to_string()) } else { None },
        app_version: "1.0.0-beta.2".to_string(),
    }
}

pub fn check_latest_update() -> OnlineUpdateInfo {
    let script = r#"
        $ProgressPreference = 'SilentlyContinue'
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12 -bor [System.Net.SecurityProtocolType]::Tls13
        $headers = @{ 'User-Agent' = 'VITL-Piano-Setup' }
        try {
            $rel = Invoke-RestMethod -Uri 'https://api.github.com/repos/RandomGuy-VN/VITL-PIANO-DESKTOP/releases' -Headers $headers -TimeoutSec 10
            if ($rel -and $rel.Count -gt 0) {
                $latest = $rel[0]
                $asset = $latest.assets | Where-Object { $_.name -like '*portable*.zip' -or $_.name -like '*windows*.zip' } | Select-Object -First 1
                $downUrl = if ($asset) { $asset.browser_download_url } else { "https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest/download/vitl-piano-windows-portable.zip" }
                $out = @{
                    available = [bool]$true
                    tag = [string]$latest.tag_name
                    name = [string]$latest.name
                    download_url = [string]$downUrl
                }
                $out | ConvertTo-Json -Compress
            } else {
                '{"available":true,"tag":"v1.0-beta.2","name":"VITL Piano Desktop V1.0 Public Beta 2","download_url":"https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest/download/vitl-piano-windows-portable.zip"}'
            }
        } catch {
            '{"available":true,"tag":"v1.0-beta.2","name":"VITL Piano Desktop V1.0 Public Beta 2","download_url":"https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest/download/vitl-piano-windows-portable.zip"}'
        }
    "#;

    if let Ok(out) = run_powershell(script) {
        let stdout_str = String::from_utf8_lossy(&out.stdout);
        if let Some(start) = stdout_str.find('{') {
            if let Some(end) = stdout_str.rfind('}') {
                if end > start {
                    let json_slice = &stdout_str[start..=end];
                    if let Ok(info) = serde_json::from_str::<OnlineUpdateInfo>(json_slice) {
                        return info;
                    }
                }
            }
        }
    }

    OnlineUpdateInfo {
        available: true,
        tag: "v1.0-beta.2".to_string(),
        name: "VITL Piano Desktop V1.0 Public Beta 2".to_string(),
        download_url: Some("https://github.com/RandomGuy-VN/VITL-PIANO-DESKTOP/releases/latest/download/vitl-piano-windows-portable.zip".to_string()),
    }
}

pub fn download_and_extract_online_update<F>(download_url: &str, target_dir: &Path, mut progress_cb: F) -> Result<(), String>
where
    F: FnMut(f32, &str),
{
    fs::create_dir_all(target_dir).map_err(|e| format!("Failed to create install directory: {}", e))?;
    
    let temp_zip = target_dir.join("_update_temp.zip");
    progress_cb(0.15, "Connecting to GitHub Releases mirror...");

    let script = format!(
        r#"
        $ProgressPreference = 'SilentlyContinue'
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12 -bor [System.Net.SecurityProtocolType]::Tls13
        $url = '{url}'
        $tempZip = '{temp_zip}'
        $targetDir = '{target_dir}'

        if (Test-Path $tempZip) {{ Remove-Item -Force $tempZip -ErrorAction SilentlyContinue }}

        # 1. Download with curl or WebClient
        $downloaded = $false
        if (Get-Command curl.exe -ErrorAction SilentlyContinue) {{
            & curl.exe -sSfL --connect-timeout 15 -o "$tempZip" "$url"
            if ((Test-Path $tempZip) -and (Get-Item $tempZip).Length -gt 1000000) {{
                $downloaded = $true
            }}
        }}
        if (-not $downloaded) {{
            try {{
                $wc = New-Object System.Net.WebClient
                $wc.Headers.Add('User-Agent', 'VITL-Piano-Setup')
                $wc.DownloadFile($url, $tempZip)
                if ((Test-Path $tempZip) -and (Get-Item $tempZip).Length -gt 1000000) {{
                    $downloaded = $true
                }}
            }} catch {{}}
        }}

        if (-not $downloaded -or -not (Test-Path $tempZip)) {{
            Write-Error "Failed to download update package from $url"
            exit 1
        }}

        # 2. Release lock on existing vitl-piano.exe by renaming if needed
        $exePath = "$targetDir\vitl-piano.exe"
        if (Test-Path $exePath) {{
            try {{
                Remove-Item -Force $exePath -ErrorAction Stop
            }} catch {{
                $oldFile = "$targetDir\vitl-piano.exe.old"
                if (Test-Path $oldFile) {{ Remove-Item -Force $oldFile -ErrorAction SilentlyContinue }}
                Rename-Item -Path $exePath -NewName "vitl-piano.exe.old" -Force -ErrorAction SilentlyContinue
            }}
        }}

        # 3. Extract using tar or Expand-Archive
        $extracted = $false
        if (Get-Command tar.exe -ErrorAction SilentlyContinue) {{
            & tar.exe -xf "$tempZip" -C "$targetDir" 2>$null
            if (Test-Path $exePath) {{ $extracted = $true }}
        }}
        if (-not $extracted) {{
            Expand-Archive -Path $tempZip -DestinationPath $targetDir -Force
        }}

        Remove-Item -Force $tempZip -ErrorAction SilentlyContinue
        "#,
        url = download_url.replace('\'', "''"),
        temp_zip = temp_zip.to_string_lossy().replace('\'', "''"),
        target_dir = target_dir.to_string_lossy().replace('\'', "''")
    );

    progress_cb(0.35, "Claiming and downloading latest release from GitHub...");

    let output = run_powershell(&script)
        .map_err(|e| format!("Failed to launch download process: {}", e))?;

    if !output.status.success() {
        return Err(format!("Update download error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    progress_cb(0.80, "Latest release package successfully unpacked!");
    Ok(())
}

pub fn close_running_instances() {
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "vitl-piano.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

pub fn extract_payload<F>(target_dir: &Path, mut progress_cb: F) -> Result<(), String>
where
    F: FnMut(f32, &str),
{
    fs::create_dir_all(target_dir).map_err(|e| format!("Failed to create install directory: {}", e))?;

    progress_cb(0.1, "Decompressing application bundle...");

    let cursor = Cursor::new(PAYLOAD);
    let mut decoder = GzDecoder::new(BufReader::new(cursor));

    let mut count_buf = [0u8; 4];
    decoder.read_exact(&mut count_buf).map_err(|e| format!("Failed to read payload header: {}", e))?;
    let file_count = u32::from_le_bytes(count_buf);

    for i in 0..file_count {
        let mut path_len_buf = [0u8; 4];
        decoder.read_exact(&mut path_len_buf).map_err(|e| format!("Payload read error: {}", e))?;
        let path_len = u32::from_le_bytes(path_len_buf) as usize;

        let mut path_bytes = vec![0u8; path_len];
        decoder.read_exact(&mut path_bytes).map_err(|e| format!("Payload path read error: {}", e))?;
        let rel_path_str = String::from_utf8(path_bytes).map_err(|e| format!("Invalid path in payload: {}", e))?;

        let mut data_len_buf = [0u8; 8];
        decoder.read_exact(&mut data_len_buf).map_err(|e| format!("Payload data length error: {}", e))?;
        let data_len = u64::from_le_bytes(data_len_buf) as usize;

        let mut data = vec![0u8; data_len];
        decoder.read_exact(&mut data).map_err(|e| format!("Payload data read error: {}", e))?;

        let dest_file_path = target_dir.join(&rel_path_str);
        if let Some(parent) = dest_file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Retry loop in case Windows is still releasing file handle from previous process
        let mut write_res = fs::write(&dest_file_path, &data);
        if write_res.is_err() {
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                write_res = fs::write(&dest_file_path, &data);
                if write_res.is_ok() {
                    break;
                }
            }
        }

        write_res.map_err(|e| format!("Failed to write {}: {}", rel_path_str, e))?;

        let fraction = 0.15 + 0.65 * ((i + 1) as f32 / file_count as f32);
        progress_cb(fraction, &format!("Extracting {}...", rel_path_str));
    }

    Ok(())
}

pub fn create_shortcut(target_exe: &Path, shortcut_path: &Path, target_dir: &Path) {
    if let Some(parent) = shortcut_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let script = format!(
        "$WshShell = New-Object -ComObject WScript.Shell; \
         $Shortcut = $WshShell.CreateShortcut('{}'); \
         $Shortcut.TargetPath = '{}'; \
         $Shortcut.WorkingDirectory = '{}'; \
         $Shortcut.IconLocation = '{},0'; \
         $Shortcut.Description = 'VITL Piano Autoplayer & Audio Synthesizer'; \
         $Shortcut.Save()",
        shortcut_path.to_string_lossy().replace('\'', "''"),
        target_exe.to_string_lossy().replace('\'', "''"),
        target_dir.to_string_lossy().replace('\'', "''"),
        target_exe.to_string_lossy().replace('\'', "''"),
    );

    let _ = run_powershell(&script);
}

pub fn register_uninstaller(target_dir: &Path, target_exe: &Path) {
    let script = format!(
        "$regPath = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\VITLPiano'; \
         New-Item -Path $regPath -Force | Out-Null; \
         Set-ItemProperty -Path $regPath -Name 'DisplayName' -Value 'VITL Piano'; \
         Set-ItemProperty -Path $regPath -Name 'DisplayVersion' -Value '1.0.0-beta.2'; \
         Set-ItemProperty -Path $regPath -Name 'Publisher' -Value 'VITL Piano Team'; \
         Set-ItemProperty -Path $regPath -Name 'DisplayIcon' -Value '{},0'; \
         Set-ItemProperty -Path $regPath -Name 'InstallLocation' -Value '{}'; \
         Set-ItemProperty -Path $regPath -Name 'UninstallString' -Value 'powershell.exe -WindowStyle Hidden -Command \"Remove-Item -Recurse -Force ''{}''; Remove-ItemProperty -Path ''HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\VITLPiano'' -ErrorAction SilentlyContinue\"';",
        target_exe.to_string_lossy().replace('\'', "''"),
        target_dir.to_string_lossy().replace('\'', "''"),
        target_dir.to_string_lossy().replace('\'', "''"),
    );

    let _ = run_powershell(&script);
}

pub fn launch_application(target_exe: &Path) {
    if let Some(target_dir) = target_exe.parent() {
        let _ = Command::new(target_exe)
            .current_dir(target_dir)
            .spawn();
    } else {
        let _ = Command::new(target_exe).spawn();
    }
}
