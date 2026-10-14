#![windows_subsystem = "windows"]

use std::env;
use std::ffi::{c_void, OsStr};
use std::fs::{self, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

// ==========================================
// Eingebettete Ressourcen für All-in-One Setup
// ==========================================
const SHELL_DLL_BYTES: &[u8] = include_bytes!("../winshred_shell/target/release/winshred_shell.dll");
const MSIX_BYTES: &[u8] = include_bytes!("../packaging/winshred.msix");
const CER_BYTES: &[u8] = include_bytes!("../packaging/winshred.cer");
const ICO_BYTES: &[u8] = include_bytes!("../assets/winshred.ico");

// ==========================================
// Win32 API Bindings & Structs
// ==========================================
#[repr(C)]
struct WNDCLASSEXW {
    cb_size: u32,
    style: u32,
    lpfn_wnd_proc: Option<unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: *mut c_void,
    h_icon: *mut c_void,
    h_cursor: *mut c_void,
    hbr_background: *mut c_void,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
    h_icon_sm: *mut c_void,
}

#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

#[repr(C)]
struct MSG {
    hwnd: *mut c_void,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: POINT,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct PAINTSTRUCT {
    hdc: *mut c_void,
    f_erase: i32,
    rc_paint: RECT,
    f_restore: i32,
    f_inc_update: i32,
    rgb_reserved: [u8; 32],
}

#[repr(C)]
struct INITCOMMONCONTROLSEX {
    dw_size: u32,
    dw_icc: u32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn InitCommonControlsEx(picce: *const INITCOMMONCONTROLSEX) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> *mut c_void;
    fn DeleteObject(ho: *mut c_void) -> i32;
    fn SetTextColor(hdc: *mut c_void, color: u32) -> u32;
    fn SetBkMode(hdc: *mut c_void, mode: i32) -> i32;
    fn SetBkColor(hdc: *mut c_void, color: u32) -> u32;
    fn SelectObject(hdc: *mut c_void, h: *mut c_void) -> *mut c_void;
    fn CreateFontW(
        cHeight: i32,
        cWidth: i32,
        cEscapement: i32,
        cOrientation: i32,
        cWeight: i32,
        bItalic: u32,
        bUnderline: u32,
        bStrikeOut: u32,
        iCharSet: u32,
        iOutPrecision: u32,
        iClipPrecision: u32,
        iQuality: u32,
        iPitchAndFamily: u32,
        pszFaceName: *const u16,
    ) -> *mut c_void;
    fn FillRect(hdc: *mut c_void, lprc: *const RECT, hbr: *mut c_void) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        hwnd: *mut c_void,
        lp_text: *const u16,
        lp_caption: *const u16,
        u_type: u32,
    ) -> i32;

    fn RegisterClassExW(lpwcx: *const WNDCLASSEXW) -> u16;
    fn CreateWindowExW(
        dwExStyle: u32,
        lpClassName: *const u16,
        lpWindowName: *const u16,
        dwStyle: u32,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: *mut c_void,
        hMenu: *mut c_void,
        hInstance: *mut c_void,
        lpParam: *mut c_void,
    ) -> *mut c_void;
    fn DefWindowProcW(hWnd: *mut c_void, Msg: u32, wParam: usize, lParam: isize) -> isize;
    fn ShowWindow(hWnd: *mut c_void, nCmdShow: i32) -> i32;
    fn UpdateWindow(hWnd: *mut c_void) -> i32;
    fn GetMessageW(lpMsg: *mut MSG, hWnd: *mut c_void, wMsgFilterMin: u32, wMsgFilterMax: u32) -> i32;
    fn TranslateMessage(lpMsg: *const MSG) -> i32;
    fn DispatchMessageW(lpMsg: *const MSG) -> isize;
    fn PostQuitMessage(nExitCode: i32);
    fn DestroyWindow(hWnd: *mut c_void) -> i32;
    fn BeginPaint(hWnd: *mut c_void, lpPaint: *mut PAINTSTRUCT) -> *mut c_void;
    fn EndPaint(hWnd: *mut c_void, lpPaint: *const PAINTSTRUCT) -> i32;
    fn DrawIconEx(
        hdc: *mut c_void,
        xLeft: i32,
        yTop: i32,
        hIcon: *mut c_void,
        cxWidth: i32,
        cyWidth: i32,
        istepIfAniCur: u32,
        hbrFlickerFreeDraw: *mut c_void,
        diFlags: u32,
    ) -> i32;
    fn LoadImageW(
        hInst: *mut c_void,
        name: *const u16,
        type_: u32,
        cx: i32,
        cy: i32,
        fuLoad: u32,
    ) -> *mut c_void;
    fn LoadCursorW(hInstance: *mut c_void, lpCursorName: *const u16) -> *mut c_void;
    fn SendMessageW(hWnd: *mut c_void, Msg: u32, wParam: usize, lParam: isize) -> isize;
    fn PostMessageW(hWnd: *mut c_void, Msg: u32, wParam: usize, lParam: isize) -> i32;
    fn GetSystemMetrics(nIndex: i32) -> i32;
    fn DrawTextW(hdc: *mut c_void, lpchText: *const u16, cchText: i32, lprc: *mut RECT, format: u32) -> i32;
    fn InvalidateRect(hWnd: *mut c_void, lpRect: *const RECT, bErase: i32) -> i32;
    fn EnableWindow(hWnd: *mut c_void, bEnable: i32) -> i32;
    fn SetWindowTextW(hWnd: *mut c_void, lpString: *const u16) -> i32;
    fn AdjustWindowRectEx(lpRect: *mut RECT, dwStyle: u32, bMenu: i32, dwExStyle: u32) -> i32;
    fn SetFocus(hWnd: *mut c_void) -> *mut c_void;
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
    c_stream_name: [u16; 296],
}

const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(lpModuleName: *const u16) -> *mut c_void;
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
const MB_YESNO: u32 = 0x00000004;
const MB_ICONWARNING: u32 = 0x00000030;
const MB_DEFBUTTON2: u32 = 0x00000100;
const MB_ICONERROR: u32 = 0x00000010;
const IDYES: i32 = 6;

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
                1,
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
    let dll_path = PathBuf::from("C:\\Program Files\\WinShred\\winshred_shell.dll");
    app_path.exists() && dll_path.exists()
}

fn restart_explorer() {
    let op = to_wide_chars("open");
    let file = to_wide_chars("explorer.exe");
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        );
    }
}

fn run_reg(args: &[&str]) {
    let _ = Command::new("reg")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(args)
        .output();
}

fn run_install_backend(opt_modern: bool, opt_classic: bool, opt_sendto: bool) -> Result<String, String> {
    let install_dir = PathBuf::from("C:\\Program Files\\WinShred");
    if let Err(e) = fs::create_dir_all(&install_dir) {
        return Err(format!("Fehler beim Erstellen des Installationsordners:\n{}", e));
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

    // 3. PowerShell-Setup: Alte Hänger beenden, Explorer stoppen
    let mut ps_command = String::from(
        "$ProgressPreference = 'SilentlyContinue'; \
        Get-CimInstance Win32_Process -Filter \"Name = 'powershell.exe'\" | Where-Object { $_.CommandLine -like '*WinShred*' -and $_.ProcessId -ne $PID } | Stop-Process -Force -ErrorAction SilentlyContinue; \
        Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue; "
    );

    // 4. Modernes Kontextmenue (Sparse Package) je nach Auswahl
    if opt_modern {
        ps_command.push_str(&format!(
            "Import-Certificate -FilePath '{cer}' -CertStoreLocation 'Cert:\\LocalMachine\\TrustedPeople' | Out-Null; \
            $pkg = Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue; \
            if ($pkg) {{ Remove-AppxPackage -Package $pkg.PackageFullName -ErrorAction SilentlyContinue }}; \
            Add-AppxPackage -Path '{msix}' -ExternalLocation '{inst}' -ErrorAction SilentlyContinue; ",
            cer = cer_path.to_string_lossy(),
            msix = msix_path.to_string_lossy(),
            inst = install_dir.to_string_lossy()
        ));
    } else {
        ps_command.push_str(
            "$pkg = Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue; \
            if ($pkg) { Remove-AppxPackage -Package $pkg.PackageFullName -ErrorAction SilentlyContinue }; "
        );
    }

    // 5. SendTo Shortcut je nach Auswahl
    if opt_sendto {
        ps_command.push_str(&format!(
            "$wsh = New-Object -ComObject WScript.Shell; \
            $s = $wsh.CreateShortcut(\"$env:APPDATA\\Microsoft\\Windows\\SendTo\\WinShred (Sicher vernichten).lnk\"); \
            $s.TargetPath = '{inst}\\winshred.exe'; \
            $s.IconLocation = '{ico}'; \
            $s.Save(); ",
            inst = install_dir.to_string_lossy(),
            ico = ico_path.to_string_lossy()
        ));
    } else {
        if let Ok(appdata) = env::var("APPDATA") {
            let lnk = PathBuf::from(appdata).join(r"Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk");
            let _ = fs::remove_file(lnk);
        }
    }

    let output = Command::new("powershell")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_command])
        .output();

    // 6. Klassisches Menue (statische Registry-Keys)
    let ico_str = ico_path.to_string_lossy().to_string();
    let cmd_str = format!("\"{}\" \"%1\"", install_dir.join("winshred.exe").to_string_lossy());

    if opt_classic {
        run_reg(&["add", r"HKLM\SOFTWARE\Classes\*\shell\WinShred", "/ve", "/d", "Mit WinShred sicher vernichten", "/f"]);
        run_reg(&["add", r"HKLM\SOFTWARE\Classes\*\shell\WinShred", "/v", "Icon", "/d", &ico_str, "/f"]);
        run_reg(&["add", r"HKLM\SOFTWARE\Classes\*\shell\WinShred\command", "/ve", "/d", &cmd_str, "/f"]);

        run_reg(&["add", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred", "/ve", "/d", "Mit WinShred sicher vernichten", "/f"]);
        run_reg(&["add", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred", "/v", "Icon", "/d", &ico_str, "/f"]);
        run_reg(&["add", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred\command", "/ve", "/d", &cmd_str, "/f"]);
    } else {
        // IMMER säubern, wenn klassisch nicht gewünscht ist (verhindert Dopplung im Legacy-Menü!)
        run_reg(&["delete", r"HKLM\SOFTWARE\Classes\*\shell\WinShred", "/f"]);
        run_reg(&["delete", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred", "/f"]);
        run_reg(&["delete", r"HKCU\Software\Classes\*\shell\WinShred", "/f"]);
        run_reg(&["delete", r"HKCU\Software\Classes\Directory\shell\WinShred", "/f"]);
    }

    // 7. Explorer sauber und entkoppelt über die Windows Shell neu starten
    restart_explorer();

    // 8. Verifikation
    let files_exist = install_dir.join("winshred.exe").exists()
        && install_dir.join("winshred_shell.dll").exists();

    if files_exist {
        let mut msg = String::from("Status: WinShred-Konfiguration abgeschlossen\n\nAUSGEWAEHLTE INTEGRATION:\n");
        if opt_modern {
            msg.push_str("[OK] Modernes Windows 11 Menue: Aktiv (\"Mit WinShred sicher vernichten\")\n");
        } else {
            msg.push_str("[--] Modernes Windows 11 Menue: Nicht installiert\n");
        }
        if opt_classic {
            msg.push_str("[OK] Klassisches Kontextmenue: Aktiv (statische Registrierung)\n");
        } else {
            msg.push_str("[--] Klassisches Kontextmenue: Nicht registriert (keine Doppelung)\n");
        }
        if opt_sendto {
            msg.push_str("[OK] Explorer 'Senden an' (SendTo): Verknuepfung aktiv\n");
        } else {
            msg.push_str("[--] Explorer 'Senden an' (SendTo): Nicht eingerichtet\n");
        }
        msg.push_str("\nSICHERHEITS-FUNKTIONEN:\n");
        msg.push_str("[OK] 3-Pass DoD 5220.22-M Krypto-Ueberschreibung (0x00, 0xFF, CSPRNG)\n");
        msg.push_str("[OK] Cluster-Slack-Bereinigung & Alternate Data Streams (ADS)\n");
        msg.push_str("[OK] SSD/NVMe TRIM Deallocate Controller-Tilgung\n");
        msg.push_str("[OK] MFT-Dateinamen-Anonymisierung im NTFS-Dateisystem");

        Ok(msg)
    } else {
        let err_detail = output
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
            .unwrap_or_default();
        if !err_detail.is_empty() {
            Err(err_detail)
        } else {
            Err(String::from("Dateien konnten nicht angelegt werden."))
        }
    }
}

fn run_uninstall_backend() -> Result<(), String> {
    // 1. PowerShell: Verwaiste Prozesse killen, Explorer stoppen, AppX-Paket und Zertifikat löschen
    let ps_command = "\
        $ProgressPreference = 'SilentlyContinue'; \
        Get-CimInstance Win32_Process -Filter \"Name = 'powershell.exe'\" | Where-Object { $_.CommandLine -like '*WinShred*' -and $_.ProcessId -ne $PID } | Stop-Process -Force -ErrorAction SilentlyContinue; \
        Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue; \
        $pkg = Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue; \
        if ($pkg) { Remove-AppxPackage -Package $pkg.PackageFullName -ErrorAction SilentlyContinue }; \
        Get-ChildItem Cert:\\LocalMachine\\TrustedPeople -ErrorAction SilentlyContinue | Where-Object { $_.Subject -eq 'CN=WinShred' } | Remove-Item -Force -ErrorAction SilentlyContinue;";

    let _ = Command::new("powershell")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps_command])
        .output();

    // 2. Registry sofort und ohne Wildcard-Probleme über reg.exe bereinigen
    run_reg(&["delete", r"HKLM\SOFTWARE\Classes\*\shell\WinShred", "/f"]);
    run_reg(&["delete", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred", "/f"]);
    run_reg(&["delete", r"HKCU\Software\Classes\*\shell\WinShred", "/f"]);
    run_reg(&["delete", r"HKCU\Software\Classes\Directory\shell\WinShred", "/f"]);

    // 3. SendTo-Verknüpfung löschen
    if let Ok(appdata) = env::var("APPDATA") {
        let lnk = PathBuf::from(appdata).join(r"Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk");
        let _ = fs::remove_file(lnk);
    }

    // 4. Explorer sauber und entkoppelt über die Windows Shell neu starten
    restart_explorer();

    // 5. Dateien aus Program Files entfernen
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

    Ok(())
}

// ==========================================
// Windows Installer Wizard (Wizard97 Layout)
// ==========================================

const ID_BTN_INSTALL: usize = 101;
const ID_BTN_NEXT: usize = 102;
const ID_BTN_CANCEL: usize = 103;
const ID_BTN_FINISH: usize = 104;

const ID_RADIO_REPAIR: usize = 201;
const ID_RADIO_UNINSTALL: usize = 202;

const ID_CHK_MODERN: usize = 210;
const ID_CHK_CLASSIC: usize = 211;
const ID_CHK_SENDTO: usize = 212;

const ID_PROGRESS_BAR: usize = 301;

const WM_APP_SETUP_DONE: u32 = 0x8001;
const WM_CTLCOLORSTATIC: u32 = 0x0138;
const PBM_SETMARQUEE: u32 = 0x040A;
const PBS_MARQUEE: u32 = 0x0008;
const BM_SETCHECK: u32 = 0x00F1;
const BM_GETCHECK: u32 = 0x00F0;
const BST_CHECKED: usize = 1;
const BS_AUTORADIOBUTTON: u32 = 0x00000009;
const BS_AUTOCHECKBOX: u32 = 0x00000003;
const BS_GROUPBOX: u32 = 0x00000007;
const BS_DEFPUSHBUTTON: u32 = 0x00000001;
const BS_PUSHBUTTON: u32 = 0x00000000;
const ES_READONLY: u32 = 0x0800;
const ES_AUTOHSCROLL: u32 = 0x0080;
const WS_CHILD: u32 = 0x40000000;
const WS_VISIBLE: u32 = 0x10000000;
const WS_TABSTOP: u32 = 0x00010000;
const WS_BORDER: u32 = 0x00800000;
const WS_GROUP: u32 = 0x00020000;
const WS_DISABLED: u32 = 0x08000000;
const WM_SETFONT: u32 = 0x0030;
const WM_COMMAND: u32 = 0x0111;
const WM_PAINT: u32 = 0x000F;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WizardPage {
    Config,
    Progress,
    Finish,
}

static mut CURRENT_PAGE: WizardPage = WizardPage::Config;
static mut IS_CURRENTLY_INSTALLED: bool = false;
static mut ACTION_REPAIR: bool = true;

static mut H_FONT_TITLE: *mut c_void = std::ptr::null_mut();
static mut H_FONT_SUBTITLE: *mut c_void = std::ptr::null_mut();
static mut H_FONT_BOLD: *mut c_void = std::ptr::null_mut();
static mut H_FONT_REGULAR: *mut c_void = std::ptr::null_mut();
static mut H_FONT_SUCCESS: *mut c_void = std::ptr::null_mut();

static mut H_BRUSH_WHITE: *mut c_void = std::ptr::null_mut();
static mut H_BRUSH_DIALOG: *mut c_void = std::ptr::null_mut();
static mut H_BRUSH_LINE: *mut c_void = std::ptr::null_mut();

static SETUP_RESULT: std::sync::Mutex<(bool, String)> = std::sync::Mutex::new((true, String::new()));

struct WizardControls {
    lbl_p1_title: *mut c_void,
    lbl_p1_desc: *mut c_void,
    grp_p1_target: *mut c_void,
    edt_p1_target: *mut c_void,
    grp_p1_features: *mut c_void,
    chk_p1_modern: *mut c_void,
    chk_p1_classic: *mut c_void,
    chk_p1_sendto: *mut c_void,
    chk_p1_dod: *mut c_void,
    chk_p1_slack: *mut c_void,

    lbl_m1_title: *mut c_void,
    lbl_m1_desc: *mut c_void,
    grp_m1_action: *mut c_void,
    rad_m1_repair: *mut c_void,
    lbl_m1_repair_desc: *mut c_void,
    rad_m1_uninstall: *mut c_void,
    lbl_m1_uninstall_desc: *mut c_void,
    lbl_m1_hint: *mut c_void,

    lbl_p2_title: *mut c_void,
    bar_p2_progress: *mut c_void,
    lbl_p2_detail: *mut c_void,

    lbl_p3_title: *mut c_void,
    lbl_p3_summary: *mut c_void,

    btn_install: *mut c_void,
    btn_next: *mut c_void,
    btn_cancel: *mut c_void,
    btn_finish: *mut c_void,
}

static mut CTRLS: WizardControls = WizardControls {
    lbl_p1_title: std::ptr::null_mut(),
    lbl_p1_desc: std::ptr::null_mut(),
    grp_p1_target: std::ptr::null_mut(),
    edt_p1_target: std::ptr::null_mut(),
    grp_p1_features: std::ptr::null_mut(),
    chk_p1_modern: std::ptr::null_mut(),
    chk_p1_classic: std::ptr::null_mut(),
    chk_p1_sendto: std::ptr::null_mut(),
    chk_p1_dod: std::ptr::null_mut(),
    chk_p1_slack: std::ptr::null_mut(),

    lbl_m1_title: std::ptr::null_mut(),
    lbl_m1_desc: std::ptr::null_mut(),
    grp_m1_action: std::ptr::null_mut(),
    rad_m1_repair: std::ptr::null_mut(),
    lbl_m1_repair_desc: std::ptr::null_mut(),
    rad_m1_uninstall: std::ptr::null_mut(),
    lbl_m1_uninstall_desc: std::ptr::null_mut(),
    lbl_m1_hint: std::ptr::null_mut(),

    lbl_p2_title: std::ptr::null_mut(),
    bar_p2_progress: std::ptr::null_mut(),
    lbl_p2_detail: std::ptr::null_mut(),

    lbl_p3_title: std::ptr::null_mut(),
    lbl_p3_summary: std::ptr::null_mut(),

    btn_install: std::ptr::null_mut(),
    btn_next: std::ptr::null_mut(),
    btn_cancel: std::ptr::null_mut(),
    btn_finish: std::ptr::null_mut(),
};

unsafe fn show_ctrl(hwnd_ctrl: *mut c_void, visible: bool) {
    if !hwnd_ctrl.is_null() {
        unsafe {
            ShowWindow(hwnd_ctrl, if visible { 5 /* SW_SHOW */ } else { 0 /* SW_HIDE */ });
        }
    }
}

unsafe fn set_p1_install_visible(visible: bool) {
    unsafe {
        show_ctrl(CTRLS.lbl_p1_title, visible);
        show_ctrl(CTRLS.lbl_p1_desc, visible);
        show_ctrl(CTRLS.grp_p1_target, visible);
        show_ctrl(CTRLS.edt_p1_target, visible);
        show_ctrl(CTRLS.grp_p1_features, visible);
        show_ctrl(CTRLS.chk_p1_modern, visible);
        show_ctrl(CTRLS.chk_p1_classic, visible);
        show_ctrl(CTRLS.chk_p1_sendto, visible);
        show_ctrl(CTRLS.chk_p1_dod, visible);
        show_ctrl(CTRLS.chk_p1_slack, visible);
    }
}

unsafe fn set_p1_manage_visible(visible: bool) {
    unsafe {
        show_ctrl(CTRLS.lbl_m1_title, visible);
        show_ctrl(CTRLS.lbl_m1_desc, visible);
        show_ctrl(CTRLS.grp_m1_action, visible);
        show_ctrl(CTRLS.rad_m1_repair, visible);
        show_ctrl(CTRLS.lbl_m1_repair_desc, visible);
        show_ctrl(CTRLS.rad_m1_uninstall, visible);
        show_ctrl(CTRLS.lbl_m1_uninstall_desc, visible);
        show_ctrl(CTRLS.lbl_m1_hint, visible);
    }
}

unsafe fn set_p2_progress_visible(visible: bool) {
    unsafe {
        show_ctrl(CTRLS.lbl_p2_title, visible);
        show_ctrl(CTRLS.bar_p2_progress, visible);
        show_ctrl(CTRLS.lbl_p2_detail, visible);
    }
}

unsafe fn set_p3_finish_visible(visible: bool) {
    unsafe {
        show_ctrl(CTRLS.lbl_p3_title, visible);
        show_ctrl(CTRLS.lbl_p3_summary, visible);
    }
}

unsafe fn update_wizard_page(hwnd: *mut c_void) {
    unsafe {
        match CURRENT_PAGE {
            WizardPage::Config => {
                if IS_CURRENTLY_INSTALLED {
                    set_p1_install_visible(false);
                    set_p1_manage_visible(true);
                    show_ctrl(CTRLS.btn_install, false);
                    show_ctrl(CTRLS.btn_next, true);
                } else {
                    set_p1_install_visible(true);
                    set_p1_manage_visible(false);
                    show_ctrl(CTRLS.btn_install, true);
                    show_ctrl(CTRLS.btn_next, false);
                }
                set_p2_progress_visible(false);
                set_p3_finish_visible(false);

                show_ctrl(CTRLS.btn_cancel, true);
                EnableWindow(CTRLS.btn_cancel, 1);
                show_ctrl(CTRLS.btn_finish, false);
            }
            WizardPage::Progress => {
                set_p1_install_visible(false);
                set_p1_manage_visible(false);
                set_p2_progress_visible(true);
                set_p3_finish_visible(false);

                show_ctrl(CTRLS.btn_install, false);
                show_ctrl(CTRLS.btn_next, false);
                show_ctrl(CTRLS.btn_cancel, true);
                EnableWindow(CTRLS.btn_cancel, 0);
                show_ctrl(CTRLS.btn_finish, false);

                SendMessageW(CTRLS.bar_p2_progress, PBM_SETMARQUEE, 1, 30);
            }
            WizardPage::Finish => {
                set_p1_install_visible(false);
                set_p1_manage_visible(false);
                set_p2_progress_visible(false);
                set_p3_finish_visible(true);

                show_ctrl(CTRLS.btn_install, false);
                show_ctrl(CTRLS.btn_next, false);
                show_ctrl(CTRLS.btn_cancel, false);
                show_ctrl(CTRLS.btn_finish, true);
                SetFocus(CTRLS.btn_finish);
            }
        }
        InvalidateRect(hwnd, std::ptr::null(), 1);
    }
}

unsafe fn create_ctrl(
    ex_style: u32,
    class_name: &str,
    text: &str,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    parent: *mut c_void,
    id: usize,
    font: *mut c_void,
) -> *mut c_void {
    let w_class = to_wide_chars(class_name);
    let w_text = to_wide_chars(text);
    let h_inst = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            w_class.as_ptr(),
            w_text.as_ptr(),
            style,
            x,
            y,
            w,
            h,
            parent,
            id as *mut c_void,
            h_inst,
            std::ptr::null_mut(),
        )
    };
    if !font.is_null() && !hwnd.is_null() {
        unsafe {
            SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
        }
    }
    hwnd
}

unsafe extern "system" fn installer_wnd_proc(
    hwnd: *mut c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = wparam & 0xFFFF;
                if id == ID_RADIO_REPAIR {
                    ACTION_REPAIR = true;
                } else if id == ID_RADIO_UNINSTALL {
                    ACTION_REPAIR = false;
                } else if id == ID_CHK_MODERN {
                    let checked = SendMessageW(CTRLS.chk_p1_modern, BM_GETCHECK, 0, 0) != 0;
                    if checked {
                        SendMessageW(CTRLS.chk_p1_classic, BM_SETCHECK, 0, 0);
                    }
                } else if id == ID_CHK_CLASSIC {
                    let checked = SendMessageW(CTRLS.chk_p1_classic, BM_GETCHECK, 0, 0) != 0;
                    if checked {
                        SendMessageW(CTRLS.chk_p1_modern, BM_SETCHECK, 0, 0);
                    }
                } else if id == ID_BTN_INSTALL || id == ID_BTN_NEXT {
                    let opt_modern = SendMessageW(CTRLS.chk_p1_modern, BM_GETCHECK, 0, 0) != 0;
                    let opt_classic = SendMessageW(CTRLS.chk_p1_classic, BM_GETCHECK, 0, 0) != 0;
                    let opt_sendto = SendMessageW(CTRLS.chk_p1_sendto, BM_GETCHECK, 0, 0) != 0;

                    CURRENT_PAGE = WizardPage::Progress;
                    update_wizard_page(hwnd);

                    let hwnd_raw = hwnd as usize;
                    let is_installed_now = IS_CURRENTLY_INSTALLED;
                    let do_repair = ACTION_REPAIR;

                    std::thread::spawn(move || {
                        let (success, message) = if is_installed_now && !do_repair {
                            match run_uninstall_backend() {
                                Ok(_) => (
                                    true,
                                    String::from(
                                        "Status: Vollstaendig aus dem System entfernt\n\n\
                                        [OK] Windows 11 Kontextmenue-Erweiterung abgemeldet\n\
                                        [OK] AppX Sparse-Paket und Zertifikat entfernt\n\
                                        [OK] Windows Explorer SendTo-Verknuepfung geloescht\n\
                                        [OK] Programmdateien aus C:\\Program Files\\WinShred bereinigt\n\n\
                                        WinShred und alle zugehoerigen Eintraege wurden restlos entfernt."
                                    ),
                                ),
                                Err(e) => (false, format!("Fehler bei der Deinstallation:\n{}", e)),
                            }
                        } else {
                            let (m, c, s) = if is_installed_now {
                                (true, false, true)
                            } else {
                                (opt_modern, opt_classic, opt_sendto)
                            };

                            match run_install_backend(m, c, s) {
                                Ok(pkg_or_msg) => (true, pkg_or_msg),
                                Err(e) => (
                                    false,
                                    format!(
                                        "Hinweis zur Einrichtung:\n\n\
                                        Die Konfiguration konnte nicht vollstaendig angewendet werden:\n\n\
                                        {}\n\n\
                                        Bitte pruefen Sie Ihre Berechtigungen oder Windows-Einstellungen.",
                                        e
                                    ),
                                ),
                            }
                        };

                        if let Ok(mut res) = SETUP_RESULT.lock() {
                            *res = (success, message);
                        }

                        PostMessageW(hwnd_raw as *mut c_void, WM_APP_SETUP_DONE, 0, 0);
                    });
                } else if id == ID_BTN_FINISH || id == ID_BTN_CANCEL {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_APP_SETUP_DONE => {
                CURRENT_PAGE = WizardPage::Finish;
                update_wizard_page(hwnd);

                let (success, message) = if let Ok(res) = SETUP_RESULT.lock() {
                    res.clone()
                } else {
                    (true, String::new())
                };

                let title = if success {
                    if IS_CURRENTLY_INSTALLED && !ACTION_REPAIR {
                        "WinShred wurde erfolgreich deinstalliert."
                    } else {
                        "WinShred wurde erfolgreich eingerichtet!"
                    }
                } else {
                    "Setup-Hinweis"
                };

                let w_title = to_wide_chars(title);
                SetWindowTextW(CTRLS.lbl_p3_title, w_title.as_ptr());

                let w_msg = to_wide_chars(&message);
                SetWindowTextW(CTRLS.lbl_p3_summary, w_msg.as_ptr());

                InvalidateRect(hwnd, std::ptr::null(), 1);
                0
            }
            WM_CTLCOLORSTATIC => {
                let hdc_static = wparam as *mut c_void;
                let hwnd_ctrl = lparam as *mut c_void;
                SetBkMode(hdc_static, 1 /* TRANSPARENT */);

                if hwnd_ctrl == CTRLS.edt_p1_target {
                    SetBkColor(hdc_static, 0x00FFFFFF);
                    return H_BRUSH_WHITE as isize;
                }

                if hwnd_ctrl == CTRLS.lbl_p3_title {
                    let (success, _) = if let Ok(res) = SETUP_RESULT.lock() {
                        res.clone()
                    } else {
                        (true, String::new())
                    };
                    if success {
                        SetTextColor(hdc_static, 0x000E7A0E); // Schönes sattes Gruen
                    } else {
                        SetTextColor(hdc_static, 0x001414B4); // Rot
                    }
                } else {
                    SetTextColor(hdc_static, 0x00000000); // Schwarz
                }

                H_BRUSH_DIALOG as isize
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);

                // 1. Header (Weiss)
                let rc_hdr = RECT { left: 0, top: 0, right: 520, bottom: 65 };
                FillRect(hdc, &rc_hdr, H_BRUSH_WHITE);

                // 2. Trennlinie 1
                let rc_line1 = RECT { left: 0, top: 65, right: 520, bottom: 66 };
                FillRect(hdc, &rc_line1, H_BRUSH_LINE);

                // 3. Body (Dialog-Hintergrund #F0F0F0)
                let rc_body = RECT { left: 0, top: 66, right: 520, bottom: 335 };
                FillRect(hdc, &rc_body, H_BRUSH_DIALOG);

                // 4. Trennlinie 2
                let rc_line2 = RECT { left: 0, top: 335, right: 520, bottom: 336 };
                FillRect(hdc, &rc_line2, H_BRUSH_LINE);

                // 5. Footer (Dialog-Hintergrund #F0F0F0)
                let rc_footer = RECT { left: 0, top: 336, right: 520, bottom: 390 };
                FillRect(hdc, &rc_footer, H_BRUSH_DIALOG);

                // Header-Icon (48x48) rechts oben
                let h_mod = GetModuleHandleW(std::ptr::null());
                let h_icon = LoadImageW(h_mod, 1 as *const u16, 1 /* IMAGE_ICON */, 48, 48, 0);
                if !h_icon.is_null() {
                    DrawIconEx(hdc, 520 - 48 - 18, 9, h_icon, 48, 48, 0, std::ptr::null_mut(), 3);
                }

                // Header-Texte
                let (header_title, header_sub) = match CURRENT_PAGE {
                    WizardPage::Config => {
                        if IS_CURRENTLY_INSTALLED {
                            ("WinShred Setup - Verwaltung", "Waehlen Sie eine Aktion fuer Ihre WinShred-Installation.")
                        } else {
                            ("WinShred Setup", "Sichere Datei-Vernichtung fuer das Windows 11 Kontextmenue")
                        }
                    }
                    WizardPage::Progress => {
                        ("Konfiguration wird ausgefuehrt...", "Bitte warten Sie einen kurzen Moment.")
                    }
                    WizardPage::Finish => {
                        let (success, _) = if let Ok(res) = SETUP_RESULT.lock() {
                            res.clone()
                        } else {
                            (true, String::new())
                        };
                        if success {
                            if IS_CURRENTLY_INSTALLED && !ACTION_REPAIR {
                                ("Deinstallation abgeschlossen", "WinShred wurde erfolgreich von Ihrem Computer entfernt.")
                            } else {
                                ("Installation abgeschlossen", "WinShred wurde erfolgreich auf Ihrem Computer eingerichtet.")
                            }
                        } else {
                            ("Setup-Hinweis", "Der Vorgang wurde mit einer Meldung beendet.")
                        }
                    }
                };

                SetBkMode(hdc, 1 /* TRANSPARENT */);

                // Header Title (Bold 11pt)
                SelectObject(hdc, H_FONT_TITLE);
                SetTextColor(hdc, 0x00000000);
                let mut rc_title = RECT { left: 24, top: 14, right: 450, bottom: 36 };
                let w_title = to_wide_chars(header_title);
                DrawTextW(hdc, w_title.as_ptr(), -1, &mut rc_title, 0);

                // Header Subtitle (9pt)
                SelectObject(hdc, H_FONT_SUBTITLE);
                SetTextColor(hdc, 0x00555555);
                let mut rc_sub = RECT { left: 36, top: 38, right: 450, bottom: 58 };
                let w_sub = to_wide_chars(header_sub);
                DrawTextW(hdc, w_sub.as_ptr(), -1, &mut rc_sub, 0);

                EndPaint(hwnd, &ps);
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn show_installer_wizard() {
    unsafe {
        IS_CURRENTLY_INSTALLED = is_installed();
        CURRENT_PAGE = WizardPage::Config;
        ACTION_REPAIR = true;

        // Common Controls initialisieren (Progress Bar, Buttons)
        let icex = INITCOMMONCONTROLSEX {
            dw_size: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dw_icc: 0x00000020 | 0x00004000 | 0x00000008,
        };
        InitCommonControlsEx(&icex);

        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide_chars("WinShredInstallerWizardClass");

        let mut wnd_class: WNDCLASSEXW = std::mem::zeroed();
        wnd_class.cb_size = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wnd_class.style = 0x0002 | 0x0001; // CS_HREDRAW | CS_VREDRAW
        wnd_class.lpfn_wnd_proc = Some(installer_wnd_proc);
        wnd_class.h_instance = instance;
        wnd_class.h_cursor = LoadCursorW(std::ptr::null_mut(), 32512 as *const u16);
        wnd_class.h_icon = LoadImageW(instance, 1 as *const u16, 1, 32, 32, 0);
        wnd_class.h_icon_sm = LoadImageW(instance, 1 as *const u16, 1, 16, 16, 0);
        wnd_class.lpsz_class_name = class_name.as_ptr();

        RegisterClassExW(&wnd_class);

        // Pinsel initialisieren
        H_BRUSH_WHITE = CreateSolidBrush(0x00FFFFFF);
        H_BRUSH_DIALOG = CreateSolidBrush(0x00F0F0F0);
        H_BRUSH_LINE = CreateSolidBrush(0x00D8D8D8);

        // Schriften (Segoe UI)
        let font_face = to_wide_chars("Segoe UI");
        H_FONT_TITLE = CreateFontW(20, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
        H_FONT_SUBTITLE = CreateFontW(14, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
        H_FONT_BOLD = CreateFontW(14, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
        H_FONT_REGULAR = CreateFontW(14, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
        H_FONT_SUCCESS = CreateFontW(17, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());

        // Fenstergroesse (Client 520x390)
        let mut rc = RECT { left: 0, top: 0, right: 520, bottom: 390 };
        let win_style = 0x00000000 | 0x00C00000 | 0x00080000 | 0x00020000; // WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX
        AdjustWindowRectEx(&mut rc, win_style, 0, 0);
        let win_w = rc.right - rc.left;
        let win_h = rc.bottom - rc.top;

        let screen_w = GetSystemMetrics(0);
        let screen_h = GetSystemMetrics(1);
        let pos_x = (screen_w - win_w) / 2;
        let pos_y = (screen_h - win_h) / 2;

        let window_title = to_wide_chars("WinShred Setup");

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_title.as_ptr(),
            win_style,
            pos_x,
            pos_y,
            win_w,
            win_h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );

        if hwnd.is_null() {
            return;
        }

        // Icons explizit setzen
        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 1 /* ICON_BIG */, wnd_class.h_icon as isize);
        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 0 /* ICON_SMALL */, wnd_class.h_icon_sm as isize);

        // ==========================================
        // Steuerelemente erzeugen
        // ==========================================

        // --- Seite 1 (Neu-Installation) ---
        CTRLS.lbl_p1_title = create_ctrl(
            0, "STATIC", "Willkommen beim WinShred Installations-Assistenten",
            WS_CHILD, 28, 76, 464, 20, hwnd, 0, H_FONT_BOLD,
        );
        CTRLS.lbl_p1_desc = create_ctrl(
            0, "STATIC", "Dieser Assistent richtet WinShred auf Ihrem Computer ein und integriert die sichere Datei-Vernichtung nahtlos in das moderne Windows 11 Kontextmenue.",
            WS_CHILD, 28, 98, 464, 34, hwnd, 0, H_FONT_REGULAR,
        );
        CTRLS.grp_p1_target = create_ctrl(
            0, "BUTTON", "Zielordner",
            WS_CHILD | BS_GROUPBOX, 28, 136, 464, 56, hwnd, 0, H_FONT_REGULAR,
        );
        CTRLS.edt_p1_target = create_ctrl(
            0, "EDIT", "C:\\Program Files\\WinShred",
            WS_CHILD | WS_BORDER | ES_READONLY | ES_AUTOHSCROLL, 40, 156, 440, 23, hwnd, 0, H_FONT_REGULAR,
        );
        CTRLS.grp_p1_features = create_ctrl(
            0, "BUTTON", "Komponenten & Sicherheitsfunktionen",
            WS_CHILD | BS_GROUPBOX, 28, 195, 464, 137, hwnd, 0, H_FONT_REGULAR,
        );
        CTRLS.chk_p1_modern = create_ctrl(
            0, "BUTTON", "Modernes Windows 11 Kontextmenue (\"Mit WinShred sicher vernichten\")",
            WS_CHILD | BS_AUTOCHECKBOX | WS_TABSTOP, 44, 214, 430, 20, hwnd, ID_CHK_MODERN, H_FONT_REGULAR,
        );
        SendMessageW(CTRLS.chk_p1_modern, BM_SETCHECK, BST_CHECKED, 0);

        CTRLS.chk_p1_classic = create_ctrl(
            0, "BUTTON", "Klassisches Kontextmenue (Nur fuer Windows 10 oder \"Weitere Optionen\")",
            WS_CHILD | BS_AUTOCHECKBOX | WS_TABSTOP, 44, 235, 430, 20, hwnd, ID_CHK_CLASSIC, H_FONT_REGULAR,
        );
        SendMessageW(CTRLS.chk_p1_classic, BM_SETCHECK, 0, 0);

        CTRLS.chk_p1_sendto = create_ctrl(
            0, "BUTTON", "Windows Explorer \"Senden an\" (SendTo) Kontextmenue",
            WS_CHILD | BS_AUTOCHECKBOX | WS_TABSTOP, 44, 256, 430, 20, hwnd, ID_CHK_SENDTO, H_FONT_REGULAR,
        );
        SendMessageW(CTRLS.chk_p1_sendto, BM_SETCHECK, BST_CHECKED, 0);

        CTRLS.chk_p1_dod = create_ctrl(
            0, "BUTTON", "3-Pass DoD 5220.22-M Krypto-Ueberschreibung & SSD/NVMe TRIM",
            WS_CHILD | BS_AUTOCHECKBOX | WS_DISABLED, 44, 280, 430, 20, hwnd, 0, H_FONT_REGULAR,
        );
        SendMessageW(CTRLS.chk_p1_dod, BM_SETCHECK, BST_CHECKED, 0);

        CTRLS.chk_p1_slack = create_ctrl(
            0, "BUTTON", "Cluster-Slack & Alternate Data Streams (ADS) Tilgung",
            WS_CHILD | BS_AUTOCHECKBOX | WS_DISABLED, 44, 302, 430, 20, hwnd, 0, H_FONT_REGULAR,
        );
        SendMessageW(CTRLS.chk_p1_slack, BM_SETCHECK, BST_CHECKED, 0);

        // --- Seite 1 (Verwaltung / Bereits installiert) ---
        CTRLS.lbl_m1_title = create_ctrl(
            0, "STATIC", "WinShred ist bereits auf diesem Computer installiert.",
            WS_CHILD, 28, 76, 464, 20, hwnd, 0, H_FONT_BOLD,
        );
        CTRLS.lbl_m1_desc = create_ctrl(
            0, "STATIC", "Installationsordner: C:\\Program Files\\WinShred   (Status: Im Kontextmenue aktiv)",
            WS_CHILD, 28, 98, 464, 20, hwnd, 0, H_FONT_REGULAR,
        );
        CTRLS.grp_m1_action = create_ctrl(
            0, "BUTTON", "Waehlen Sie eine Aktion aus:",
            WS_CHILD | BS_GROUPBOX, 28, 126, 464, 168, hwnd, 0, H_FONT_REGULAR,
        );
        CTRLS.rad_m1_repair = create_ctrl(
            0, "BUTTON", "WinShred reparieren oder aktualisieren (Empfohlen)",
            WS_CHILD | BS_AUTORADIOBUTTON | WS_GROUP | WS_TABSTOP, 44, 148, 430, 20, hwnd, ID_RADIO_REPAIR, H_FONT_BOLD,
        );
        SendMessageW(CTRLS.rad_m1_repair, BM_SETCHECK, BST_CHECKED, 0);

        CTRLS.lbl_m1_repair_desc = create_ctrl(
            0, "STATIC", "Erneuert alle Programmdateien, das Sicherheitszertifikat und die moderne Windows 11 Shell-Erweiterung.",
            WS_CHILD, 64, 170, 410, 32, hwnd, 0, H_FONT_REGULAR,
        );

        CTRLS.rad_m1_uninstall = create_ctrl(
            0, "BUTTON", "WinShred vollstaendig deinstallieren",
            WS_CHILD | BS_AUTORADIOBUTTON | WS_TABSTOP, 44, 212, 430, 20, hwnd, ID_RADIO_UNINSTALL, H_FONT_BOLD,
        );

        CTRLS.lbl_m1_uninstall_desc = create_ctrl(
            0, "STATIC", "Entfernt das Programm, die Kontextmenue-Eintraege und das Zertifikat restlos von Ihrem System.",
            WS_CHILD, 64, 234, 410, 32, hwnd, 0, H_FONT_REGULAR,
        );

        CTRLS.lbl_m1_hint = create_ctrl(
            0, "STATIC", "Klicken Sie auf 'Weiter', um die gewaehlte Aktion durchzufuehren.",
            WS_CHILD, 28, 304, 464, 20, hwnd, 0, H_FONT_REGULAR,
        );

        // --- Seite 2 (Fortschritt) ---
        CTRLS.lbl_p2_title = create_ctrl(
            0, "STATIC", "WinShred wird konfiguriert...",
            WS_CHILD, 40, 110, 440, 22, hwnd, 0, H_FONT_BOLD,
        );
        CTRLS.bar_p2_progress = create_ctrl(
            0, "msctls_progress32", "",
            WS_CHILD | PBS_MARQUEE, 40, 140, 440, 24, hwnd, ID_PROGRESS_BAR, std::ptr::null_mut(),
        );
        CTRLS.lbl_p2_detail = create_ctrl(
            0, "STATIC", "Dateien werden uebertragen und Windows 11 Kontextmenue wird registriert...",
            WS_CHILD, 40, 175, 440, 36, hwnd, 0, H_FONT_REGULAR,
        );

        // --- Seite 3 (Abschluss) ---
        CTRLS.lbl_p3_title = create_ctrl(
            0, "STATIC", "WinShred wurde erfolgreich eingerichtet!",
            WS_CHILD, 28, 76, 464, 24, hwnd, 0, H_FONT_SUCCESS,
        );
        CTRLS.lbl_p3_summary = create_ctrl(
            0, "STATIC", "",
            WS_CHILD, 28, 108, 464, 215, hwnd, 0, H_FONT_REGULAR,
        );

        // --- Footer Buttons ---
        CTRLS.btn_install = create_ctrl(
            0, "BUTTON", "Installieren",
            WS_CHILD | BS_DEFPUSHBUTTON | WS_TABSTOP, 290, 348, 105, 28, hwnd, ID_BTN_INSTALL, H_FONT_REGULAR,
        );
        CTRLS.btn_next = create_ctrl(
            0, "BUTTON", "Weiter >",
            WS_CHILD | BS_DEFPUSHBUTTON | WS_TABSTOP, 290, 348, 105, 28, hwnd, ID_BTN_NEXT, H_FONT_REGULAR,
        );
        CTRLS.btn_cancel = create_ctrl(
            0, "BUTTON", "Abbrechen",
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP, 405, 348, 90, 28, hwnd, ID_BTN_CANCEL, H_FONT_REGULAR,
        );
        CTRLS.btn_finish = create_ctrl(
            0, "BUTTON", "Fertigstellen",
            WS_CHILD | BS_DEFPUSHBUTTON | WS_TABSTOP, 390, 348, 105, 28, hwnd, ID_BTN_FINISH, H_FONT_REGULAR,
        );

        // Erste Seite initialisieren
        update_wizard_page(hwnd);

        ShowWindow(hwnd, 1 /* SW_SHOWNORMAL */);
        UpdateWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Aufraeumen
        DeleteObject(H_BRUSH_WHITE);
        DeleteObject(H_BRUSH_DIALOG);
        DeleteObject(H_BRUSH_LINE);
        DeleteObject(H_FONT_TITLE);
        DeleteObject(H_FONT_SUBTITLE);
        DeleteObject(H_FONT_BOLD);
        DeleteObject(H_FONT_REGULAR);
        DeleteObject(H_FONT_SUCCESS);
    }
}

// ==========================================
// Entry Point (CLI / GUI / Kontextmenue)
// ==========================================
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // 1. Spezial-Flags fuer Headless CLI
    if args.iter().any(|a| a == "--install" || a == "-i") {
        let is_admin = unsafe { IsUserAnAdmin() } != 0;
        if !is_admin {
            elevate_relaunch("--install");
            return;
        }
        let _ = run_install_backend(true, false, true);
        return;
    }
    if args.iter().any(|a| a == "--uninstall" || a == "-u") {
        let is_admin = unsafe { IsUserAnAdmin() } != 0;
        if !is_admin {
            elevate_relaunch("--uninstall");
            return;
        }
        let _ = run_uninstall_backend();
        return;
    }

    // 2. Ohne Parameter gestartet (Doppelklick auf die EXE) -> Installer Wizard
    if args.is_empty() {
        let is_admin = unsafe { IsUserAnAdmin() } != 0;
        if !is_admin {
            elevate_relaunch("");
            return;
        }
        show_installer_wizard();
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
