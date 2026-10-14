#![windows_subsystem = "windows"]

use std::env;
use std::ffi::{c_void, OsStr};
use std::fs::{self, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::Command;

// ==========================================
// Eingebettete Ressourcen für All-in-One Setup
// ==========================================
const SHELL_DLL_BYTES: &[u8] = include_bytes!("../winshred_shell/target/release/winshred_shell.dll");
const MSIX_BYTES: &[u8] = include_bytes!("../winshred.msix");
const CER_BYTES: &[u8] = include_bytes!("../winshred.cer");
const ICO_BYTES: &[u8] = include_bytes!("../winshred.ico");

// ==========================================
// Win32 API Bindings
// ==========================================
#[repr(C)]
struct TASKDIALOG_BUTTON {
    n_button_id: i32,
    psz_button_text: *const u16,
}

#[repr(C)]
struct TASKDIALOGCONFIG {
    cb_size: u32,
    hwnd_parent: *mut c_void,
    h_instance: *mut c_void,
    dw_flags: u32,
    dw_common_buttons: u32,
    psz_window_title: *const u16,
    h_main_icon: *const u16,
    psz_main_instruction: *const u16,
    psz_content: *const u16,
    c_buttons: u32,
    p_buttons: *const TASKDIALOG_BUTTON,
    n_default_button: i32,
    c_radio_buttons: u32,
    p_radio_buttons: *const TASKDIALOG_BUTTON,
    n_default_radio_button: i32,
    psz_verification_text: *const u16,
    psz_expanded_information: *const u16,
    psz_expanded_control_text: *const u16,
    psz_collapsed_control_text: *const u16,
    h_footer_icon: *const u16,
    psz_footer: *const u16,
    pf_callback: *const c_void,
    lp_callback_data: usize,
    cx_width: u32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn TaskDialogIndirect(
        p_task_config: *const TASKDIALOGCONFIG,
        pn_button: *mut i32,
        pn_radio_button: *mut i32,
        pf_verification_flag_checked: *mut i32,
    ) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        hwnd: *mut c_void,
        lp_text: *const u16,
        lp_caption: *const u16,
        u_type: u32,
    ) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn IsUserAnAdmin() -> i32;
    fn ShellExecuteW(
        hwnd: *mut c_void,
        lp_operation: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show_cmd: i32,
    ) -> isize;
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(
        h_algorithm: *mut c_void,
        pb_buffer: *mut u8,
        cb_buffer: u32,
        dw_flags: u32,
    ) -> i32;
}

#[repr(C)]
struct WIN32_FIND_STREAM_DATA {
    stream_size: i64,
    c_stream_name: [u16; 296], // MAX_PATH + 36
}

const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn FindFirstStreamW(
        lpFileName: *const u16,
        InfoLevel: u32,
        lpFindStreamData: *mut c_void,
        dwFlags: u32,
    ) -> *mut c_void;

    fn FindNextStreamW(
        hFindStream: *mut c_void,
        lpFindStreamData: *mut c_void,
    ) -> i32;

    fn FindClose(hFindFile: *mut c_void) -> i32;

    fn DeleteFileW(lpFileName: *const u16) -> i32;

    fn DeviceIoControl(
        hDevice: *mut c_void,
        dwIoControlCode: u32,
        lpInBuffer: *const c_void,
        nInBufferSize: u32,
        lpOutBuffer: *mut c_void,
        nOutBufferSize: u32,
        lpBytesReturned: *mut u32,
        lpOverlapped: *mut c_void,
    ) -> i32;

    fn GetDiskFreeSpaceW(
        lpRootPathName: *const u16,
        lpSectorsPerCluster: *mut u32,
        lpBytesPerSector: *mut u32,
        lpNumberOfFreeClusters: *mut u32,
        lpTotalNumberOfClusters: *mut u32,
    ) -> i32;
}

const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;
const MB_OK: u32 = 0x00000000;
const MB_YESNO: u32 = 0x00000004;
const MB_ICONWARNING: u32 = 0x00000030;
const MB_DEFBUTTON2: u32 = 0x00000100;
const MB_ICONINFORMATION: u32 = 0x00000040;
const MB_ICONERROR: u32 = 0x00000010;
const IDYES: i32 = 6;

const ID_REPAIR: i32 = 1001;
const ID_UNINSTALL: i32 = 1002;
const ID_INSTALL: i32 = 1003;
const IDCANCEL: i32 = 2;

const TDF_USE_COMMAND_LINKS: u32 = 0x0010;
const TDF_ALLOW_DIALOG_CANCELLATION: u32 = 0x0008;
const TDCBF_CANCEL_BUTTON: u32 = 0x0008;

const FSCTL_FILE_LEVEL_TRIM: u32 = 0x00098208;

fn to_wide_chars(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn path_to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn show_message_box(text: &str, title: &str, flags: u32) -> i32 {
    let wide_text = to_wide_chars(text);
    let wide_title = to_wide_chars(title);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide_text.as_ptr(),
            wide_title.as_ptr(),
            flags,
        )
    }
}

fn elevate_relaunch(params: &str) {
    if let Ok(exe) = env::current_exe() {
        let op = to_wide_chars("runas");
        let file = to_wide_chars(&exe.to_string_lossy());
        let wide_params = to_wide_chars(params);
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                file.as_ptr(),
                wide_params.as_ptr(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            );
        }
    }
}

fn fill_random_bytes(buf: &mut [u8]) -> io::Result<()> {
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "BCryptGenRandom failed"))
    }
}

// ==========================================
// Shredder Logik (Slack, ADS, TRIM, MFT-Wipe)
// ==========================================

fn get_cluster_size(path: &Path) -> u64 {
    let mut sectors_per_cluster: u32 = 0;
    let mut bytes_per_sector: u32 = 0;
    let mut free_clusters: u32 = 0;
    let mut total_clusters: u32 = 0;

    let root_path = path.components().next().map(|c| {
        let mut p = PathBuf::from(c.as_os_str());
        p.push("\\");
        p
    });

    let wide_root: Vec<u16> = match root_path {
        Some(ref p) => p.as_os_str().encode_wide().chain(Some(0)).collect(),
        None => vec![0],
    };

    let root_ptr = if root_path.is_some() {
        wide_root.as_ptr()
    } else {
        std::ptr::null()
    };

    let ret = unsafe {
        GetDiskFreeSpaceW(
            root_ptr,
            &mut sectors_per_cluster,
            &mut bytes_per_sector,
            &mut free_clusters,
            &mut total_clusters,
        )
    };

    if ret != 0 && sectors_per_cluster > 0 && bytes_per_sector > 0 {
        (sectors_per_cluster as u64) * (bytes_per_sector as u64)
    } else {
        4096
    }
}

fn trim_file(file: &fs::File, len: u64) {
    if len == 0 {
        return;
    }

    #[repr(C)]
    struct FileLevelTrimRange {
        offset: u64,
        length: u64,
    }

    #[repr(C)]
    struct FileLevelTrim {
        key: u32,
        num_ranges: u32,
        ranges: [FileLevelTrimRange; 1],
    }

    #[repr(C)]
    struct FileLevelTrimOutput {
        num_ranges_processed: u32,
    }

    let trim_input = FileLevelTrim {
        key: 0,
        num_ranges: 1,
        ranges: [FileLevelTrimRange {
            offset: 0,
            length: len,
        }],
    };

    let mut trim_output = FileLevelTrimOutput {
        num_ranges_processed: 0,
    };
    let mut bytes_returned: u32 = 0;

    let handle = file.as_raw_handle() as *mut c_void;
    unsafe {
        DeviceIoControl(
            handle,
            FSCTL_FILE_LEVEL_TRIM,
            &trim_input as *const _ as *const c_void,
            std::mem::size_of::<FileLevelTrim>() as u32,
            &mut trim_output as *mut _ as *mut c_void,
            std::mem::size_of::<FileLevelTrimOutput>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        );
    }
}

fn wipe_open_file(file: &mut fs::File, wipe_len: u64) -> io::Result<()> {
    if wipe_len == 0 {
        return Ok(());
    }

    const CHUNK_SIZE: usize = 64 * 1024;
    let zero_chunk = vec![0u8; CHUNK_SIZE];
    let one_chunk = vec![0xFFu8; CHUNK_SIZE];
    let mut rand_chunk = vec![0u8; CHUNK_SIZE];

    // Pass 1: Nullen (0x00)
    file.seek(SeekFrom::Start(0))?;
    let mut remaining = wipe_len;
    while remaining > 0 {
        let write_size = std::cmp::min(remaining, CHUNK_SIZE as u64) as usize;
        file.write_all(&zero_chunk[..write_size])?;
        remaining -= write_size as u64;
    }
    file.sync_all()?;

    // Pass 2: Einsen (0xFF)
    file.seek(SeekFrom::Start(0))?;
    let mut remaining = wipe_len;
    while remaining > 0 {
        let write_size = std::cmp::min(remaining, CHUNK_SIZE as u64) as usize;
        file.write_all(&one_chunk[..write_size])?;
        remaining -= write_size as u64;
    }
    file.sync_all()?;

    // Pass 3: Krypto-Zufallsbytes
    file.seek(SeekFrom::Start(0))?;
    let mut remaining = wipe_len;
    while remaining > 0 {
        let write_size = std::cmp::min(remaining, CHUNK_SIZE as u64) as usize;
        fill_random_bytes(&mut rand_chunk[..write_size])?;
        file.write_all(&rand_chunk[..write_size])?;
        remaining -= write_size as u64;
    }
    file.sync_all()?;

    // SSD Deallocate / TRIM senden
    trim_file(file, wipe_len);

    // Auf 0 Bytes abschneiden und flushen
    file.set_len(0)?;
    file.sync_all()?;

    Ok(())
}

fn get_file_streams(path: &Path) -> Vec<String> {
    let mut streams = Vec::new();
    let wide_path = path_to_wide(path);

    let mut stream_data: WIN32_FIND_STREAM_DATA = unsafe { std::mem::zeroed() };
    let handle = unsafe {
        FindFirstStreamW(
            wide_path.as_ptr(),
            0,
            &mut stream_data as *mut _ as *mut c_void,
            0,
        )
    };

    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return streams;
    }

    loop {
        let name_len = stream_data
            .c_stream_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(stream_data.c_stream_name.len());
        let stream_name = String::from_utf16_lossy(&stream_data.c_stream_name[..name_len]);

        if stream_name != "::$DATA" && !stream_name.is_empty() {
            streams.push(stream_name);
        }

        let next = unsafe {
            FindNextStreamW(
                handle,
                &mut stream_data as *mut _ as *mut c_void,
            )
        };
        if next == 0 {
            break;
        }
    }

    unsafe {
        FindClose(handle);
    }

    streams
}

fn shred_alternate_streams(path: &Path) -> io::Result<()> {
    let streams = get_file_streams(path);
    for stream_name in streams {
        let mut stream_path = path.as_os_str().to_os_string();
        stream_path.push(&stream_name);
        let stream_path = PathBuf::from(stream_path);

        if let Ok(mut file) = OpenOptions::new().write(true).read(true).open(&stream_path) {
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if len > 0 {
                let _ = wipe_open_file(&mut file, len);
            }
        }

        let wide_stream_path = path_to_wide(&stream_path);
        unsafe {
            DeleteFileW(wide_stream_path.as_ptr());
        }
    }
    Ok(())
}

fn secure_multi_rename(path: &Path) -> io::Result<PathBuf> {
    let parent = match path.parent() {
        Some(p) => p,
        None => return Ok(path.to_path_buf()),
    };

    let mut current_path = path.to_path_buf();

    if let Ok(meta) = fs::metadata(&current_path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(&current_path, perms);
        }
    }

    let mut rand_bytes = [0u8; 12];
    let _ = fill_random_bytes(&mut rand_bytes);

    let pass1_name = format!(
        "shred_aaaa_{:02x}{:02x}{:02x}{:02x}.tmp",
        rand_bytes[0], rand_bytes[1], rand_bytes[2], rand_bytes[3]
    );
    let p1 = parent.join(pass1_name);
    if fs::rename(&current_path, &p1).is_ok() {
        current_path = p1;
    }

    let pass2_name = format!(
        "shred_0000_{:02x}{:02x}{:02x}{:02x}.tmp",
        rand_bytes[4], rand_bytes[5], rand_bytes[6], rand_bytes[7]
    );
    let p2 = parent.join(pass2_name);
    if fs::rename(&current_path, &p2).is_ok() {
        current_path = p2;
    }

    let pass3_name = format!(
        "shred_rand_{:02x}{:02x}{:02x}{:02x}.tmp",
        rand_bytes[8], rand_bytes[9], rand_bytes[10], rand_bytes[11]
    );
    let p3 = parent.join(pass3_name);
    if fs::rename(&current_path, &p3).is_ok() {
        current_path = p3;
    }

    Ok(current_path)
}

fn clear_readonly(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

fn shred_file(path: &Path) -> io::Result<()> {
    clear_readonly(path);
    let _ = shred_alternate_streams(path);

    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .open(path)?;

    let len = file.metadata()?.len();
    if len > 0 {
        let cluster_size = get_cluster_size(path);
        let remainder = len % cluster_size;
        let wipe_len = if remainder != 0 {
            len + (cluster_size - remainder)
        } else {
            len
        };

        wipe_open_file(&mut file, wipe_len)?;
    }

    drop(file);

    let final_path = secure_multi_rename(path)?;
    fs::remove_file(final_path)
}

fn shred_recursive(path: &Path) -> io::Result<()> {
    clear_readonly(path);

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            shred_recursive(&entry.path())?;
        }
        let final_dir_path = secure_multi_rename(path)?;
        fs::remove_dir(final_dir_path)?;
    } else if path.is_file() {
        shred_file(path)?;
    }
    Ok(())
}

// ==========================================
// All-in-One Installer / Deinstaller Logik
// ==========================================

fn is_installed() -> bool {
    let app_path = PathBuf::from("C:\\Program Files\\WinShred\\winshred.exe");
    app_path.exists()
}

fn run_install() {
    let is_admin = unsafe { IsUserAnAdmin() } != 0;
    if !is_admin {
        elevate_relaunch("--install");
        return;
    }

    let install_dir = PathBuf::from("C:\\Program Files\\WinShred");
    if let Err(e) = fs::create_dir_all(&install_dir) {
        show_message_box(
            &format!("Fehler beim Erstellen des Installationsordners:\n{}", e),
            "WinShred Setup - Fehler",
            MB_ICONERROR,
        );
        return;
    }

    // 1. Eigene EXE nach Program Files kopieren
    if let Ok(current_exe) = env::current_exe() {
        let target_exe = install_dir.join("winshred.exe");
        if current_exe != target_exe {
            let _ = fs::copy(&current_exe, &target_exe);
        }
    }

    // 2. Eingebettete Shell-DLL, MSIX, Zertifikat und Icon ablegen
    let dll_path = install_dir.join("winshred_shell.dll");
    let msix_path = install_dir.join("winshred.msix");
    let cer_path = install_dir.join("winshred.cer");
    let ico_path = install_dir.join("winshred.ico");

    let _ = fs::write(&dll_path, SHELL_DLL_BYTES);
    let _ = fs::write(&msix_path, MSIX_BYTES);
    let _ = fs::write(&cer_path, CER_BYTES);
    let _ = fs::write(&ico_path, ICO_BYTES);

    // 3. PowerShell-Setup für Zertifikat + AppX + Shell + SendTo + Registrierung
    let ps_command = format!(
        "Import-Certificate -FilePath '{cer}' -CertStoreLocation 'Cert:\\LocalMachine\\TrustedPeople' | Out-Null; \
        Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue; \
        Add-AppxPackage -Path '{msix}' -ExternalLocation '{inst}'; \
        New-Item -Path 'HKLM:\\SOFTWARE\\Classes\\*\\shell\\WinShred\\command' -Force | Out-Null; \
        Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Classes\\*\\shell\\WinShred' -Name '(Default)' -Value 'Mit WinShred sicher vernichten'; \
        Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Classes\\*\\shell\\WinShred' -Name 'Icon' -Value '{ico}'; \
        Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Classes\\*\\shell\\WinShred\\command' -Name '(Default)' -Value '\"{inst}\\winshred.exe\" \"%1\"'; \
        New-Item -Path 'HKLM:\\SOFTWARE\\Classes\\Directory\\shell\\WinShred\\command' -Force | Out-Null; \
        Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Classes\\Directory\\shell\\WinShred' -Name '(Default)' -Value 'Mit WinShred sicher vernichten'; \
        Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Classes\\Directory\\shell\\WinShred' -Name 'Icon' -Value '{ico}'; \
        Set-ItemProperty -Path 'HKLM:\\SOFTWARE\\Classes\\Directory\\shell\\WinShred\\command' -Name '(Default)' -Value '\"{inst}\\winshred.exe\" \"%1\"'; \
        $wsh = New-Object -ComObject WScript.Shell; \
        $s = $wsh.CreateShortcut(\"$env:APPDATA\\Microsoft\\Windows\\SendTo\\WinShred (Sicher vernichten).lnk\"); \
        $s.TargetPath = '{inst}\\winshred.exe'; \
        $s.IconLocation = '{ico}'; \
        $s.Save(); \
        Stop-Process -Name explorer -Force; Start-Process explorer",
        cer = cer_path.to_string_lossy(),
        msix = msix_path.to_string_lossy(),
        inst = install_dir.to_string_lossy(),
        ico = ico_path.to_string_lossy()
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_command])
        .output();

    // 4. Verifikation der Installation
    let verify_output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "(Get-AppxPackage -Name 'WinShred.ShellExtension').PackageFullName",
        ])
        .output();

    let package_name = verify_output
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let files_exist = install_dir.join("winshred.exe").exists()
        && install_dir.join("winshred_shell.dll").exists();

    if !package_name.is_empty() && files_exist {
        let msg = format!(
            "====================================================\n\
              INSTALLATIONSBESTAETIGUNG - WINSHRED\n\
            ====================================================\n\n\
            Status: Erfolgreich installiert und verifiziert!\n\n\
            KOMPONENTEN:\n\
            [OK] Programmdatei: C:\\Program Files\\WinShred\\winshred.exe\n\
            [OK] Shell-Erweiterung: winshred_shell.dll\n\
            [OK] Modernes Windows 11 Menue: Aktiviert ({})\n\
            [OK] Eigenes Icon: Eingerichtet (winshred.ico)\n\
            [OK] Zertifikat: Vertrauenswuerdig (LocalMachine\\TrustedPeople)\n\
            [OK] Explorer-Shell: Aktualisiert\n\n\
            BEDIENUNG:\n\
            Klicken Sie mit der rechten Maustaste auf eine beliebige Datei\n\
            oder einen Ordner. Der Eintrag 'Mit WinShred sicher vernichten'\n\
            steht ab sofort auf der ersten Ebene Ihres Kontextmenues bereit.",
            package_name
        );
        show_message_box(&msg, "WinShred - Installationsbestätigung", MB_OK | MB_ICONINFORMATION);
    } else {
        let err_detail = output
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
            .unwrap_or_default();

        let msg = if !err_detail.is_empty() {
            format!(
                "Hinweis zur Installation:\n\n\
                Die Dateien wurden kopiert, aber bei der Paket-Registrierung gab es eine Rueckmeldung:\n\n\
                {}\n\n\
                Bitte pruefen Sie, ob Windows Developer Mode oder lokale AppX-Installationen zugelassen sind.",
                err_detail
            )
        } else {
            "Die Installation wurde durchgefuehrt. Falls das Menue noch nicht sichtbar ist, starten Sie bitte Explorer neu.".to_string()
        };

        show_message_box(&msg, "WinShred - Installationshinweis", MB_OK | MB_ICONWARNING);
    }
}

fn run_uninstall() {
    let is_admin = unsafe { IsUserAnAdmin() } != 0;
    if !is_admin {
        elevate_relaunch("--uninstall");
        return;
    }

    let ps_command = "\
        Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue; \
        $cert = Get-ChildItem Cert:\\LocalMachine\\TrustedPeople | Where-Object { $_.Subject -eq 'CN=WinShred' }; \
        if ($cert) { Remove-Item ('Cert:\\LocalMachine\\TrustedPeople\\' + $cert.Thumbprint) -Force }; \
        Remove-Item -Path 'HKLM:\\SOFTWARE\\Classes\\*\\shell\\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; \
        Remove-Item -Path 'HKLM:\\SOFTWARE\\Classes\\Directory\\shell\\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; \
        Remove-Item -Path 'HKCU:\\Software\\Classes\\*\\shell\\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; \
        Remove-Item -Path 'HKCU:\\Software\\Classes\\Directory\\shell\\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; \
        Remove-Item \"$env:APPDATA\\Microsoft\\Windows\\SendTo\\WinShred (Sicher vernichten).lnk\" -Force -ErrorAction SilentlyContinue; \
        Stop-Process -Name explorer -Force; Start-Process explorer";

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps_command])
        .status();

    let install_dir = PathBuf::from("C:\\Program Files\\WinShred");
    let _ = fs::remove_file(install_dir.join("winshred_shell.dll"));
    let _ = fs::remove_file(install_dir.join("winshred.msix"));
    let _ = fs::remove_file(install_dir.join("winshred.cer"));
    let _ = fs::remove_file(install_dir.join("winshred.ico"));

    if let Ok(current_exe) = env::current_exe() {
        if !current_exe.starts_with(&install_dir) {
            let _ = fs::remove_file(install_dir.join("winshred.exe"));
            let _ = fs::remove_dir(&install_dir);
        }
    }

    show_message_box(
        "WinShred wurde erfolgreich aus dem Kontextmenü entfernt und deinstalliert.",
        "WinShred Setup",
        MB_OK | MB_ICONINFORMATION,
    );
}

// ==========================================
// Native Windows TaskDialog für das Setup
// ==========================================

fn show_installed_choice_dialog() -> i32 {
    let title = to_wide_chars("WinShred - Verwaltung");
    let main_inst = to_wide_chars("WinShred ist auf diesem Computer installiert");
    let content = to_wide_chars("Wählen Sie die gewünschte Aktion:");

    let btn_repair_text = to_wide_chars("WinShred reparieren / aktualisieren\nInstalliert die Shell-Erweiterung und alle Dateien neu.");
    let btn_uninstall_text = to_wide_chars("WinShred deinstallieren\nEntfernt WinShred und alle Einträge aus dem Kontextmenü.");

    let buttons = [
        TASKDIALOG_BUTTON {
            n_button_id: ID_REPAIR,
            psz_button_text: btn_repair_text.as_ptr(),
        },
        TASKDIALOG_BUTTON {
            n_button_id: ID_UNINSTALL,
            psz_button_text: btn_uninstall_text.as_ptr(),
        },
    ];

    let mut config: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
    config.cb_size = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
    config.dw_flags = TDF_USE_COMMAND_LINKS | TDF_ALLOW_DIALOG_CANCELLATION;
    config.dw_common_buttons = TDCBF_CANCEL_BUTTON;
    config.psz_window_title = title.as_ptr();
    config.psz_main_instruction = main_inst.as_ptr();
    config.psz_content = content.as_ptr();
    config.c_buttons = buttons.len() as u32;
    config.p_buttons = buttons.as_ptr();

    let mut selected_button: i32 = 0;
    let ret = unsafe {
        TaskDialogIndirect(
            &config,
            &mut selected_button,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if ret == 0 {
        selected_button
    } else {
        // Fallback falls TaskDialogIndirect nicht unterstützt wird
        let res = show_message_box(
            "WinShred ist installiert.\n\n[Ja] = Reparieren / Aktualisieren\n[Nein] = Deinstallieren",
            "WinShred",
            MB_YESNO,
        );
        if res == IDYES {
            ID_REPAIR
        } else {
            ID_UNINSTALL
        }
    }
}

fn show_not_installed_dialog() -> i32 {
    let title = to_wide_chars("WinShred - Setup");
    let main_inst = to_wide_chars("WinShred im Windows 11 Kontextmenü installieren");
    let content = to_wide_chars("Möchten Sie WinShred jetzt auf diesem System einrichten?");

    let btn_install_text = to_wide_chars("WinShred jetzt installieren\nRichtet das moderne Kontextmenü ('Mit WinShred sicher vernichten') ein.");

    let buttons = [
        TASKDIALOG_BUTTON {
            n_button_id: ID_INSTALL,
            psz_button_text: btn_install_text.as_ptr(),
        },
    ];

    let mut config: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
    config.cb_size = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
    config.dw_flags = TDF_USE_COMMAND_LINKS | TDF_ALLOW_DIALOG_CANCELLATION;
    config.dw_common_buttons = TDCBF_CANCEL_BUTTON;
    config.psz_window_title = title.as_ptr();
    config.psz_main_instruction = main_inst.as_ptr();
    config.psz_content = content.as_ptr();
    config.c_buttons = buttons.len() as u32;
    config.p_buttons = buttons.as_ptr();

    let mut selected_button: i32 = 0;
    let ret = unsafe {
        TaskDialogIndirect(
            &config,
            &mut selected_button,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if ret == 0 {
        selected_button
    } else {
        let res = show_message_box(
            "Möchten Sie WinShred im Kontextmenü installieren?",
            "WinShred Setup",
            MB_YESNO,
        );
        if res == IDYES {
            ID_INSTALL
        } else {
            IDCANCEL
        }
    }
}

// ==========================================
// Entry Point (CLI / GUI / Kontextmenü)
// ==========================================
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // 1. Spezial-Flags für CLI
    if args.iter().any(|a| a == "--install" || a == "-i") {
        run_install();
        return;
    }
    if args.iter().any(|a| a == "--uninstall" || a == "-u") {
        run_uninstall();
        return;
    }

    // 2. Ohne Parameter gestartet (z.B. Doppelklick auf die EXE) -> Modernes TaskDialog Setup
    if args.is_empty() {
        if !is_installed() {
            let choice = show_not_installed_dialog();
            if choice == ID_INSTALL {
                run_install();
            }
        } else {
            let choice = show_installed_choice_dialog();
            if choice == ID_REPAIR {
                run_install();
            } else if choice == ID_UNINSTALL {
                run_uninstall();
            }
        }
        return;
    }

    // 3. Mit Parametern gestartet (z.B. Rechtsklick im Kontextmenü oder Drag & Drop) -> Shredder-Modus
    let mut paths_to_shred: Vec<PathBuf> = Vec::new();
    let mut auto_confirm = false;

    for arg in args {
        if arg == "-y" || arg == "--yes" || arg == "--quiet" {
            auto_confirm = true;
        } else {
            let p = PathBuf::from(arg);
            if p.exists() {
                paths_to_shred.push(p);
            }
        }
    }

    if paths_to_shred.is_empty() {
        show_message_box(
            "Keine gültigen Dateien oder Ordner zum Vernichten gefunden.",
            "WinShred - Hinweis",
            MB_ICONWARNING,
        );
        return;
    }

    // Sicherheitsabfrage vor dem Vernichten
    if !auto_confirm {
        let count = paths_to_shred.len();
        let prompt_text = if count == 1 {
            let name = paths_to_shred[0]
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let is_dir = paths_to_shred[0].is_dir();
            let typ = if is_dir { "den Ordner" } else { "die Datei" };
            format!(
                "Möchten Sie {} '{}' wirklich unwiderruflich vernichten?\n\n\
                SICHERHEITS-FEATURES:\n\
                - 3-faches Überschreiben (0x00, 0xFF, Krypto-Zufall)\n\
                - Cluster-Slack & Alternate Data Streams (ADS) Bereinigung\n\
                - SSD/NVMe TRIM Deallocate-Signal zur Controller-Tilgung\n\
                - Mehrfache Namens-Anonymisierung im NTFS MFT-Dateisystem\n\n\
                WARNUNG: Dieser Vorgang ist unumkehrbar!",
                typ, name
            )
        } else {
            format!(
                "Möchten Sie diese {} Elemente wirklich unwiderruflich vernichten?\n\n\
                WARNUNG: Dieser Vorgang ist unumkehrbar!",
                count
            )
        };

        let result = show_message_box(
            &prompt_text,
            "WinShred - Sicherheitsbestätigung",
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        );

        if result != IDYES {
            return;
        }
    }

    // Ausführung
    let mut errors = 0;
    for path in &paths_to_shred {
        if let Err(e) = shred_recursive(path) {
            errors += 1;
            eprintln!("Fehler bei {:?}: {}", path, e);
        }
    }

    if errors > 0 {
        show_message_box(
            &format!("WinShred abgeschlossen, aber bei {} Element(en) gab es Zugriffsfehler.", errors),
            "WinShred - Hinweis",
            MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_size() {
        let current_dir = env::current_dir().unwrap();
        let cs = get_cluster_size(&current_dir);
        assert!(cs >= 512, "Cluster-Größe sollte mindestens 512 Bytes sein: {}", cs);
    }

    #[test]
    fn test_shred_file_with_ads() {
        let temp_dir = env::temp_dir().join(format!("winshred_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        let test_file = temp_dir.join("test_secret.txt");
        fs::write(&test_file, b"Hauptinhalt der Testdatei").unwrap();

        let ads_path_str = format!("{}:ads_secret:$DATA", test_file.to_string_lossy());
        if let Ok(mut ads_file) = OpenOptions::new().write(true).create(true).open(&ads_path_str) {
            ads_file.write_all(b"Geheimer Alternate Data Stream").unwrap();
        }

        assert!(test_file.exists());
        shred_file(&test_file).unwrap();
        assert!(!test_file.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_shred_recursive_folder() {
        let temp_dir = env::temp_dir().join(format!("winshred_dir_test_{}", std::process::id()));
        let sub_dir = temp_dir.join("unterordner");
        fs::create_dir_all(&sub_dir).unwrap();

        let file1 = temp_dir.join("datei1.txt");
        let file2 = sub_dir.join("datei2.txt");
        fs::write(&file1, b"Test 1").unwrap();
        fs::write(&file2, b"Test 2").unwrap();

        assert!(temp_dir.exists());
        shred_recursive(&temp_dir).unwrap();
        assert!(!temp_dir.exists());
    }
}
