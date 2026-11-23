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
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ==========================================
// Eingebettete Ressourcen für All-in-One Setup
// ==========================================
const SHELL_DLL_BYTES: &[u8] = include_bytes!("../winshred_shell/target/release/winshred_shell.dll");
const MSIX_BYTES: &[u8] = include_bytes!("../packaging/winshred.msix");
const CER_BYTES: &[u8] = include_bytes!("../packaging/winshred.cer");
const ICO_BYTES: &[u8] = include_bytes!("../assets/winshred.ico");
const LICENSE_TEXT: &str = include_str!("../LICENSE");

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
    fn SetForegroundWindow(hWnd: *mut c_void) -> i32;
    fn SetCursor(hCursor: *mut c_void) -> *mut c_void;
    fn SetWindowPos(
        hWnd: *mut c_void,
        hWndInsertAfter: *mut c_void,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: u32,
    ) -> i32;
    fn BringWindowToTop(hWnd: *mut c_void) -> i32;
    fn GetForegroundWindow() -> *mut c_void;
    fn GetWindowThreadProcessId(hWnd: *mut c_void, lpdwProcessId: *mut u32) -> u32;
    fn AttachThreadInput(idAttach: u32, idAttachTo: u32, fAttach: i32) -> i32;
    fn SwitchToThisWindow(hWnd: *mut c_void, fUnknown: i32);
    fn SetActiveWindow(hWnd: *mut c_void) -> *mut c_void;
    fn AllowSetForegroundWindow(dwProcessId: u32) -> i32;
    fn IsDialogMessageW(hDlg: *mut c_void, lpMsg: *const MSG) -> i32;
    fn SetTimer(
        hWnd: *mut c_void,
        nIDEvent: usize,
        uElapse: u32,
        lpTimerFunc: *mut c_void,
    ) -> usize;
    fn KillTimer(hWnd: *mut c_void, uIDEvent: usize) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        hKey: *mut c_void,
        lpSubKey: *const u16,
        ulOptions: u32,
        samDesired: u32,
        phkResult: *mut *mut c_void,
    ) -> i32;
    fn RegCreateKeyExW(
        hKey: *mut c_void,
        lpSubKey: *const u16,
        Reserved: u32,
        lpClass: *mut u16,
        dwOptions: u32,
        samDesired: u32,
        lpSecurityAttributes: *mut c_void,
        phkResult: *mut *mut c_void,
        lpdwDisposition: *mut u32,
    ) -> i32;
    fn RegQueryValueExW(
        hKey: *mut c_void,
        lpValueName: *const u16,
        lpReserved: *mut u32,
        lpType: *mut u32,
        lpData: *mut u8,
        lpcbData: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        hKey: *mut c_void,
        lpValueName: *const u16,
        Reserved: u32,
        dwType: u32,
        lpData: *const u8,
        cbData: u32,
    ) -> i32;
    fn RegDeleteValueW(hKey: *mut c_void, lpValueName: *const u16) -> i32;
    fn RegCloseKey(hKey: *mut c_void) -> i32;
    fn OpenProcessToken(
        ProcessHandle: *mut c_void,
        DesiredAccess: u32,
        TokenHandle: *mut *mut c_void,
    ) -> i32;
    fn LookupPrivilegeValueW(
        lpSystemName: *const u16,
        lpName: *const u16,
        lpLuid: *mut LUID,
    ) -> i32;
    fn AdjustTokenPrivileges(
        TokenHandle: *mut c_void,
        DisableAllPrivileges: i32,
        NewState: *const TOKEN_PRIVILEGES,
        BufferLength: u32,
        PreviousState: *mut c_void,
        ReturnLength: *mut u32,
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
    fn SHChangeNotify(
        wEventId: i32,
        uFlags: u32,
        dwItem1: *const c_void,
        dwItem2: *const c_void,
    );
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

#[repr(C)]
struct FILETIME {
    dw_low_date_time: u32,
    dw_high_date_time: u32,
}

#[repr(C)]
struct FILE_STANDARD_INFO {
    allocation_size: i64,
    end_of_file: i64,
    number_of_links: u32,
    delete_pending: u8,
    directory: u8,
}

#[repr(C)]
struct STORAGE_PROPERTY_QUERY {
    property_id: u32,
    query_type: u32,
    additional_parameters: [u8; 1],
}

#[repr(C)]
struct DEVICE_SEEK_PENALTY_DESCRIPTOR {
    version: u32,
    size: u32,
    incurs_seek_penalty: u8,
}

#[repr(C)]
struct DEVICE_TRIM_DESCRIPTOR {
    version: u32,
    size: u32,
    trim_enabled: u8,
}

#[repr(C)]
struct USN_JOURNAL_DATA_V0 {
    usn_journal_id: u64,
    first_usn: i64,
    next_usn: i64,
    lowest_valid_usn: i64,
    max_usn: i64,
    maximum_size: u64,
    allocation_delta: u64,
}

#[repr(C)]
struct DELETE_USN_JOURNAL_DATA {
    usn_journal_id: u64,
    delete_flags: u32,
}

#[repr(C)]
struct CREATE_USN_JOURNAL_DATA {
    maximum_size: u64,
    allocation_delta: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LUID {
    low_part: u32,
    high_part: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LUID_AND_ATTRIBUTES {
    luid: LUID,
    attributes: u32,
}

#[repr(C)]
struct TOKEN_PRIVILEGES {
    privilege_count: u32,
    privileges: [LUID_AND_ATTRIBUTES; 1],
}

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

    fn GetFileAttributesW(lpFileName: *const u16) -> u32;
    fn SetFileAttributesW(lpFileName: *const u16, dwFileAttributes: u32) -> i32;
    fn MoveFileExW(
        lpExistingFileName: *const u16,
        lpNewFileName: *const u16,
        dwFlags: u32,
    ) -> i32;

    fn CreateFileW(
        lpFileName: *const u16,
        dwDesiredAccess: u32,
        dwShareMode: u32,
        lpSecurityAttributes: *mut c_void,
        dwCreationDisposition: u32,
        dwFlagsAndAttributes: u32,
        hTemplateFile: *mut c_void,
    ) -> *mut c_void;

    fn CloseHandle(hObject: *mut c_void) -> i32;

    fn SetFileTime(
        hFile: *mut c_void,
        lpCreationTime: *const FILETIME,
        lpLastAccessTime: *const FILETIME,
        lpLastWriteTime: *const FILETIME,
    ) -> i32;

    fn GetFileInformationByHandleEx(
        hFile: *mut c_void,
        FileInformationClass: u32,
        lpFileInformation: *mut c_void,
        dwBufferSize: u32,
    ) -> i32;

    fn GetLastError() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcess() -> *mut c_void;
    fn AttachConsole(dwProcessId: u32) -> i32;
    fn GetStdHandle(nStdHandle: u32) -> *mut c_void;
}

const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
const STD_ERROR_HANDLE: u32 = (-12i32) as u32;

fn attach_parent_console_if_available() -> bool {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            return true;
        }
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        let err = GetStdHandle(STD_ERROR_HANDLE);
        (!out.is_null() && out != INVALID_HANDLE_VALUE)
            || (!err.is_null() && err != INVALID_HANDLE_VALUE)
    }
}

const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D1400;
const STORAGE_DEVICE_SEEK_PENALTY_PROPERTY: u32 = 7;
const STORAGE_DEVICE_TRIM_PROPERTY: u32 = 8;
const PROPERTY_STANDARD_QUERY: u32 = 0;

const FSCTL_GET_RETRIEVAL_POINTERS: u32 = 0x00090073;
const FSCTL_QUERY_ALLOCATED_RANGES: u32 = 0x000940CF;
const FSCTL_QUERY_USN_JOURNAL: u32 = 0x000900F4;
const FSCTL_DELETE_USN_JOURNAL: u32 = 0x000900F8;
const FSCTL_CREATE_USN_JOURNAL: u32 = 0x000900e7;
const USN_DELETE_FLAG_DELETE: u32 = 0x00000001;
const USN_DELETE_FLAG_NOTIFY: u32 = 0x00000002;

const ERROR_INVALID_FUNCTION: u32 = 1;
const ERROR_HANDLE_EOF: u32 = 38;
const ERROR_NOT_SUPPORTED: u32 = 50;
const ERROR_FILE_EXISTS: u32 = 80;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_ALREADY_EXISTS: u32 = 183;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_JOURNAL_DELETE_IN_PROGRESS: u32 = 1178;
const ERROR_JOURNAL_NOT_ACTIVE: u32 = 1179;

const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
const TOKEN_QUERY: u32 = 0x0008;
const SE_PRIVILEGE_ENABLED: u32 = 0x00000002;

const FILE_STANDARD_INFO_CLASS: u32 = 1;

const GENERIC_READ: u32 = 0x80000000;
const GENERIC_WRITE: u32 = 0x40000000;
const FILE_SHARE_READ: u32 = 0x00000001;
const FILE_SHARE_WRITE: u32 = 0x00000002;
const FILE_SHARE_DELETE: u32 = 0x00000004;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;

const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x00000002;
#[allow(dead_code)]
const MB_YESNO: u32 = 0x00000004;
const MB_ICONWARNING: u32 = 0x00000030;
#[allow(dead_code)]
const MB_DEFBUTTON2: u32 = 0x00000100;
const MB_ICONERROR: u32 = 0x00000010;
const MB_ICONINFORMATION: u32 = 0x00000040;
const MB_TOPMOST: u32 = 0x00040000;
const MB_SETFOREGROUND: u32 = 0x00010000;
#[allow(dead_code)]
const IDYES: i32 = 6;

const SHCNE_ALLEVENTS: i32 = 0x7FFFFFFF;
const SHCNF_PATHW: u32 = 0x0005;
const SHCNF_FLUSH: u32 = 0x1000;
const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFFFFFF;
const FILE_ATTRIBUTE_READONLY: u32 = 0x00000001;
#[cfg(test)]
const FILE_ATTRIBUTE_NORMAL: u32 = 0x00000080;
const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x00000200;
const IDC_WAIT: usize = 32514;
const LR_SHARED: u32 = 0x00008000;

const HKEY_CURRENT_USER: *mut c_void = (-2147483647i32 as isize) as *mut c_void;
const KEY_READ: u32 = 0x00020019;
const KEY_WRITE: u32 = 0x00020006;
const REG_DWORD: u32 = 4;

const FSCTL_FILE_LEVEL_TRIM: u32 = 0x00098208;

const HWND_TOPMOST: *mut c_void = -1isize as *mut c_void;
const HWND_NOTOPMOST: *mut c_void = -2isize as *mut c_void;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_SHOWWINDOW: u32 = 0x0040;
const ASFW_ANY: u32 = 0xFFFFFFFF;

fn to_wide_chars(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn path_to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub unsafe fn force_foreground_window(hwnd: *mut c_void) {
    unsafe {
        force_foreground_window_with_topmost(hwnd, true);
    }
}

pub unsafe fn force_foreground_window_with_topmost(hwnd: *mut c_void, keep_topmost: bool) {
    if hwnd.is_null() {
        return;
    }

    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);

        ShowWindow(hwnd, 1 /* SW_SHOWNORMAL */);
        UpdateWindow(hwnd);

        // 1. Z-Order kurzzeitig auf HWND_TOPMOST setzen und anzeigen, um Window-Focus durchzusetzen
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        BringWindowToTop(hwnd);

        // 2. AttachThreadInput-Kopplung mit aktuellem Vordergrundfenster
        let fg_hwnd = GetForegroundWindow();
        if !fg_hwnd.is_null() && fg_hwnd != hwnd {
            let fg_thread = GetWindowThreadProcessId(fg_hwnd, std::ptr::null_mut());
            let cur_thread = GetCurrentThreadId();
            if cur_thread != fg_thread {
                AttachThreadInput(cur_thread, fg_thread, 1);
                SetForegroundWindow(hwnd);
                BringWindowToTop(hwnd);
                SetFocus(hwnd);
                AttachThreadInput(cur_thread, fg_thread, 0);
            } else {
                SetForegroundWindow(hwnd);
                BringWindowToTop(hwnd);
                SetFocus(hwnd);
            }
        } else {
            SetForegroundWindow(hwnd);
            BringWindowToTop(hwnd);
            SetFocus(hwnd);
        }

        // 3. SwitchToThisWindow & SetActiveWindow
        SwitchToThisWindow(hwnd, 1);
        SetActiveWindow(hwnd);
        SetFocus(hwnd);

        // 4. Endgültige Z-Order setzen (Topmost beibehalten oder wieder freigeben)
        SetWindowPos(
            hwnd,
            if keep_topmost { HWND_TOPMOST } else { HWND_NOTOPMOST },
            0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
}

fn show_message_box(text: &str, title: &str, flags: u32) -> i32 {
    let wide_text = to_wide_chars(text);
    let wide_title = to_wide_chars(title);
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
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
            AllowSetForegroundWindow(ASFW_ANY);
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

/// RAII-Wrapper für rohe Win32-Handles, der beim Verlassen des Scopes `CloseHandle` aufruft.
struct OwnedHandle(*mut c_void);

impl OwnedHandle {
    fn from_raw(handle: *mut c_void) -> Option<Self> {
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }

    fn as_raw(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// Löst relative Pfade sowie `.` und `..` auf, ohne Symlinks oder Junctions aufzulösen.
fn resolve_absolute_no_symlink(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| {
        if path.is_relative() {
            env::current_dir()
                .map(|cd| cd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        } else {
            path.to_path_buf()
        }
    })
}

/// Extrahiert den normalisierten Laufwerksbuchstaben (z. B. `"C:"`) für einen lokalen Pfad.
fn extract_drive_letter(path: &Path) -> Option<String> {
    let abs_path = resolve_absolute_no_symlink(path);
    match abs_path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => {
            let s = prefix.as_os_str().to_string_lossy();
            let trimmed = s.trim_start_matches(r"\\?\").trim_end_matches('\\');
            if trimmed.len() == 2 && trimmed.ends_with(':') {
                Some(trimmed.to_ascii_uppercase())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn get_cluster_size(path: &Path) -> u64 {
    let mut sectors_per_cluster: u32 = 0;
    let mut bytes_per_sector: u32 = 0;
    let mut free_clusters: u32 = 0;
    let mut total_clusters: u32 = 0;

    let abs_path = resolve_absolute_no_symlink(path);

    let root_path = abs_path.components().next().map(|c| {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveMediaType {
    SsdNvme { trim_supported: bool },
    RotationalHdd,
    Unknown,
}

pub fn get_drive_media_type(path: &Path) -> DriveMediaType {
    let Some(drive_letter) = extract_drive_letter(path) else {
        return DriveMediaType::Unknown;
    };

    let volume_path = format!(r"\\.\{}", drive_letter);
    let wide_vol = to_wide_chars(&volume_path);

    let Some(handle) = OwnedHandle::from_raw(unsafe {
        CreateFileW(
            wide_vol.as_ptr(),
            0, // Keine Lese-/Schreibberechtigung für Property-Queries nötig!
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    }) else {
        return DriveMediaType::Unknown;
    };

    let query = STORAGE_PROPERTY_QUERY {
        property_id: STORAGE_DEVICE_SEEK_PENALTY_PROPERTY,
        query_type: PROPERTY_STANDARD_QUERY,
        additional_parameters: [0],
    };

    let mut seek_penalty = DEVICE_SEEK_PENALTY_DESCRIPTOR {
        version: 0,
        size: 0,
        incurs_seek_penalty: 1, // Standard: HDD
    };
    let mut bytes_returned: u32 = 0;

    let success = unsafe {
        DeviceIoControl(
            handle.as_raw(),
            IOCTL_STORAGE_QUERY_PROPERTY,
            &query as *const _ as *const c_void,
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            &mut seek_penalty as *mut _ as *mut c_void,
            std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };

    if success == 0 {
        return DriveMediaType::Unknown;
    }

    // TRIM-Unterstützung prüfen
    let trim_query = STORAGE_PROPERTY_QUERY {
        property_id: STORAGE_DEVICE_TRIM_PROPERTY,
        query_type: PROPERTY_STANDARD_QUERY,
        additional_parameters: [0],
    };
    let mut trim_desc = DEVICE_TRIM_DESCRIPTOR {
        version: 0,
        size: 0,
        trim_enabled: 0,
    };
    let trim_success = unsafe {
        DeviceIoControl(
            handle.as_raw(),
            IOCTL_STORAGE_QUERY_PROPERTY,
            &trim_query as *const _ as *const c_void,
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            &mut trim_desc as *mut _ as *mut c_void,
            std::mem::size_of::<DEVICE_TRIM_DESCRIPTOR>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };

    let trim_supported = trim_success != 0 && trim_desc.trim_enabled != 0;

    if seek_penalty.incurs_seek_penalty == 0 {
        DriveMediaType::SsdNvme { trim_supported }
    } else {
        DriveMediaType::RotationalHdd
    }
}

/// Prüft, ob die Daten einer Datei direkt resident im NTFS-MFT-Eintrag gespeichert sind.
pub fn is_file_resident_in_mft(file: &fs::File, len: u64) -> bool {
    if len == 0 {
        return true;
    }
    // Standard MFT-Records sind 1024 Bytes groß. Daten > 1024 Bytes sind nie resident.
    if len > 1024 {
        return false;
    }

    let handle = file.as_raw_handle();

    // Primäre Verifikation über FSCTL_GET_RETRIEVAL_POINTERS:
    // Residente NTFS-Streams haben (auch wenn FileStandardInfo.AllocationSize > 0, z. B. 32 Bytes meldet)
    // keinerlei physische Cluster-Extents und liefern ERROR_HANDLE_EOF (38) oder extent_count == 0.
    #[repr(C)]
    struct StartingVcnInputBuffer {
        starting_vcn: i64,
    }
    #[repr(C)]
    struct RetrievalPointersBuffer {
        extent_count: u32,
        starting_vcn: i64,
        extents: [u64; 2],
    }

    let in_buf = StartingVcnInputBuffer { starting_vcn: 0 };
    let mut out_buf: RetrievalPointersBuffer = unsafe { std::mem::zeroed() };
    let mut bytes_ret = 0u32;

    let c_ret = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_RETRIEVAL_POINTERS,
            &in_buf as *const _ as *const c_void,
            std::mem::size_of::<StartingVcnInputBuffer>() as u32,
            &mut out_buf as *mut _ as *mut c_void,
            std::mem::size_of::<RetrievalPointersBuffer>() as u32,
            &mut bytes_ret,
            std::ptr::null_mut(),
        )
    };

    if c_ret != 0 {
        return out_buf.extent_count == 0;
    }

    let err = unsafe { GetLastError() };
    if err == ERROR_HANDLE_EOF {
        return true;
    }
    if err == ERROR_MORE_DATA {
        return false;
    }

    // Fallback nur, wenn Retrieval-Pointers nicht unterstützt werden:
    // Auf NTFS meldet dann höchstens AllocationSize == 0 eine residente Datei.
    let mut std_info = FILE_STANDARD_INFO {
        allocation_size: 0,
        end_of_file: 0,
        number_of_links: 0,
        delete_pending: 0,
        directory: 0,
    };

    let ret = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FILE_STANDARD_INFO_CLASS,
            &mut std_info as *mut _ as *mut c_void,
            std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
        )
    };

    ret != 0 && std_info.allocation_size == 0 && std_info.end_of_file > 0
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FileAllocatedRangeBuffer {
    file_offset: i64,
    length: i64,
}

fn is_sparse_file(path: &Path) -> bool {
    let wide = path_to_wide(path);
    let attrs = unsafe { GetFileAttributesW(wide.as_ptr()) };
    attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_SPARSE_FILE) != 0
}

/// Fragt alle physisch allokierten Bereiche einer (Sparse-)Datei innerhalb von `[0, total_len)` ab.
pub fn query_allocated_ranges(file: &fs::File, total_len: u64) -> io::Result<Vec<(u64, u64)>> {
    if total_len == 0 {
        return Ok(Vec::new());
    }

    let handle = file.as_raw_handle();
    let total_i64 = i64::try_from(total_len).unwrap_or(i64::MAX);
    let mut ranges = Vec::new();
    let mut cursor: i64 = 0;

    while cursor < total_i64 {
        let query = FileAllocatedRangeBuffer {
            file_offset: cursor,
            length: total_i64 - cursor,
        };
        let mut out_buf = [FileAllocatedRangeBuffer {
            file_offset: 0,
            length: 0,
        }; 64];
        let mut bytes_ret: u32 = 0;

        let ok = unsafe {
            DeviceIoControl(
                handle,
                FSCTL_QUERY_ALLOCATED_RANGES,
                &query as *const _ as *const c_void,
                std::mem::size_of::<FileAllocatedRangeBuffer>() as u32,
                out_buf.as_mut_ptr() as *mut c_void,
                std::mem::size_of_val(&out_buf) as u32,
                &mut bytes_ret,
                std::ptr::null_mut(),
            )
        };

        let err = if ok == 0 { unsafe { GetLastError() } } else { 0 };
        if ok == 0 && err != ERROR_MORE_DATA {
            return Err(io::Error::from_raw_os_error(err as i32));
        }

        let elem_size = std::mem::size_of::<FileAllocatedRangeBuffer>();
        let count = (bytes_ret as usize) / elem_size;
        if count == 0 {
            break;
        }

        for item in &out_buf[..count] {
            if item.file_offset >= 0 && item.length > 0 {
                let start = item.file_offset as u64;
                let end = start.saturating_add(item.length as u64).min(total_len);
                if end > start {
                    ranges.push((start, end - start));
                }
            }
            let next_pos = item.file_offset.saturating_add(item.length);
            if next_pos > cursor {
                cursor = next_pos;
            }
        }

        if ok != 0 {
            break;
        }
    }

    Ok(ranges)
}

pub fn trim_file(file: &fs::File, len: u64) -> bool {
    if len == 0 {
        return true;
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
    let ret = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_FILE_LEVEL_TRIM,
            &trim_input as *const _ as *const c_void,
            std::mem::size_of::<FileLevelTrim>() as u32,
            &mut trim_output as *mut _ as *mut c_void,
            std::mem::size_of::<FileLevelTrimOutput>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };

    ret != 0 && trim_output.num_ranges_processed > 0
}

pub fn neutralize_timestamps(file: &fs::File) {
    // Windows FileTime Epoch: 1601-01-01 00:00:00.0000001 (dw_low = 1, dw_high = 0)
    // Hinweis: (0,0) wird von SetFileTime als 'nicht aendern' ignoriert, daher low = 1
    let ft = FILETIME { dw_low_date_time: 1, dw_high_date_time: 0 };
    unsafe {
        SetFileTime(
            file.as_raw_handle() as *mut c_void,
            &ft,
            &ft,
            &ft,
        );
    }
}

pub fn neutralize_dir_timestamps(path: &Path) {
    let wide = path_to_wide(path);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
        let ft = FILETIME { dw_low_date_time: 1, dw_high_date_time: 0 };
        unsafe {
            SetFileTime(handle, &ft, &ft, &ft);
            CloseHandle(handle);
        }
    }
}

pub trait ShredReporter: Send {
    fn is_cancelled(&self) -> bool { false }
    fn on_item_start(&mut self, _path: &Path, _is_dir: bool) {}
    fn on_bytes_written(&mut self, _bytes: u64) {}
    fn on_item_done(&mut self, _path: &Path, _is_dir: bool) {}
}

pub struct NoopShredReporter;
impl ShredReporter for NoopShredReporter {}

pub fn wipe_open_file(
    file: &mut fs::File,
    wipe_len: u64,
    num_passes: usize,
    is_ssd: bool,
    is_resident: bool,
) -> io::Result<()> {
    wipe_open_file_with_reporter(file, wipe_len, num_passes, is_ssd, is_resident, &mut NoopShredReporter)
}

pub fn wipe_open_file_with_reporter<R: ShredReporter>(
    file: &mut fs::File,
    wipe_len: u64,
    num_passes: usize,
    is_ssd: bool,
    is_resident: bool,
    reporter: &mut R,
) -> io::Result<()> {
    if wipe_len == 0 {
        return Ok(());
    }
    wipe_open_ranges_with_reporter(
        file,
        &[(0, wipe_len)],
        wipe_len,
        num_passes,
        is_ssd,
        is_resident,
        reporter,
    )
}

/// Überschreibt gezielt alle angegebenen `(offset, len)`-Bereiche einer offenen Datei,
/// sendet bei SSDs ein TRIM-Deallocate, neutralisiert Zeitstempel und kürzt auf 0 Bytes.
pub fn wipe_open_ranges_with_reporter<R: ShredReporter>(
    file: &mut fs::File,
    ranges: &[(u64, u64)],
    trim_len: u64,
    num_passes: usize,
    is_ssd: bool,
    is_resident: bool,
    reporter: &mut R,
) -> io::Result<()> {
    let total_range_bytes: u64 = ranges.iter().map(|&(_, l)| l).sum();
    if total_range_bytes == 0 && trim_len == 0 {
        return Ok(());
    }

    if total_range_bytes > 0 {
        const CHUNK_SIZE: usize = 64 * 1024;
        let zero_chunk = vec![0u8; CHUNK_SIZE];
        let one_chunk = vec![0xFFu8; CHUNK_SIZE];
        let mut rand_chunk = vec![0u8; CHUNK_SIZE];

        // Auf SSD/NVMe oder bei residenten MFT-Dateien ist 1 Pass optimal:
        // Schützt Flash-Endurance (P/E-Cycles) und überschreibt MFT-Payload direkt in-place
        let effective_passes = if is_ssd || is_resident {
            1
        } else {
            num_passes.max(1)
        };

        for pass in 0..effective_passes {
            if reporter.is_cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Vorgang durch Benutzer abgebrochen",
                ));
            }

            // Wenn 1 Pass: Krypto-Zufall (NIST SP 800-88 konform)
            // Wenn 3 Passes: DoD 5220.22-M (0x00, 0xFF, Krypto-Zufall)
            let pattern_type = if effective_passes == 1 { 2 } else { pass % 3 };

            for &(offset, range_len) in ranges {
                if range_len == 0 {
                    continue;
                }
                file.seek(SeekFrom::Start(offset))?;
                let mut remaining = range_len;

                while remaining > 0 {
                    if reporter.is_cancelled() {
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "Vorgang durch Benutzer abgebrochen",
                        ));
                    }

                    let write_size = std::cmp::min(remaining, CHUNK_SIZE as u64) as usize;
                    match pattern_type {
                        0 => file.write_all(&zero_chunk[..write_size])?,
                        1 => file.write_all(&one_chunk[..write_size])?,
                        _ => {
                            fill_random_bytes(&mut rand_chunk[..write_size])?;
                            file.write_all(&rand_chunk[..write_size])?;
                        }
                    }
                    remaining -= write_size as u64;
                    reporter.on_bytes_written(write_size as u64);
                }
            }
            file.sync_all()?;
        }
    }

    // Hardware TRIM Deallocate senden (nur bei nicht-residenten Cluster-Dateien auf Flash)
    if is_ssd && !is_resident && trim_len > 0 {
        let _ = trim_file(file, trim_len);
    }

    // Zeitstempel auf 1601-01-01 neutralisieren (Timestomp)
    neutralize_timestamps(file);

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

fn shred_alternate_streams(path: &Path, num_passes: usize, is_ssd: bool) -> io::Result<()> {
    let streams = get_file_streams(path);

    for stream_name in streams {
        let mut stream_path = path.as_os_str().to_os_string();
        stream_path.push(&stream_name);
        let stream_path = PathBuf::from(stream_path);

        if let Ok(mut file) = OpenOptions::new().write(true).read(true).open(&stream_path) {
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if len > 0 {
                let is_res = is_file_resident_in_mft(&file, len);
                let _ = wipe_open_file(&mut file, len, num_passes, is_ssd, is_res);
            }
        }

        let wide_stream_path = path_to_wide(&stream_path);
        unsafe {
            DeleteFileW(wide_stream_path.as_ptr());
        }
    }
    Ok(())
}

/// Benennt `from` nach `to` um, ohne eine bereits existierende Zieldatei zu überschreiben (`dwFlags = 0`).
fn move_file_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Zieldatei existiert bereits",
        ));
    }

    let wide_from = path_to_wide(from);
    let wide_to = path_to_wide(to);
    const MAX_RETRIES: usize = 3;

    for attempt in 0..MAX_RETRIES {
        let ok = unsafe { MoveFileExW(wide_from.as_ptr(), wide_to.as_ptr(), 0) };
        if ok != 0 {
            return Ok(());
        }
        let err = unsafe { GetLastError() };
        if err == ERROR_FILE_EXISTS || err == ERROR_ALREADY_EXISTS {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Zieldatei existiert bereits",
            ));
        }
        // Kurzzeitige Sperren durch Explorer/Indexer kurz abwarten
        if (err == 5 || err == 32) && attempt + 1 < MAX_RETRIES {
            std::thread::sleep(std::time::Duration::from_millis(30));
            continue;
        }
        return Err(io::Error::from_raw_os_error(err as i32));
    }

    Err(io::Error::last_os_error())
}

fn generate_random_hex_name(hex_chars: usize, prefix: &str, suffix: &str) -> io::Result<String> {
    let byte_len = hex_chars.div_ceil(2).max(4);
    let mut rand_bytes = vec![0u8; byte_len];
    fill_random_bytes(&mut rand_bytes)?;
    let mut name = String::with_capacity(prefix.len() + hex_chars + suffix.len());
    name.push_str(prefix);
    for b in &rand_bytes {
        name.push_str(&format!("{:02x}", b));
    }
    name.truncate(prefix.len() + hex_chars);
    name.push_str(suffix);
    Ok(name)
}

fn rename_step_unique(
    current: &Path,
    parent: &Path,
    hex_chars: usize,
    prefix: &str,
    suffix: &str,
) -> io::Result<PathBuf> {
    for _ in 0..16 {
        let candidate_name = generate_random_hex_name(hex_chars, prefix, suffix)?;
        let candidate_path = parent.join(candidate_name);
        match move_file_no_replace(current, &candidate_path) {
            Ok(()) => return Ok(candidate_path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Kein kollisionsfreier Zufallsname gefunden",
    ))
}

/// Anonymisiert den Datei- oder Ordnernamen mehrfach kollisionssicher in der MFT / im Verzeichnisindex.
pub fn secure_single_rename(path: &Path) -> io::Result<PathBuf> {
    let abs_path = resolve_absolute_no_symlink(path);
    let clean_path = strip_trailing_separators(&abs_path);
    let parent = match clean_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Ok(clean_path),
    };

    let mut current_path = clean_path.clone();
    let mut ro_guard = ReadonlyGuard::acquire(&current_path);

    // Runde 1: Langer Zufallsname (überschreibt lange Originalnamen im Directory-Index, falls Pfadlänge reicht)
    let orig_len = current_path
        .file_name()
        .map(|n| n.to_string_lossy().len())
        .unwrap_or(16);
    let long_len = orig_len.clamp(24, 64);
    if let Ok(next) = rename_step_unique(&current_path, parent, long_len, "s_", ".tmp") {
        current_path = next;
        ro_guard.disarm();
    }

    // Runde 2: Mittlerer Zufallsname
    if let Ok(next) = rename_step_unique(&current_path, parent, 20, "w_", ".dat") {
        current_path = next;
        ro_guard.disarm();
    }

    // Runde 3 (Final): Kurzer Zufallsname mit `shred_`-Präfix; Fehler werden hier explizit gemeldet
    let final_path = rename_step_unique(&current_path, parent, 16, "shred_", ".tmp")?;
    ro_guard.disarm();
    Ok(final_path)
}

fn enable_manage_volume_privilege() {
    unsafe {
        let mut token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            return;
        }
        let _token_guard = OwnedHandle::from_raw(token);
        let priv_name = to_wide_chars("SeManageVolumePrivilege");
        let mut luid = LUID { low_part: 0, high_part: 0 };
        if LookupPrivilegeValueW(std::ptr::null(), priv_name.as_ptr(), &mut luid) != 0 {
            let tp = TOKEN_PRIVILEGES {
                privilege_count: 1,
                privileges: [LUID_AND_ATTRIBUTES {
                    luid,
                    attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            AdjustTokenPrivileges(
                token,
                0,
                &tp,
                std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }
}

fn wait_for_usn_journal_deletion(handle: &OwnedHandle) -> io::Result<()> {
    let mut del_notify = DELETE_USN_JOURNAL_DATA {
        usn_journal_id: 0,
        delete_flags: USN_DELETE_FLAG_NOTIFY,
    };
    let mut bytes_ret = 0u32;
    let ret = unsafe {
        DeviceIoControl(
            handle.as_raw(),
            FSCTL_DELETE_USN_JOURNAL,
            &mut del_notify as *mut _ as *mut c_void,
            std::mem::size_of::<DELETE_USN_JOURNAL_DATA>() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_ret,
            std::ptr::null_mut(),
        )
    };
    if ret != 0 {
        return Ok(());
    }
    let err = unsafe { GetLastError() };
    if err == ERROR_JOURNAL_NOT_ACTIVE {
        return Ok(());
    }
    Err(io::Error::from_raw_os_error(err as i32))
}

fn create_usn_journal_with_retry(
    handle: &OwnedHandle,
    volume_path: &str,
    max_size: u64,
    alloc_delta: u64,
) -> io::Result<()> {
    let mut create_data = CREATE_USN_JOURNAL_DATA {
        maximum_size: max_size,
        allocation_delta: alloc_delta,
    };
    let mut bytes_ret = 0u32;
    let mut last_err = 0u32;

    for _ in 0..30 {
        let c_ret = unsafe {
            DeviceIoControl(
                handle.as_raw(),
                FSCTL_CREATE_USN_JOURNAL,
                &mut create_data as *mut _ as *mut c_void,
                std::mem::size_of::<CREATE_USN_JOURNAL_DATA>() as u32,
                std::ptr::null_mut(),
                0,
                &mut bytes_ret,
                std::ptr::null_mut(),
            )
        };
        if c_ret != 0 {
            return Ok(());
        }
        last_err = unsafe { GetLastError() };
        if last_err == ERROR_JOURNAL_DELETE_IN_PROGRESS {
            let _ = wait_for_usn_journal_deletion(handle);
            std::thread::sleep(std::time::Duration::from_millis(150));
            continue;
        }
        if last_err == ERROR_INVALID_PARAMETER
            && (create_data.maximum_size != 0 || create_data.allocation_delta != 0)
        {
            create_data.maximum_size = 0;
            create_data.allocation_delta = 0;
            continue;
        }
        break;
    }

    let ce = io::Error::from_raw_os_error(last_err as i32);
    Err(io::Error::new(
        ce.kind(),
        format!(
            "FSCTL_CREATE_USN_JOURNAL auf {} fehlgeschlagen (Code {}): {}",
            volume_path, last_err, ce
        ),
    ))
}

/// Löscht das NTFS USN-Journal auf dem Volume von `volume_root` und legt es mit derselben Konfiguration neu an.
/// Gibt `Ok(true)` zurück, wenn ein Journal bereinigt wurde, oder `Ok(false)`, wenn kein aktives Journal vorliegt.
pub fn purge_volume_usn_journal(volume_root: &Path) -> io::Result<bool> {
    let Some(drive_letter) = extract_drive_letter(volume_root) else {
        return Ok(false);
    };

    enable_manage_volume_privilege();

    let volume_path = format!(r"\\.\{}", drive_letter);
    let wide_vol = to_wide_chars(&volume_path);

    let handle = OwnedHandle::from_raw(unsafe {
        CreateFileW(
            wide_vol.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    })
    .ok_or_else(|| {
        let e = io::Error::last_os_error();
        io::Error::new(
            e.kind(),
            format!(
                "Volume-Zugriff {} fehlgeschlagen (Code {:?}): {}",
                volume_path,
                e.raw_os_error(),
                e
            ),
        )
    })?;

    // 1. Aktuelle Journal-Konfiguration abfragen
    let mut journal_data = USN_JOURNAL_DATA_V0 {
        usn_journal_id: 0,
        first_usn: 0,
        next_usn: 0,
        lowest_valid_usn: 0,
        max_usn: 0,
        maximum_size: 0,
        allocation_delta: 0,
    };
    let mut bytes_ret = 0u32;
    let q_ret = unsafe {
        DeviceIoControl(
            handle.as_raw(),
            FSCTL_QUERY_USN_JOURNAL,
            std::ptr::null(),
            0,
            &mut journal_data as *mut _ as *mut c_void,
            std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32,
            &mut bytes_ret,
            std::ptr::null_mut(),
        )
    };

    if q_ret == 0 {
        let err = unsafe { GetLastError() };
        if err == ERROR_JOURNAL_DELETE_IN_PROGRESS {
            wait_for_usn_journal_deletion(&handle)?;
            let retry_ret = unsafe {
                DeviceIoControl(
                    handle.as_raw(),
                    FSCTL_QUERY_USN_JOURNAL,
                    std::ptr::null(),
                    0,
                    &mut journal_data as *mut _ as *mut c_void,
                    std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32,
                    &mut bytes_ret,
                    std::ptr::null_mut(),
                )
            };
            if retry_ret == 0 {
                let retry_err = unsafe { GetLastError() };
                if retry_err == ERROR_JOURNAL_NOT_ACTIVE {
                    create_usn_journal_with_retry(&handle, &volume_path, 0, 0)?;
                    return Ok(true);
                }
                if retry_err == ERROR_INVALID_FUNCTION || retry_err == ERROR_NOT_SUPPORTED {
                    return Ok(false);
                }
                let qe = io::Error::from_raw_os_error(retry_err as i32);
                return Err(io::Error::new(
                    qe.kind(),
                    format!(
                        "FSCTL_QUERY_USN_JOURNAL auf {} fehlgeschlagen (Code {}): {}",
                        volume_path, retry_err, qe
                    ),
                ));
            }
        } else if err == ERROR_JOURNAL_NOT_ACTIVE
            || err == ERROR_INVALID_FUNCTION
            || err == ERROR_NOT_SUPPORTED
        {
            return Ok(false);
        } else {
            let qe = io::Error::from_raw_os_error(err as i32);
            return Err(io::Error::new(
                qe.kind(),
                format!(
                    "FSCTL_QUERY_USN_JOURNAL auf {} fehlgeschlagen (Code {}): {}",
                    volume_path, err, qe
                ),
            ));
        }
    }

    if journal_data.usn_journal_id == 0 {
        return Ok(false);
    }

    // 2. Bestehendes Journal synchron löschen (USN_DELETE_FLAG_NOTIFY wartet auf Abschluss!)
    let del_data = DELETE_USN_JOURNAL_DATA {
        usn_journal_id: journal_data.usn_journal_id,
        delete_flags: USN_DELETE_FLAG_DELETE | USN_DELETE_FLAG_NOTIFY,
    };
    let d_ret = unsafe {
        DeviceIoControl(
            handle.as_raw(),
            FSCTL_DELETE_USN_JOURNAL,
            &del_data as *const _ as *const c_void,
            std::mem::size_of::<DELETE_USN_JOURNAL_DATA>() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_ret,
            std::ptr::null_mut(),
        )
    };

    if d_ret == 0 {
        let err = unsafe { GetLastError() };
        if err == ERROR_JOURNAL_DELETE_IN_PROGRESS {
            wait_for_usn_journal_deletion(&handle)?;
        } else if err != ERROR_JOURNAL_NOT_ACTIVE {
            let de = io::Error::from_raw_os_error(err as i32);
            return Err(io::Error::new(
                de.kind(),
                format!(
                    "FSCTL_DELETE_USN_JOURNAL auf {} fehlgeschlagen (Code {}): {}",
                    volume_path, err, de
                ),
            ));
        }
    }

    // 3. Journal leer mit ursprünglichen Parametern (oder Fallback 0/0) neu anlegen
    create_usn_journal_with_retry(
        &handle,
        &volume_path,
        journal_data.maximum_size,
        journal_data.allocation_delta,
    )?;

    Ok(true)
}

/// Entfernt temporär ausschließlich das Read-Only-Bit und stellt es bei Drop wieder her,
/// sofern der Vorgang zuvor fehlgeschlagen oder abgebrochen wurde.
struct ReadonlyGuard {
    path: PathBuf,
    original_attrs: Option<u32>,
    active: bool,
}

impl ReadonlyGuard {
    fn acquire(path: &Path) -> Self {
        let wide = path_to_wide(path);
        let attrs = unsafe { GetFileAttributesW(wide.as_ptr()) };
        if attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_READONLY) != 0 {
            let new_attrs = attrs & !FILE_ATTRIBUTE_READONLY;
            let ok = unsafe { SetFileAttributesW(wide.as_ptr(), new_attrs) };
            if ok != 0 {
                return Self {
                    path: path.to_path_buf(),
                    original_attrs: Some(attrs),
                    active: true,
                };
            }
        }
        Self {
            path: path.to_path_buf(),
            original_attrs: None,
            active: false,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for ReadonlyGuard {
    fn drop(&mut self) {
        if self.active
            && let Some(orig) = self.original_attrs
        {
            let wide = path_to_wide(&self.path);
            unsafe {
                SetFileAttributesW(wide.as_ptr(), orig);
            }
        }
    }
}

fn strip_trailing_separators(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    // Laufwerks-Roots wie "C:\" behalten ihren Backslash
    if s.len() <= 3 && s.as_bytes().get(1) == Some(&b':') {
        return path.to_path_buf();
    }
    let trimmed = s.trim_end_matches(['\\', '/']);
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        PathBuf::from(format!("{}\\", trimmed))
    } else if trimmed.is_empty() {
        path.to_path_buf()
    } else {
        PathBuf::from(trimmed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSafetyLevel {
    Safe,
    Important(String),
    Blocked(String),
}

fn normalize_win_path_for_compare(path: &Path, follow_symlinks: bool) -> String {
    let resolved = if follow_symlinks {
        path.canonicalize()
            .unwrap_or_else(|_| resolve_absolute_no_symlink(path))
    } else {
        resolve_absolute_no_symlink(path)
    };

    let s = resolved.to_string_lossy();
    let without_verbatim = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    };

    without_verbatim.trim_end_matches(['\\', '/']).to_lowercase()
}

fn is_same_or_subpath(candidate: &str, base: &str) -> bool {
    if base.is_empty() {
        return false;
    }
    if candidate == base {
        return true;
    }
    candidate.len() > base.len()
        && candidate.starts_with(base)
        && candidate.as_bytes()[base.len()] == b'\\'
}

fn is_strict_subpath(candidate: &str, base: &str) -> bool {
    !base.is_empty()
        && candidate.len() > base.len()
        && candidate.starts_with(base)
        && candidate.as_bytes()[base.len()] == b'\\'
}

fn is_unc_share_or_server_root(norm: &str) -> bool {
    if let Some(rest) = norm.strip_prefix(r"\\") {
        let parts: Vec<&str> = rest.split('\\').filter(|p| !p.is_empty()).collect();
        return parts.len() <= 2;
    }
    false
}

fn env_norm(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| normalize_win_path_for_compare(Path::new(&v), false))
        .filter(|v| !v.is_empty())
}

fn classify_normalized_path(norm: &str) -> PathSafetyLevel {
    if norm.is_empty()
        || (norm.len() == 2 && norm.ends_with(':'))
        || is_unc_share_or_server_root(norm)
    {
        return PathSafetyLevel::Blocked("Laufwerks- oder Netzwerkfreigabe-Stammverzeichnis".to_string());
    }

    // Laufwerksunabhängige System-/Boot-Container direkt auf beliebigen Laufwerken (<Drive>:\<ordner>)
    if norm.len() > 3 && norm.as_bytes().get(1) == Some(&b':') && norm.as_bytes().get(2) == Some(&b'\\') {
        let after_drive = &norm[3..];
        let first_segment = after_drive.split('\\').next().unwrap_or("");
        let drive_prefix = &norm[..2];

        for critical_sub in ["boot", "efi", "recovery", "system volume information"] {
            let full = format!("{}\\{}", drive_prefix, critical_sub);
            if is_same_or_subpath(norm, &full) {
                return PathSafetyLevel::Blocked(format!(
                    "System-/Boot-Verzeichnis ({})",
                    full.to_uppercase()
                ));
            }
        }
        if after_drive.eq_ignore_ascii_case("$recycle.bin") {
            return PathSafetyLevel::Blocked("Windows-Papierkorb-Stammverzeichnis ($Recycle.Bin)".to_string());
        }

        // Kritische Root-Systemdateien oder bekannte Root-Ordner direkt auf System/Datenlaufwerk
        for important_root_dir in ["windows.old", "perflogs", "inetpub"] {
            let full = format!("{}\\{}", drive_prefix, important_root_dir);
            if is_same_or_subpath(norm, &full) {
                return PathSafetyLevel::Important(format!(
                    "Relevanter System-/Installationsordner ({})",
                    important_root_dir
                ));
            }
        }
        if first_segment == after_drive
            && matches!(
                first_segment,
                "pagefile.sys" | "hiberfil.sys" | "swapfile.sys" | "bootmgr" | "bootnxt"
            )
        {
            return PathSafetyLevel::Important(format!("Systemdatei im Stammverzeichnis ({})", first_segment));
        }
    }

    // Windows-Systemverzeichnis & Kern-Unterordner
    if let Some(sys_root) = env_norm("SystemRoot") {
        if norm == sys_root {
            return PathSafetyLevel::Blocked("Windows-Systemverzeichnis (%SystemRoot%)".to_string());
        }
        for core_sub in [
            "system32",
            "syswow64",
            "winsxs",
            "boot",
            "systemapps",
            "systemresources",
            "servicing",
            "fonts",
            "inf",
        ] {
            let core_path = format!("{}\\{}", sys_root, core_sub);
            if is_same_or_subpath(norm, &core_path) {
                return PathSafetyLevel::Blocked(format!(
                    "Geschützter Windows-Kernordner ({})",
                    core_path
                ));
            }
        }
        if is_strict_subpath(norm, &sys_root) {
            return PathSafetyLevel::Important(
                "Unterordner bzw. Datei im Windows-Verzeichnis (%SystemRoot%\\*)".to_string(),
            );
        }
    }

    // Programme & systemweite Programmdaten
    for (var_name, label) in [
        ("ProgramFiles", "Programme (%ProgramFiles%)"),
        ("ProgramFiles(x86)", "Programme x86 (%ProgramFiles(x86)%)"),
        ("ProgramData", "Systemweite Programmdaten (%ProgramData%)"),
    ] {
        if let Some(base) = env_norm(var_name) {
            if norm == base {
                return PathSafetyLevel::Blocked(label.to_string());
            }
            if is_strict_subpath(norm, &base) {
                return PathSafetyLevel::Important(format!("Installations-/App-Datenordner unter {}", label));
            }
        }
    }

    // Benutzer-Basisordner & Profile
    if let Some(sys_drive) = env_norm("SystemDrive") {
        let users_dir = format!("{}\\users", sys_drive);
        if norm == users_dir {
            return PathSafetyLevel::Blocked("Benutzer-Stammverzeichnis (\\Users)".to_string());
        }
        if is_same_or_subpath(norm, &format!("{}\\default", users_dir)) {
            return PathSafetyLevel::Blocked("Standard-Benutzervorlage (\\Users\\Default)".to_string());
        }
        if norm == format!("{}\\public", users_dir) {
            return PathSafetyLevel::Blocked("Öffentliches Benutzerverzeichnis (\\Users\\Public)".to_string());
        }
        // Direktes Profilverzeichnis C:\Users\<Name>
        if let Some(rest) = norm.strip_prefix(&format!("{}\\", users_dir))
            && !rest.contains('\\')
        {
            return PathSafetyLevel::Important(format!("Benutzerprofil-Stammverzeichnis ({})", norm));
        }
    }

    // Aktives Benutzerprofil, AppData und wichtige Haupt-Benutzerordner selbst
    if let Some(user_prof) = env_norm("USERPROFILE") {
        if norm == user_prof {
            return PathSafetyLevel::Important("Aktives Benutzerprofil (%USERPROFILE%)".to_string());
        }
        if norm == format!("{}\\appdata", user_prof) {
            return PathSafetyLevel::Important("Anwendungsdaten-Stammordner (AppData)".to_string());
        }
        for folder in [
            "desktop",
            "documents",
            "dokumente",
            "downloads",
            "pictures",
            "bilder",
            "music",
            "musik",
            "videos",
            "onedrive",
        ] {
            if norm == format!("{}\\{}", user_prof, folder) {
                return PathSafetyLevel::Important(format!(
                    "Wichtiger persönlicher Hauptordner ({})",
                    folder
                ));
            }
        }
    }

    for (var_name, label) in [
        ("APPDATA", "Roaming-Anwendungsdaten (%APPDATA%)"),
        ("LOCALAPPDATA", "Lokale Anwendungsdaten (%LOCALAPPDATA%)"),
    ] {
        if let Some(base) = env_norm(var_name)
            && norm == base
        {
            return PathSafetyLevel::Important(label.to_string());
        }
    }

    PathSafetyLevel::Safe
}

/// Klassifiziert einen Pfad in `Safe`, `Important` (erzwingt Warnung) oder `Blocked` (strikte Blockade).
/// Löst Symlinks/Junctions nicht blind als Blockade auf, sodass Links auf Systemordner als `Important` gelöscht werden dürfen.
pub fn classify_path_safety(path: &Path) -> PathSafetyLevel {
    if path.as_os_str().is_empty() {
        return PathSafetyLevel::Blocked("Leerer Pfad".to_string());
    }

    // 1. Prüfung des Pfads selbst ohne Symlink-Auflösung
    let own_norm = normalize_win_path_for_compare(path, false);
    let own_level = classify_normalized_path(&own_norm);
    if matches!(own_level, PathSafetyLevel::Blocked(_)) {
        return own_level;
    }

    // 2. Reparse-Point / Symlink prüfen: Ziel nicht stillschweigend blockieren, sondern nur warnen
    let is_symlink = fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    if is_symlink {
        if let Ok(canon) = path.canonicalize() {
            let target_norm = normalize_win_path_for_compare(&canon, false);
            match classify_normalized_path(&target_norm) {
                PathSafetyLevel::Blocked(r) | PathSafetyLevel::Important(r) => {
                    return PathSafetyLevel::Important(format!(
                        "Verknüpfung/Junction verweist auf geschütztes Ziel ({}) – nur die Verknüpfung wird gelöscht",
                        r
                    ));
                }
                PathSafetyLevel::Safe => {}
            }
        }
        return own_level;
    }

    // 3. Für normale Dateien/Ordner zusätzlich aufgelösten Pfad prüfen (z. B. subst-Laufwerke / Mount-Points)
    if let Ok(canon) = path.canonicalize() {
        let canon_norm = normalize_win_path_for_compare(&canon, false);
        let canon_level = classify_normalized_path(&canon_norm);
        match (&own_level, &canon_level) {
            (_, PathSafetyLevel::Blocked(_)) => return canon_level,
            (PathSafetyLevel::Safe, PathSafetyLevel::Important(_)) => return canon_level,
            _ => {}
        }
    }

    own_level
}

fn is_dangerous_or_root_path(path: &Path) -> bool {
    matches!(classify_path_safety(path), PathSafetyLevel::Blocked(_))
}

pub fn shred_file(path: &Path, num_passes: usize) -> io::Result<()> {
    shred_file_with_reporter(path, num_passes, &mut NoopShredReporter)
}

fn compute_non_resident_wipe_len(path: &Path, file: &fs::File, len: u64) -> u64 {
    // Ermittle reale physische Cluster-Belegung (AllocationSize)
    let mut std_info = FILE_STANDARD_INFO {
        allocation_size: 0,
        end_of_file: 0,
        number_of_links: 0,
        delete_pending: 0,
        directory: 0,
    };
    let ret = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FILE_STANDARD_INFO_CLASS,
            &mut std_info as *mut _ as *mut c_void,
            std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
        )
    };
    if ret != 0 && std_info.allocation_size > 0 {
        std_info.allocation_size as u64
    } else {
        let cluster_size = get_cluster_size(path);
        let remainder = len % cluster_size;
        if remainder != 0 {
            len + (cluster_size - remainder)
        } else {
            len
        }
    }
}

fn estimate_file_wipe_bytes(path: &Path, logical_len: u64, num_passes: usize, is_ssd: bool) -> u64 {
    if logical_len == 0 {
        return 0;
    }
    if let Ok(file) = OpenOptions::new().read(true).open(path) {
        let is_resident = is_file_resident_in_mft(&file, logical_len);
        let effective_passes = if is_ssd || is_resident {
            1
        } else {
            num_passes.max(1)
        };
        if !is_resident && is_sparse_file(path) {
            if let Ok(ranges) = query_allocated_ranges(&file, logical_len) {
                let range_bytes: u64 = ranges.iter().map(|&(_, l)| l).sum();
                return range_bytes * effective_passes as u64;
            }
        }
        let wipe_len = if is_resident {
            logical_len
        } else {
            compute_non_resident_wipe_len(path, &file, logical_len)
        };
        return wipe_len * effective_passes as u64;
    }
    let effective_passes = if is_ssd { 1 } else { num_passes.max(1) };
    logical_len * effective_passes as u64
}

pub fn shred_file_with_reporter<R: ShredReporter>(
    path: &Path,
    num_passes: usize,
    reporter: &mut R,
) -> io::Result<()> {
    if reporter.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Vorgang durch Benutzer abgebrochen",
        ));
    }
    reporter.on_item_start(path, false);

    // Zuerst Schreibzugriff auf die Hauptdatei absichern, bevor Attribute oder ADS verändert werden
    let mut ro_guard = ReadonlyGuard::acquire(path);
    let mut file = OpenOptions::new().write(true).read(true).open(path)?;

    let media_type = get_drive_media_type(path);
    let is_ssd = matches!(media_type, DriveMediaType::SsdNvme { .. });

    // Alternate Data Streams erst nach erfolgreichem Öffnen der Hauptdatei schreddern
    let _ = shred_alternate_streams(path, num_passes, is_ssd);

    let len = file.metadata()?.len();
    if len > 0 {
        let is_resident = is_file_resident_in_mft(&file, len);
        let sparse_ranges = if !is_resident && is_sparse_file(path) {
            query_allocated_ranges(&file, len).ok()
        } else {
            None
        };

        if let Some(ranges) = sparse_ranges {
            wipe_open_ranges_with_reporter(
                &mut file,
                &ranges,
                len,
                num_passes,
                is_ssd,
                false,
                reporter,
            )?;
        } else {
            let wipe_len = if is_resident {
                // KRITISCH: Residente Dateien NIEMALS vergroessern!
                // Exakt die Nutzdatenlaenge in-place ueberschreiben, damit keine Cluster ausgelagert werden.
                len
            } else {
                compute_non_resident_wipe_len(path, &file, len)
            };

            wipe_open_file_with_reporter(
                &mut file,
                wipe_len,
                num_passes,
                is_ssd,
                is_resident,
                reporter,
            )?;
        }
    } else {
        neutralize_timestamps(&file);
    }

    drop(file);

    if reporter.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Vorgang durch Benutzer abgebrochen",
        ));
    }

    let final_path = secure_single_rename(path)?;
    ro_guard.disarm();
    let mut final_ro = ReadonlyGuard::acquire(&final_path);
    fs::remove_file(&final_path)?;
    final_ro.disarm();

    // Explorer über die Löschung benachrichtigen
    let wide_deleted = path_to_wide(&final_path);
    unsafe {
        SHChangeNotify(
            SHCNE_ALLEVENTS,
            SHCNF_PATHW | SHCNF_FLUSH,
            wide_deleted.as_ptr() as *const c_void,
            std::ptr::null(),
        );
    }

    reporter.on_item_done(path, false);
    Ok(())
}

pub fn shred_recursive(path: &Path, num_passes: usize, errors: &mut usize) {
    shred_recursive_with_reporter(path, num_passes, errors, &mut NoopShredReporter);
}

pub fn shred_recursive_with_reporter<R: ShredReporter>(
    path: &Path,
    num_passes: usize,
    errors: &mut usize,
    reporter: &mut R,
) {
    if reporter.is_cancelled() {
        return;
    }

    let clean_path = strip_trailing_separators(path);

    if is_dangerous_or_root_path(&clean_path) {
        *errors += 1;
        eprintln!(
            "Schutzfunktion aktiv: System- oder Stammverzeichnis wird nicht geschreddert: {:?}",
            clean_path
        );
        return;
    }

    let sym_meta = match fs::symlink_metadata(&clean_path) {
        Ok(m) => m,
        Err(e) => {
            *errors += 1;
            eprintln!("Fehler beim Lesen der Metadaten für {:?}: {}", clean_path, e);
            return;
        }
    };

    // KRITISCH: Symlinks und Junctions NIEMALS betreten oder das Ziel überschreiben!
    if sym_meta.file_type().is_symlink() {
        reporter.on_item_start(&clean_path, false);
        match secure_single_rename(&clean_path) {
            Ok(final_path) => {
                let mut ro_guard = ReadonlyGuard::acquire(&final_path);
                if fs::remove_file(&final_path).is_err() && fs::remove_dir(&final_path).is_err() {
                    *errors += 1;
                } else {
                    ro_guard.disarm();
                    let wide_deleted = path_to_wide(&final_path);
                    unsafe {
                        SHChangeNotify(
                            SHCNE_ALLEVENTS,
                            SHCNF_PATHW | SHCNF_FLUSH,
                            wide_deleted.as_ptr() as *const c_void,
                            std::ptr::null(),
                        );
                    }
                }
            }
            Err(e) => {
                *errors += 1;
                eprintln!("Fehler beim Umbenennen der Verknüpfung {:?}: {}", clean_path, e);
            }
        }
        reporter.on_item_done(&clean_path, false);
        return;
    }

    if sym_meta.is_dir() {
        reporter.on_item_start(&clean_path, true);
        match fs::read_dir(&clean_path) {
            Ok(entries) => {
                for entry in entries {
                    if reporter.is_cancelled() {
                        return;
                    }
                    match entry {
                        Ok(e) => {
                            shred_recursive_with_reporter(&e.path(), num_passes, errors, reporter)
                        }
                        Err(_) => *errors += 1,
                    }
                }
            }
            Err(e) => {
                *errors += 1;
                eprintln!(
                    "Fehler beim Lesen des Verzeichnisses {:?}: {}",
                    clean_path, e
                );
            }
        }

        if reporter.is_cancelled() {
            return;
        }

        // Eventuelle Alternate Data Streams des Verzeichnisses schreddern
        let is_ssd = matches!(
            get_drive_media_type(&clean_path),
            DriveMediaType::SsdNvme { .. }
        );
        let _ = shred_alternate_streams(&clean_path, num_passes, is_ssd);

        let mut ro_guard = ReadonlyGuard::acquire(&clean_path);
        neutralize_dir_timestamps(&clean_path);
        match secure_single_rename(&clean_path) {
            Ok(final_dir_path) => {
                ro_guard.disarm();
                let mut final_ro = ReadonlyGuard::acquire(&final_dir_path);
                if let Err(e) = fs::remove_dir(&final_dir_path) {
                    *errors += 1;
                    eprintln!(
                        "Fehler beim Entfernen des Verzeichnisses {:?}: {}",
                        final_dir_path, e
                    );
                } else {
                    final_ro.disarm();
                    let wide_deleted = path_to_wide(&final_dir_path);
                    unsafe {
                        SHChangeNotify(
                            SHCNE_ALLEVENTS,
                            SHCNF_PATHW | SHCNF_FLUSH,
                            wide_deleted.as_ptr() as *const c_void,
                            std::ptr::null(),
                        );
                    }
                }
            }
            Err(e) => {
                *errors += 1;
                eprintln!(
                    "Fehler beim Umbenennen des Verzeichnisses {:?}: {}",
                    clean_path, e
                );
            }
        }
        reporter.on_item_done(&clean_path, true);
    } else if let Err(e) = shred_file_with_reporter(&clean_path, num_passes, reporter)
        && e.kind() != io::ErrorKind::Interrupted
    {
        *errors += 1;
        eprintln!("Fehler beim Schreddern der Datei {:?}: {}", clean_path, e);
    }
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
    let output = Command::new("tasklist")
        .creation_flags(0x08000000)
        .args(["/fi", "imagename eq explorer.exe"])
        .output();

    let already_running = output
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("explorer.exe"))
        .unwrap_or(false);

    if !already_running {
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
}

static PENDING_SELF_DELETE: AtomicBool = AtomicBool::new(false);

fn reg_key_exists(key: &str) -> bool {
    Command::new("reg")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["query", key])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_classic_menu_installed() -> bool {
    reg_key_exists(r"HKLM\SOFTWARE\Classes\*\shell\WinShred")
        || reg_key_exists(r"HKCU\Software\Classes\*\shell\WinShred")
}

fn is_sendto_installed() -> bool {
    env::var("APPDATA")
        .map(|appdata| {
            PathBuf::from(appdata)
                .join(r"Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk")
                .exists()
        })
        .unwrap_or(false)
}

fn is_modern_package_installed() -> bool {
    Command::new("powershell")
        .creation_flags(0x08000000)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "if (Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_reg(args: &[&str]) -> Result<(), String> {
    let output = Command::new("reg")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(args)
        .output()
        .map_err(|e| format!("reg konnte nicht gestartet werden ({:?}): {}", args, e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("Registrierungsbefehl {:?} fehlgeschlagen: {}", args, detail));
    }
    Ok(())
}

fn delete_reg_key_if_exists(key: &str) -> Result<(), String> {
    if reg_key_exists(key) {
        run_reg(&["delete", key, "/f"])?;
    }
    Ok(())
}

fn schedule_self_delete_on_exit() {
    if !PENDING_SELF_DELETE.swap(false, Ordering::SeqCst) {
        return;
    }
    let install_dir = PathBuf::from(r"C:\Program Files\WinShred");
    let exe_path = install_dir.join("winshred.exe");
    let cmd_cleanup = format!(
        "for /L %i in (1,1,30) do (del /f /q \"{}\" >nul 2>&1 & if not exist \"{}\" (rmdir \"{}\" >nul 2>&1 & exit) & ping -n 1 -w 300 127.0.0.1 >nul)",
        exe_path.to_string_lossy(),
        exe_path.to_string_lossy(),
        install_dir.to_string_lossy(),
    );
    let _ = Command::new("cmd")
        .current_dir(env::temp_dir())
        .creation_flags(0x08000000)
        .args(["/c", &cmd_cleanup])
        .spawn();
}

fn stop_shell_hosts() {
    let _ = Command::new("powershell")
        .creation_flags(0x08000000)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue; \
             Get-CimInstance Win32_Process -Filter \"Name = 'dllhost.exe'\" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like '*8a76b81c-0a87-4348-9f7c-79f8815e7c81*' } | Stop-Process -Force -ErrorAction SilentlyContinue;",
        ])
        .output();
}

fn run_install_backend(opt_modern: bool, opt_classic: bool, opt_sendto: bool) -> Result<String, String> {
    let install_dir = PathBuf::from("C:\\Program Files\\WinShred");
    if let Err(e) = fs::create_dir_all(&install_dir) {
        return Err(format!("Fehler beim Erstellen des Installationsordners:\n{}", e));
    }

    // 1. Zuerst Explorer und verwaiste WinShred-Surrogate stoppen, damit DLL nicht schreibgeschützt gesperrt ist
    stop_shell_hosts();

    // Alles Weitere in einem Hilfsblock ausführen, damit Explorer am Ende garantiert wieder gestartet wird
    let install_result: Result<String, String> = (|| {
        // 2. Eigene EXE nach Program Files kopieren (mit kurzem Retry falls Ziel noch freigegeben wird)
        let target_exe = install_dir.join("winshred.exe");
        if let Ok(current_exe) = env::current_exe() {
            let same_exe = current_exe.to_string_lossy().to_ascii_lowercase()
                == target_exe.to_string_lossy().to_ascii_lowercase();
            if !same_exe {
                let mut copy_err = None;
                for _ in 0..5 {
                    match fs::copy(&current_exe, &target_exe) {
                        Ok(_) => {
                            copy_err = None;
                            break;
                        }
                        Err(e) => {
                            copy_err = Some(e);
                            std::thread::sleep(std::time::Duration::from_millis(150));
                        }
                    }
                }
                if let Some(e) = copy_err {
                    return Err(format!("Fehler beim Kopieren von winshred.exe:\n{}", e));
                }
            }
        }

        // 3. Eingebettete Shell-DLL, MSIX, Zertifikat und Icon ablegen (mit Retry bei sperrenden Handles)
        let dll_path = install_dir.join("winshred_shell.dll");
        let msix_path = install_dir.join("winshred.msix");
        let cer_path = install_dir.join("winshred.cer");
        let ico_path = install_dir.join("winshred.ico");
        let lic_path = install_dir.join("LICENSE.txt");

        let mut dll_err = None;
        for _ in 0..8 {
            match fs::write(&dll_path, SHELL_DLL_BYTES) {
                Ok(_) => {
                    dll_err = None;
                    break;
                }
                Err(e) => {
                    dll_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
        }
        if let Some(e) = dll_err {
            return Err(format!("Fehler beim Schreiben von winshred_shell.dll:\n{}", e));
        }

        fs::write(&msix_path, MSIX_BYTES)
            .map_err(|e| format!("Fehler beim Schreiben von winshred.msix:\n{}", e))?;
        fs::write(&cer_path, CER_BYTES)
            .map_err(|e| format!("Fehler beim Schreiben von winshred.cer:\n{}", e))?;
        fs::write(&ico_path, ICO_BYTES)
            .map_err(|e| format!("Fehler beim Schreiben von winshred.ico:\n{}", e))?;
        fs::write(&lic_path, LICENSE_TEXT)
            .map_err(|e| format!("Fehler beim Schreiben von LICENSE.txt:\n{}", e))?;

        // 4. Modernes Kontextmenue (Sparse Package) je nach Auswahl
        let mut ps_command = String::from("$ProgressPreference = 'SilentlyContinue'; ");
        if opt_modern {
            ps_command.push_str(&format!(
                "Import-Certificate -FilePath '{cer}' -CertStoreLocation 'Cert:\\LocalMachine\\TrustedPeople' -ErrorAction Stop | Out-Null; \
                $pkg = Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue; \
                if ($pkg) {{ Remove-AppxPackage -Package $pkg.PackageFullName -ErrorAction SilentlyContinue }}; \
                try {{ \
                    Add-AppxPackage -Path '{msix}' -ExternalLocation '{inst}' -ErrorAction Stop; \
                }} catch {{ \
                    [Console]::Error.WriteLine($_.Exception.Message); \
                }} \
                $verify = Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue; \
                if (-not $verify) {{ [Console]::Error.WriteLine('Windows 11 Sparse-Package konnte nicht registriert werden. Stellen Sie sicher, dass Sideloading/Entwicklermodus erlaubt ist.'); }} ",
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
                "try {{ \
                    $wsh = New-Object -ComObject WScript.Shell; \
                    $s = $wsh.CreateShortcut(\"$env:APPDATA\\Microsoft\\Windows\\SendTo\\WinShred (Sicher vernichten).lnk\"); \
                    $s.TargetPath = '{inst}\\winshred.exe'; \
                    $s.IconLocation = '{ico}'; \
                    $s.Save(); \
                }} catch {{ [Console]::Error.WriteLine($_.Exception.Message); }} ",
                inst = install_dir.to_string_lossy(),
                ico = ico_path.to_string_lossy()
            ));
        } else if let Ok(appdata) = env::var("APPDATA") {
            let lnk = PathBuf::from(appdata).join(r"Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk");
            if lnk.exists() {
                fs::remove_file(&lnk)
                    .map_err(|e| format!("Fehler beim Entfernen der SendTo-Verknuepfung:\n{}", e))?;
            }
        }

        let output = Command::new("powershell")
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_command])
            .output()
            .map_err(|e| format!("PowerShell konnte nicht gestartet werden:\n{}", e))?;

        let ps_err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !ps_err.is_empty() {
            return Err(ps_err);
        }
        if !output.status.success() {
            return Err(format!("PowerShell-Konfiguration fehlgeschlagen (Exit-Code {:?}).", output.status.code()));
        }

        // 6. Klassisches Menue (statische Registry-Keys)
        let ico_str = ico_path.to_string_lossy().to_string();
        let cmd_str = format!("\"{}\" \"%1\"", target_exe.to_string_lossy());

        if opt_classic {
            run_reg(&["add", r"HKLM\SOFTWARE\Classes\*\shell\WinShred", "/ve", "/d", "Mit WinShred sicher vernichten", "/f"])?;
            run_reg(&["add", r"HKLM\SOFTWARE\Classes\*\shell\WinShred", "/v", "Icon", "/d", &ico_str, "/f"])?;
            run_reg(&["add", r"HKLM\SOFTWARE\Classes\*\shell\WinShred\command", "/ve", "/d", &cmd_str, "/f"])?;

            run_reg(&["add", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred", "/ve", "/d", "Mit WinShred sicher vernichten", "/f"])?;
            run_reg(&["add", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred", "/v", "Icon", "/d", &ico_str, "/f"])?;
            run_reg(&["add", r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred\command", "/ve", "/d", &cmd_str, "/f"])?;
        } else {
            delete_reg_key_if_exists(r"HKLM\SOFTWARE\Classes\*\shell\WinShred")?;
            delete_reg_key_if_exists(r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred")?;
            delete_reg_key_if_exists(r"HKCU\Software\Classes\*\shell\WinShred")?;
            delete_reg_key_if_exists(r"HKCU\Software\Classes\Directory\shell\WinShred")?;
        }

        // 7. Verifikation
        if !target_exe.exists() || !dll_path.exists() {
            return Err(String::from("Installationsdateien sind nach dem Kopieren nicht vorhanden."));
        }

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
        msg.push_str("[OK] HDD: 3-Pass DoD 5220.22-M / SSD & NVMe: 1-Pass + TRIM\n");
        msg.push_str("[OK] Cluster-Slack-Bereinigung & Alternate Data Streams (ADS)\n");
        msg.push_str("[OK] MFT-Dateinamen-Anonymisierung im NTFS-Dateisystem\n");
        msg.push_str("[OK] Schutz vor Symlink-/Junction-Traversierung & Systemverzeichnissen");

        Ok(msg)
    })();

    // 8. Explorer sauber und entkoppelt über die Windows Shell neu starten (auch im Fehlerfall)
    restart_explorer();

    install_result
}

fn run_uninstall_backend() -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();

    // 1. PowerShell: Explorer & dllhost stoppen, AppX-Paket und Zertifikat löschen
    let ps_command = "\
        $ProgressPreference = 'SilentlyContinue'; \
        Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue; \
        Get-CimInstance Win32_Process -Filter \"Name = 'dllhost.exe'\" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like '*8a76b81c-0a87-4348-9f7c-79f8815e7c81*' } | Stop-Process -Force -ErrorAction SilentlyContinue; \
        $pkg = Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue; \
        if ($pkg) { Remove-AppxPackage -Package $pkg.PackageFullName -ErrorAction Stop }; \
        Get-ChildItem Cert:\\LocalMachine\\TrustedPeople -ErrorAction SilentlyContinue | Where-Object { $_.Subject -match 'CN=WinShred' } | Remove-Item -Force -ErrorAction Stop;";

    match Command::new("powershell")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps_command])
        .output()
    {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if !stderr.is_empty() {
                errors.push(stderr);
            } else if !out.status.success() {
                errors.push(format!("AppX/Zertifikat-Entfernung fehlgeschlagen (Exit {:?}).", out.status.code()));
            }
        }
        Err(e) => errors.push(format!("PowerShell konnte nicht gestartet werden: {}", e)),
    }

    // 2. Registry säubern
    for key in [
        r"HKLM\SOFTWARE\Classes\*\shell\WinShred",
        r"HKLM\SOFTWARE\Classes\Directory\shell\WinShred",
        r"HKCU\Software\Classes\*\shell\WinShred",
        r"HKCU\Software\Classes\Directory\shell\WinShred",
        r"HKCR\*\shell\WinShred",
        r"HKCR\Directory\shell\WinShred",
        r"HKCU\Software\WinShred",
    ] {
        if let Err(e) = delete_reg_key_if_exists(key) {
            errors.push(e);
        }
    }

    // 3. SendTo-Verknüpfung löschen
    if let Ok(appdata) = env::var("APPDATA") {
        let lnk = PathBuf::from(appdata).join(r"Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk");
        if lnk.exists() {
            if let Err(e) = fs::remove_file(&lnk) {
                errors.push(format!("SendTo-Verknuepfung konnte nicht geloescht werden: {}", e));
            }
        }
    }

    // 4. Dateien aus Program Files entfernen BEVOR Explorer neu gestartet wird
    // (verhindert, dass ein neu startender Explorer sofort Handles auf winshred_shell.dll zieht)
    let install_dir = PathBuf::from("C:\\Program Files\\WinShred");
    let dll_path = install_dir.join("winshred_shell.dll");
    if dll_path.exists() {
        let mut dll_del_err = None;
        for _ in 0..8 {
            match fs::remove_file(&dll_path) {
                Ok(_) => {
                    dll_del_err = None;
                    break;
                }
                Err(e) => {
                    dll_del_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
        }
        if let Some(e) = dll_del_err {
            errors.push(format!("winshred_shell.dll konnte nicht geloescht werden: {}", e));
        }
    }

    for fname in ["winshred.msix", "winshred.cer", "winshred.ico", "LICENSE.txt"] {
        let p = install_dir.join(fname);
        if p.exists() {
            if let Err(e) = fs::remove_file(&p) {
                errors.push(format!("{} konnte nicht geloescht werden: {}", fname, e));
            }
        }
    }

    if let Ok(current_exe) = env::current_exe() {
        let in_install_dir = current_exe
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(&install_dir.to_string_lossy().to_ascii_lowercase());
        if !in_install_dir {
            let exe_path = install_dir.join("winshred.exe");
            if exe_path.exists() {
                if let Err(e) = fs::remove_file(&exe_path) {
                    errors.push(format!("winshred.exe konnte nicht geloescht werden: {}", e));
                }
            }
            if install_dir.exists() {
                let _ = fs::remove_dir(&install_dir);
            }
        } else {
            // Selbstlöschung erst nach sauberem Beenden des Prozesses anstoßen
            PENDING_SELF_DELETE.store(true, Ordering::SeqCst);
        }
    }

    // 5. Explorer sauber neu starten (auch bei Teilfehlern, damit der Desktop nutzbar bleibt)
    restart_explorer();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

// ==========================================
// Windows Installer Wizard (Wizard97 Layout)
// ==========================================

const ID_BTN_BACK: usize = 100;
const ID_BTN_INSTALL: usize = 101;
const ID_BTN_NEXT: usize = 102;
const ID_BTN_CANCEL: usize = 103;
const ID_BTN_FINISH: usize = 104;

const ID_RADIO_REPAIR: usize = 201;
const ID_RADIO_UNINSTALL: usize = 202;
const ID_RAD_LIC_ACCEPT: usize = 205;
const ID_RAD_LIC_DECLINE: usize = 206;

const ID_CHK_MODERN: usize = 210;
const ID_CHK_CLASSIC: usize = 211;
const ID_CHK_SENDTO: usize = 212;

const ID_PROGRESS_BAR: usize = 301;
const ID_BTN_PROG_CANCEL: usize = 1001;
const ID_SHRED_PROGRESS_BAR: usize = 1002;

const WM_APP_SETUP_DONE: u32 = 0x8001;
const WM_APP_PROGRESS_UPDATE: u32 = 0x8010;
const WM_APP_SHRED_DONE: u32 = 0x8011;

const WM_CTLCOLORSTATIC: u32 = 0x0138;
const PBM_SETPOS: u32 = 0x0402;
const PBM_SETRANGE32: u32 = 0x0406;
const PBM_SETMARQUEE: u32 = 0x040A;
const PBS_SMOOTH: u32 = 0x0001;
const PBS_MARQUEE: u32 = 0x0008;
const BM_SETCHECK: u32 = 0x00F1;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETSTYLE: u32 = 0x00F4;
const DM_SETDEFID: u32 = 0x0401;
const BST_CHECKED: usize = 1;
const BS_AUTORADIOBUTTON: u32 = 0x00000009;
const BS_AUTOCHECKBOX: u32 = 0x00000003;
const BS_GROUPBOX: u32 = 0x00000007;
const BS_DEFPUSHBUTTON: u32 = 0x00000001;
const BS_PUSHBUTTON: u32 = 0x00000000;
const ES_MULTILINE: u32 = 0x0004;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_READONLY: u32 = 0x0800;
const ES_AUTOHSCROLL: u32 = 0x0080;
const WS_CHILD: u32 = 0x40000000;
const WS_VISIBLE: u32 = 0x10000000;
const WS_TABSTOP: u32 = 0x00010000;
const WS_BORDER: u32 = 0x00800000;
const WS_VSCROLL: u32 = 0x00200000;
const WS_GROUP: u32 = 0x00020000;
const WS_DISABLED: u32 = 0x08000000;
const WM_SETFONT: u32 = 0x0030;
const WM_COMMAND: u32 = 0x0111;
const WM_TIMER: u32 = 0x0113;
const WM_PAINT: u32 = 0x000F;
const WM_DESTROY: u32 = 0x0002;
const WM_CLOSE: u32 = 0x0010;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WizardPage {
    License,
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
    lbl_lic_prompt: *mut c_void,
    edt_lic_text: *mut c_void,
    rad_lic_accept: *mut c_void,
    rad_lic_decline: *mut c_void,

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

    btn_back: *mut c_void,
    btn_install: *mut c_void,
    btn_next: *mut c_void,
    btn_cancel: *mut c_void,
    btn_finish: *mut c_void,
}

static mut CTRLS: WizardControls = WizardControls {
    lbl_lic_prompt: std::ptr::null_mut(),
    edt_lic_text: std::ptr::null_mut(),
    rad_lic_accept: std::ptr::null_mut(),
    rad_lic_decline: std::ptr::null_mut(),

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

    btn_back: std::ptr::null_mut(),
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

unsafe fn set_lic_visible(visible: bool) {
    unsafe {
        show_ctrl(CTRLS.lbl_lic_prompt, visible);
        show_ctrl(CTRLS.edt_lic_text, visible);
        show_ctrl(CTRLS.rad_lic_accept, visible);
        show_ctrl(CTRLS.rad_lic_decline, visible);
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
            WizardPage::License => {
                set_lic_visible(true);
                set_p1_install_visible(false);
                set_p1_manage_visible(false);
                set_p2_progress_visible(false);
                set_p3_finish_visible(false);

                show_ctrl(CTRLS.btn_back, true);
                EnableWindow(CTRLS.btn_back, 0);
                show_ctrl(CTRLS.btn_install, false);
                show_ctrl(CTRLS.btn_next, true);

                let accepted = SendMessageW(CTRLS.rad_lic_accept, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                EnableWindow(CTRLS.btn_next, if accepted { 1 } else { 0 });

                show_ctrl(CTRLS.btn_cancel, true);
                EnableWindow(CTRLS.btn_cancel, 1);
                show_ctrl(CTRLS.btn_finish, false);

                SendMessageW(
                    hwnd,
                    DM_SETDEFID,
                    if accepted { ID_BTN_NEXT } else { ID_BTN_CANCEL },
                    0,
                );
            }
            WizardPage::Config => {
                set_lic_visible(false);
                if IS_CURRENTLY_INSTALLED {
                    set_p1_install_visible(false);
                    set_p1_manage_visible(true);
                    show_ctrl(CTRLS.btn_back, false);
                    show_ctrl(CTRLS.btn_install, false);
                    show_ctrl(CTRLS.btn_next, true);
                    EnableWindow(CTRLS.btn_next, 1);
                    SendMessageW(hwnd, DM_SETDEFID, ID_BTN_NEXT, 0);
                } else {
                    set_p1_install_visible(true);
                    set_p1_manage_visible(false);
                    show_ctrl(CTRLS.btn_back, true);
                    EnableWindow(CTRLS.btn_back, 1);
                    show_ctrl(CTRLS.btn_install, true);
                    EnableWindow(CTRLS.btn_install, 1);
                    show_ctrl(CTRLS.btn_next, false);
                    SendMessageW(hwnd, DM_SETDEFID, ID_BTN_INSTALL, 0);
                }
                set_p2_progress_visible(false);
                set_p3_finish_visible(false);

                show_ctrl(CTRLS.btn_cancel, true);
                EnableWindow(CTRLS.btn_cancel, 1);
                show_ctrl(CTRLS.btn_finish, false);
            }
            WizardPage::Progress => {
                set_lic_visible(false);
                set_p1_install_visible(false);
                set_p1_manage_visible(false);
                set_p2_progress_visible(true);
                set_p3_finish_visible(false);

                show_ctrl(CTRLS.btn_back, true);
                EnableWindow(CTRLS.btn_back, 0);
                show_ctrl(CTRLS.btn_install, false);
                show_ctrl(CTRLS.btn_next, false);
                show_ctrl(CTRLS.btn_cancel, true);
                EnableWindow(CTRLS.btn_cancel, 0);
                show_ctrl(CTRLS.btn_finish, false);

                SendMessageW(hwnd, DM_SETDEFID, 0, 0);
                SendMessageW(CTRLS.bar_p2_progress, PBM_SETMARQUEE, 1, 30);
            }
            WizardPage::Finish => {
                set_lic_visible(false);
                set_p1_install_visible(false);
                set_p1_manage_visible(false);
                set_p2_progress_visible(false);
                set_p3_finish_visible(true);

                show_ctrl(CTRLS.btn_back, false);
                show_ctrl(CTRLS.btn_install, false);
                show_ctrl(CTRLS.btn_next, false);
                show_ctrl(CTRLS.btn_cancel, false);
                show_ctrl(CTRLS.btn_finish, true);
                SendMessageW(hwnd, DM_SETDEFID, ID_BTN_FINISH, 0);
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
                if id == ID_RAD_LIC_ACCEPT {
                    EnableWindow(CTRLS.btn_next, 1);
                    SendMessageW(hwnd, DM_SETDEFID, ID_BTN_NEXT, 0);
                } else if id == ID_RAD_LIC_DECLINE {
                    EnableWindow(CTRLS.btn_next, 0);
                    SendMessageW(hwnd, DM_SETDEFID, ID_BTN_CANCEL, 0);
                } else if id == ID_BTN_BACK {
                    if CURRENT_PAGE == WizardPage::Config && !IS_CURRENTLY_INSTALLED {
                        CURRENT_PAGE = WizardPage::License;
                        update_wizard_page(hwnd);
                    }
                } else if id == ID_RADIO_REPAIR {
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
                } else if id == ID_BTN_NEXT && CURRENT_PAGE == WizardPage::License {
                    CURRENT_PAGE = WizardPage::Config;
                    update_wizard_page(hwnd);
                } else if (id == ID_BTN_INSTALL && CURRENT_PAGE == WizardPage::Config && !IS_CURRENTLY_INSTALLED)
                    || (id == ID_BTN_NEXT && CURRENT_PAGE == WizardPage::Config && IS_CURRENTLY_INSTALLED)
                {
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
                                let cur_classic = is_classic_menu_installed();
                                let mut cur_modern = is_modern_package_installed();
                                let cur_sendto = is_sendto_installed();
                                if !cur_modern && !cur_classic {
                                    cur_modern = true;
                                }
                                (cur_modern, cur_classic, cur_sendto)
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
                } else if id == ID_BTN_FINISH
                    || id == ID_BTN_CANCEL
                    || (id == 2 /* IDCANCEL (Escape) */ && CURRENT_PAGE != WizardPage::Progress)
                {
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

                if hwnd_ctrl == CTRLS.edt_p1_target || hwnd_ctrl == CTRLS.edt_lic_text {
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

                // Header-Icon (48x48) rechts oben (LR_SHARED verhindert HICON-Handle-Leak beim Repaint)
                let h_mod = GetModuleHandleW(std::ptr::null());
                let h_icon = LoadImageW(h_mod, 1 as *const u16, 1 /* IMAGE_ICON */, 48, 48, LR_SHARED);
                if !h_icon.is_null() {
                    DrawIconEx(hdc, 520 - 48 - 18, 9, h_icon, 48, 48, 0, std::ptr::null_mut(), 3);
                }

                // Header-Texte
                let (header_title, header_sub) = match CURRENT_PAGE {
                    WizardPage::License => {
                        ("Lizenzvereinbarung", "Bitte lesen Sie die Lizenzbedingungen sorgfaeltig durch.")
                    }
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
                if CURRENT_PAGE == WizardPage::Progress {
                    return 0;
                }
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
        if IS_CURRENTLY_INSTALLED {
            CURRENT_PAGE = WizardPage::Config;
        } else {
            CURRENT_PAGE = WizardPage::License;
        }
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
        wnd_class.h_icon = LoadImageW(instance, 1 as *const u16, 1, 32, 32, LR_SHARED);
        wnd_class.h_icon_sm = LoadImageW(instance, 1 as *const u16, 1, 16, 16, LR_SHARED);
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

        // --- Seite 0 (Lizenzvereinbarung) ---
        CTRLS.lbl_lic_prompt = create_ctrl(
            0, "STATIC", "Bitte lesen Sie die folgenden Lizenzbedingungen sorgfaeltig durch:",
            WS_CHILD, 28, 76, 464, 18, hwnd, 0, H_FONT_REGULAR,
        );
        CTRLS.edt_lic_text = create_ctrl(
            0, "EDIT", "",
            WS_CHILD | WS_BORDER | WS_VSCROLL | ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_TABSTOP,
            28, 98, 464, 180, hwnd, 0, H_FONT_REGULAR,
        );
        let lic_crlf = LICENSE_TEXT.replace("\r\n", "\n").replace('\n', "\r\n");
        let w_lic = to_wide_chars(&lic_crlf);
        SendMessageW(CTRLS.edt_lic_text, 0x000C /* WM_SETTEXT */, 0, w_lic.as_ptr() as isize);

        CTRLS.rad_lic_accept = create_ctrl(
            0, "BUTTON", "Ich akzeptiere die Lizenzvereinbarung",
            WS_CHILD | BS_AUTORADIOBUTTON | WS_GROUP | WS_TABSTOP, 28, 284, 464, 20, hwnd, ID_RAD_LIC_ACCEPT, H_FONT_REGULAR,
        );
        CTRLS.rad_lic_decline = create_ctrl(
            0, "BUTTON", "Ich lehne die Lizenzvereinbarung ab",
            WS_CHILD | BS_AUTORADIOBUTTON | WS_TABSTOP, 28, 306, 464, 20, hwnd, ID_RAD_LIC_DECLINE, H_FONT_REGULAR,
        );
        SendMessageW(CTRLS.rad_lic_decline, BM_SETCHECK, BST_CHECKED, 0);

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
            0, "BUTTON", "HDD: 3-Pass DoD 5220.22-M / SSD & NVMe: 1-Pass + TRIM",
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
        CTRLS.btn_back = create_ctrl(
            0, "BUTTON", "< Zurueck",
            WS_CHILD | BS_PUSHBUTTON | WS_TABSTOP, 200, 348, 88, 28, hwnd, ID_BTN_BACK, H_FONT_REGULAR,
        );
        CTRLS.btn_install = create_ctrl(
            0, "BUTTON", "Installieren",
            WS_CHILD | BS_DEFPUSHBUTTON | WS_TABSTOP, 294, 348, 100, 28, hwnd, ID_BTN_INSTALL, H_FONT_REGULAR,
        );
        CTRLS.btn_next = create_ctrl(
            0, "BUTTON", "Weiter >",
            WS_CHILD | BS_DEFPUSHBUTTON | WS_TABSTOP, 294, 348, 100, 28, hwnd, ID_BTN_NEXT, H_FONT_REGULAR,
        );
        CTRLS.btn_cancel = create_ctrl(
            0, "BUTTON", "Abbrechen",
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP, 402, 348, 92, 28, hwnd, ID_BTN_CANCEL, H_FONT_REGULAR,
        );
        CTRLS.btn_finish = create_ctrl(
            0, "BUTTON", "Fertigstellen",
            WS_CHILD | BS_DEFPUSHBUTTON | WS_TABSTOP, 394, 348, 100, 28, hwnd, ID_BTN_FINISH, H_FONT_REGULAR,
        );

        // Erste Seite initialisieren
        update_wizard_page(hwnd);

        force_foreground_window_with_topmost(hwnd, false);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Aufraeumen (inkl. Zurücksetzen der Handles, damit init_gui_resources danach neu erzeugen kann)
        DeleteObject(H_BRUSH_WHITE);
        DeleteObject(H_BRUSH_DIALOG);
        DeleteObject(H_BRUSH_LINE);
        DeleteObject(H_FONT_TITLE);
        DeleteObject(H_FONT_SUBTITLE);
        DeleteObject(H_FONT_BOLD);
        DeleteObject(H_FONT_REGULAR);
        DeleteObject(H_FONT_SUCCESS);
        H_BRUSH_WHITE = std::ptr::null_mut();
        H_BRUSH_DIALOG = std::ptr::null_mut();
        H_BRUSH_LINE = std::ptr::null_mut();
        H_FONT_TITLE = std::ptr::null_mut();
        H_FONT_SUBTITLE = std::ptr::null_mut();
        H_FONT_BOLD = std::ptr::null_mut();
        H_FONT_REGULAR = std::ptr::null_mut();
        H_FONT_SUCCESS = std::ptr::null_mut();
    }
}

// ==========================================
// Persistente Benutzereinstellungen (Registry)
// ==========================================

const REG_SETTINGS_SUBKEY: &str = r"Software\WinShred";
const REG_VAL_SKIP_PROMPT: &str = "SkipSafetyPrompt";

/// Liest die Einstellung „Sicherheitsprüfung in Zukunft überspringen“ aus `HKCU\Software\WinShred`.
pub fn get_skip_safety_prompt() -> bool {
    let subkey = to_wide_chars(REG_SETTINGS_SUBKEY);
    let val_name = to_wide_chars(REG_VAL_SKIP_PROMPT);
    let mut hkey: *mut c_void = std::ptr::null_mut();

    let res = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if res != 0 || hkey.is_null() {
        return false;
    }

    let mut data: u32 = 0;
    let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
    let mut val_type: u32 = 0;
    let qres = unsafe {
        RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            &mut data as *mut u32 as *mut u8,
            &mut data_size,
        )
    };
    unsafe {
        RegCloseKey(hkey);
    }

    qres == 0 && val_type == REG_DWORD && data != 0
}

/// Speichert die Einstellung „Sicherheitsprüfung in Zukunft überspringen“ in `HKCU\Software\WinShred`.
pub fn set_skip_safety_prompt(skip: bool) {
    let subkey = to_wide_chars(REG_SETTINGS_SUBKEY);
    let val_name = to_wide_chars(REG_VAL_SKIP_PROMPT);
    let mut hkey: *mut c_void = std::ptr::null_mut();
    let mut disp: u32 = 0;

    let res = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            KEY_WRITE,
            std::ptr::null_mut(),
            &mut hkey,
            &mut disp,
        )
    };
    if res != 0 || hkey.is_null() {
        return;
    }

    if skip {
        let data: u32 = 1;
        unsafe {
            RegSetValueExW(
                hkey,
                val_name.as_ptr(),
                0,
                REG_DWORD,
                &data as *const u32 as *const u8,
                std::mem::size_of::<u32>() as u32,
            );
        }
    } else {
        unsafe {
            RegDeleteValueW(hkey, val_name.as_ptr());
        }
    }

    unsafe {
        RegCloseKey(hkey);
    }
}

/// Zitiert ein einzelnes Argument nach Windows-`CommandLineToArgvW`-Regeln (inkl. Trailing-Backslashes).
pub fn quote_windows_cli_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '"')) {
        return arg.to_string();
    }

    let mut out = String::with_capacity(arg.len() + 4);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        if ch == '\\' {
            backslashes += 1;
        } else if ch == '"' {
            for _ in 0..(backslashes * 2 + 1) {
                out.push('\\');
            }
            out.push('"');
            backslashes = 0;
        } else {
            for _ in 0..backslashes {
                out.push('\\');
            }
            backslashes = 0;
            out.push(ch);
        }
    }
    for _ in 0..(backslashes * 2) {
        out.push('\\');
    }
    out.push('"');
    out
}

// ==========================================
// Sicherheitsbestätigungs-Dialog mit Admin-Option
// ==========================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationResult {
    Cancel,
    ShredStandard,
    ShredAsAdmin,
}

const ID_CHK_SKIP_PROMPT: usize = 104;

static mut CONFIRMATION_CHOICE: ConfirmationResult = ConfirmationResult::Cancel;
static mut CONFIRM_HDR_TITLE: *mut c_void = std::ptr::null_mut();
static mut CONFIRM_HDR_SUB: *mut c_void = std::ptr::null_mut();
static mut CONFIRM_LBL_IMPORTANT: *mut c_void = std::ptr::null_mut();
static mut CONFIRM_CHK_SKIP: *mut c_void = std::ptr::null_mut();
static mut CONFIRM_SKIP_CHECKED: bool = false;

unsafe extern "system" fn confirm_dialog_wnd_proc(
    hwnd: *mut c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = wparam & 0xFFFF;
                if id == 101 || id == 102 {
                    if !CONFIRM_CHK_SKIP.is_null() {
                        CONFIRM_SKIP_CHECKED =
                            SendMessageW(CONFIRM_CHK_SKIP, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                    }
                    CONFIRMATION_CHOICE = if id == 101 {
                        ConfirmationResult::ShredStandard
                    } else {
                        ConfirmationResult::ShredAsAdmin
                    };
                    DestroyWindow(hwnd);
                } else if id == 103 || id == 2 /* IDCANCEL */ {
                    CONFIRMATION_CHOICE = ConfirmationResult::Cancel;
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_CTLCOLORSTATIC => {
                let hdc = wparam as *mut c_void;
                let h_ctrl = lparam as *mut c_void;
                SetBkMode(hdc, 1 /* TRANSPARENT */);

                if h_ctrl == CONFIRM_HDR_TITLE || h_ctrl == CONFIRM_HDR_SUB {
                    SetBkColor(hdc, 0x00FFFFFF);
                    H_BRUSH_WHITE as isize
                } else {
                    if !CONFIRM_LBL_IMPORTANT.is_null() && h_ctrl == CONFIRM_LBL_IMPORTANT {
                        SetTextColor(hdc, 0x001414B4); // Rot
                    } else {
                        SetTextColor(hdc, 0x00000000);
                    }
                    SetBkColor(hdc, 0x00F0F0F0);
                    H_BRUSH_DIALOG as isize
                }
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);

                // 1. Header-Bereich (Weiss)
                let rc_top = RECT { left: 0, top: 0, right: 540, bottom: 85 };
                FillRect(hdc, &rc_top, H_BRUSH_WHITE);

                // 2. Trennlinie unter dem Header
                let rc_line = RECT { left: 0, top: 85, right: 540, bottom: 86 };
                FillRect(hdc, &rc_line, H_BRUSH_LINE);

                // 3. Unterer Bereich (Dialog-Grau)
                let rc_bottom = RECT { left: 0, top: 86, right: 540, bottom: 420 };
                FillRect(hdc, &rc_bottom, H_BRUSH_DIALOG);

                // 4. Logo / Icon im Header (LR_SHARED verhindert Handle-Leak)
                let h_inst = GetModuleHandleW(std::ptr::null());
                let h_icon = LoadImageW(h_inst, 1 as *const u16, 1, 32, 32, LR_SHARED);
                if !h_icon.is_null() {
                    DrawIconEx(hdc, 20, 24, h_icon, 32, 32, 0, std::ptr::null_mut(), 0x0003);
                }

                EndPaint(hwnd, &ps);
                0
            }
            WM_CLOSE => {
                CONFIRMATION_CHOICE = ConfirmationResult::Cancel;
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

fn init_gui_resources() {
    unsafe {
        if H_BRUSH_WHITE.is_null() {
            H_BRUSH_WHITE = CreateSolidBrush(0x00FFFFFF);
            H_BRUSH_DIALOG = CreateSolidBrush(0x00F0F0F0);
            H_BRUSH_LINE = CreateSolidBrush(0x00D8D8D8);

            let font_face = to_wide_chars("Segoe UI");
            H_FONT_TITLE = CreateFontW(18, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
            H_FONT_SUBTITLE = CreateFontW(13, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
            H_FONT_BOLD = CreateFontW(13, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
            H_FONT_REGULAR = CreateFontW(13, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
            H_FONT_SUCCESS = CreateFontW(16, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_face.as_ptr());
        }
    }
}

pub fn show_confirmation_dialog(
    paths: &[PathBuf],
    is_already_admin: bool,
    important_reasons: &[String],
    num_passes: usize,
) -> (ConfirmationResult, bool) {
    unsafe {
        CONFIRMATION_CHOICE = ConfirmationResult::Cancel;
        CONFIRM_LBL_IMPORTANT = std::ptr::null_mut();
        CONFIRM_CHK_SKIP = std::ptr::null_mut();
        CONFIRM_SKIP_CHECKED = get_skip_safety_prompt();
        init_gui_resources();

        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide_chars("WinShredConfirmDialogClass");

        let mut wnd_class: WNDCLASSEXW = std::mem::zeroed();
        wnd_class.cb_size = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wnd_class.style = 0x0002 | 0x0001;
        wnd_class.lpfn_wnd_proc = Some(confirm_dialog_wnd_proc);
        wnd_class.h_instance = instance;
        wnd_class.h_cursor = LoadCursorW(std::ptr::null_mut(), 32512 as *const u16);
        wnd_class.h_icon = LoadImageW(instance, 1 as *const u16, 1, 32, 32, LR_SHARED);
        wnd_class.h_icon_sm = LoadImageW(instance, 1 as *const u16, 1, 16, 16, LR_SHARED);
        wnd_class.lpsz_class_name = class_name.as_ptr();

        RegisterClassExW(&wnd_class);

        let has_important = !important_reasons.is_empty();
        let client_h = if has_important { 420 } else { 386 };

        // Fenstergroesse
        let mut rc = RECT { left: 0, top: 0, right: 540, bottom: client_h };
        let win_style = 0x00000000 | 0x00C00000 | 0x00080000; // WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU
        AdjustWindowRectEx(&mut rc, win_style, 0, 0);
        let win_w = rc.right - rc.left;
        let win_h = rc.bottom - rc.top;

        let screen_w = GetSystemMetrics(0);
        let screen_h = GetSystemMetrics(1);
        let pos_x = (screen_w - win_w) / 2;
        let pos_y = (screen_h - win_h) / 2;

        let window_title = to_wide_chars("WinShred - Sicherheitsbestätigung");

        let hwnd = CreateWindowExW(
            0x00040000, // WS_EX_TOPMOST
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
            return (ConfirmationResult::Cancel, CONFIRM_SKIP_CHECKED);
        }

        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 1 /* ICON_BIG */, wnd_class.h_icon as isize);
        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 0 /* ICON_SMALL */, wnd_class.h_icon_sm as isize);

        // Medientypen der Pfade analysieren
        let mut has_ssd = false;
        let mut has_hdd = false;
        for p in paths {
            match get_drive_media_type(p) {
                DriveMediaType::SsdNvme { .. } => has_ssd = true,
                DriveMediaType::RotationalHdd => has_hdd = true,
                DriveMediaType::Unknown => (),
            }
        }
        let hdd_owned_text;
        let media_text = if has_ssd && !has_hdd {
            "[SSD / NVMe erkannt] Flash-optimiertes 1-Pass Wiping + Hardware-TRIM aktiv"
        } else if has_hdd && !has_ssd {
            hdd_owned_text = if num_passes == 3 {
                "[HDD erkannt] 3-Pass DoD 5220.22-M (0x00, 0xFF, Krypto-Zufall)".to_string()
            } else if num_passes > 1 {
                format!("[HDD erkannt] {}-Pass Ueberschreibung (DoD / Krypto-Zufall)", num_passes)
            } else {
                "[HDD erkannt] 1-Pass Krypto-Zufall (NIST SP 800-88 / BSI konform)".to_string()
            };
            &hdd_owned_text
        } else if has_ssd && has_hdd {
            hdd_owned_text = if num_passes > 1 {
                format!("[SSD & HDD erkannt] SSD: 1-Pass + TRIM / HDD: {}-Pass aktiv", num_passes)
            } else {
                "[SSD & HDD erkannt] Medienspezifische Tilgung (TRIM / NIST) aktiv".to_string()
            };
            &hdd_owned_text
        } else {
            "[Speicher erkannt] Sichere adaptive Vernichtung aktiv"
        };

        let count = paths.len();
        let prompt_sub = if count == 1 {
            let name = paths[0].file_name().unwrap_or_default().to_string_lossy();
            let sym_meta = fs::symlink_metadata(&paths[0]);
            let is_link = sym_meta.as_ref().map(|m| m.file_type().is_symlink()).unwrap_or(false);
            let is_dir = sym_meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let typ = if is_link {
                "die Verknüpfung"
            } else if is_dir {
                "den Ordner"
            } else {
                "die Datei"
            };
            format!("Möchten Sie {} '{}' unwiderruflich vernichten?", typ, name)
        } else {
            format!("Möchten Sie diese {} Elemente unwiderruflich vernichten?", count)
        };

        // Header Labels (Weiss)
        let header_title = if is_already_admin {
            "Sicherheitsüberprüfung (Administrator-Modus)"
        } else {
            "Sicherheitsüberprüfung vor dem Vernichten"
        };
        CONFIRM_HDR_TITLE = create_ctrl(
            0, "STATIC", header_title,
            WS_CHILD | WS_VISIBLE, 65, 18, 455, 22, hwnd, 0, H_FONT_TITLE,
        );
        CONFIRM_HDR_SUB = create_ctrl(
            0, "STATIC", &prompt_sub,
            WS_CHILD | WS_VISIBLE, 65, 44, 455, 36, hwnd, 0, H_FONT_SUBTITLE,
        );

        // Detailbereich
        create_ctrl(
            0, "STATIC", media_text,
            WS_CHILD | WS_VISIBLE, 24, 98, 492, 20, hwnd, 0, H_FONT_BOLD,
        );
        create_ctrl(
            0, "STATIC", "• Resident In-Place MFT-Wipe: Kein Auslagern, keine MFT-Slack-Reste",
            WS_CHILD | WS_VISIBLE, 24, 122, 492, 18, hwnd, 0, H_FONT_REGULAR,
        );
        create_ctrl(
            0, "STATIC", "• Alternate Data Streams (ADS) & Zeitstempel-Neutralisierung (1601-01-01)",
            WS_CHILD | WS_VISIBLE, 24, 144, 492, 18, hwnd, 0, H_FONT_REGULAR,
        );
        create_ctrl(
            0, "STATIC", "WARNUNG: Dieser Vorgang kann nicht rückgängig gemacht werden!",
            WS_CHILD | WS_VISIBLE, 24, 168, 492, 18, hwnd, 0, H_FONT_BOLD,
        );

        // Infobox "Warum als Administrator?"
        create_ctrl(
            0, "BUTTON", "Option: Erweiterte Forensik & Journal-Bereinigung",
            WS_CHILD | WS_VISIBLE | BS_GROUPBOX, 18, 194, 504, 90, hwnd, 0, H_FONT_BOLD,
        );
        let info_desc = if is_already_admin {
            "Administrator-Modus aktiv: WinShred kann nach dem Schreddern das interne NTFS-Änderungsjournal ($UsnJrnl) zurücksetzen, um auch historische Dateinamen-Einträge auf den Laufwerken rückstandslos zu tilgen."
        } else {
            "Warum als Administrator? Im Standardmodus protokolliert Windows NTFS-Umbenennungen im internen USN-Journal ($UsnJrnl). Mit Administratorrechten kann WinShred dieses Journal leeren und jegliche Dateinamen-Spuren tilgen."
        };
        create_ctrl(
            0, "STATIC", info_desc,
            WS_CHILD | WS_VISIBLE, 28, 216, 484, 60, hwnd, 0, H_FONT_REGULAR,
        );

        let mut next_y = 292;
        if has_important {
            let reason_line = important_reasons
                .first()
                .map(|s| s.as_str())
                .unwrap_or("Relevanter System-, Programm- oder Profilordner");
            let warn_text = format!(
                "ACHTUNG – Kritischer Ordner/Pfad: {}",
                shorten_path_display(reason_line, 66)
            );
            CONFIRM_LBL_IMPORTANT = create_ctrl(
                0, "STATIC", &warn_text,
                WS_CHILD | WS_VISIBLE, 22, next_y, 496, 28, hwnd, 0, H_FONT_BOLD,
            );
            next_y += 34;
        }

        CONFIRM_CHK_SKIP = create_ctrl(
            0,
            "BUTTON",
            "Sicherheitsprüfung in Zukunft überspringen (außer bei wichtigen System-/Profilordnern)",
            WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX | WS_TABSTOP,
            20,
            next_y,
            500,
            20,
            hwnd,
            ID_CHK_SKIP_PROMPT,
            H_FONT_REGULAR,
        );
        if CONFIRM_SKIP_CHECKED {
            SendMessageW(CONFIRM_CHK_SKIP, BM_SETCHECK, BST_CHECKED, 0);
        }

        let btn_y = client_h - 46;

        // Buttons
        if !is_already_admin {
            create_ctrl(
                0, "BUTTON", "Vernichten (Standard)",
                WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
                18, btn_y, 150, 32, hwnd, 101, H_FONT_REGULAR,
            );
            create_ctrl(
                0, "BUTTON", "Als Administrator vernichten",
                WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON | WS_TABSTOP,
                176, btn_y, 230, 32, hwnd, 102, H_FONT_BOLD,
            );
            create_ctrl(
                0, "BUTTON", "Abbrechen",
                WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
                414, btn_y, 108, 32, hwnd, 103, H_FONT_REGULAR,
            );
        } else {
            create_ctrl(
                0, "BUTTON", "Vernichten inkl. USN-Journal",
                WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON | WS_TABSTOP,
                18, btn_y, 230, 32, hwnd, 102, H_FONT_BOLD,
            );
            create_ctrl(
                0, "BUTTON", "Schnellvernichtung",
                WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
                256, btn_y, 150, 32, hwnd, 101, H_FONT_REGULAR,
            );
            create_ctrl(
                0, "BUTTON", "Abbrechen",
                WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
                414, btn_y, 108, 32, hwnd, 103, H_FONT_REGULAR,
            );
        }

        force_foreground_window(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        (CONFIRMATION_CHOICE, CONFIRM_SKIP_CHECKED)
    }
}

// ==========================================
// Letzte Warnung mit 2-Sekunden-Countdown (#17)
// ==========================================

const ID_FINAL_CONFIRM: usize = 201;
const ID_FINAL_CHK_SKIP: usize = 202;
const ID_FINAL_TIMER: usize = 1;

static FINAL_COUNTDOWN: AtomicI32 = AtomicI32::new(2);
static mut FINAL_CONFIRMED: bool = false;
static mut FINAL_HDR_TITLE: *mut c_void = std::ptr::null_mut();
static mut FINAL_HDR_SUB: *mut c_void = std::ptr::null_mut();
static mut FINAL_LBL_IMPORTANT: *mut c_void = std::ptr::null_mut();
static mut FINAL_BTN_CONFIRM: *mut c_void = std::ptr::null_mut();
static mut FINAL_CHK_SKIP: *mut c_void = std::ptr::null_mut();
static mut FINAL_SKIP_CHECKED: bool = false;

unsafe extern "system" fn final_warning_wnd_proc(
    hwnd: *mut c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    unsafe {
        match msg {
            WM_TIMER => {
                if wparam == ID_FINAL_TIMER {
                    let prev = FINAL_COUNTDOWN.load(Ordering::SeqCst);
                    let remaining = if prev > 0 {
                        let next = prev - 1;
                        FINAL_COUNTDOWN.store(next, Ordering::SeqCst);
                        next
                    } else {
                        0
                    };
                    if remaining > 0 {
                        let txt = format!("Bestätigen ({})", remaining);
                        let w_txt = to_wide_chars(&txt);
                        SetWindowTextW(FINAL_BTN_CONFIRM, w_txt.as_ptr());
                    } else {
                        KillTimer(hwnd, ID_FINAL_TIMER);
                        let w_txt = to_wide_chars("Bestätigen");
                        SetWindowTextW(FINAL_BTN_CONFIRM, w_txt.as_ptr());
                        EnableWindow(FINAL_BTN_CONFIRM, 1);
                        SendMessageW(FINAL_BTN_CONFIRM, BM_SETSTYLE, BS_DEFPUSHBUTTON as usize, 1);
                        SendMessageW(hwnd, DM_SETDEFID, ID_FINAL_CONFIRM, 0);
                        SetFocus(FINAL_BTN_CONFIRM);
                    }
                }
                0
            }
            WM_COMMAND => {
                let id = wparam & 0xFFFF;
                if id == ID_FINAL_CONFIRM {
                    if FINAL_COUNTDOWN.load(Ordering::SeqCst) <= 0 {
                        if !FINAL_CHK_SKIP.is_null() {
                            FINAL_SKIP_CHECKED =
                                SendMessageW(FINAL_CHK_SKIP, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                        }
                        FINAL_CONFIRMED = true;
                        DestroyWindow(hwnd);
                    }
                } else if id == 2 /* IDCANCEL */ {
                    FINAL_CONFIRMED = false;
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_CTLCOLORSTATIC => {
                let hdc = wparam as *mut c_void;
                let h_ctrl = lparam as *mut c_void;
                SetBkMode(hdc, 1 /* TRANSPARENT */);

                if h_ctrl == FINAL_HDR_TITLE || h_ctrl == FINAL_HDR_SUB {
                    if h_ctrl == FINAL_HDR_TITLE {
                        SetTextColor(hdc, 0x001414B4); // Warn-Rot
                    } else {
                        SetTextColor(hdc, 0x00000000);
                    }
                    SetBkColor(hdc, 0x00FFFFFF);
                    H_BRUSH_WHITE as isize
                } else {
                    if !FINAL_LBL_IMPORTANT.is_null() && h_ctrl == FINAL_LBL_IMPORTANT {
                        SetTextColor(hdc, 0x001414B4); // Rot
                    } else {
                        SetTextColor(hdc, 0x00000000);
                    }
                    SetBkColor(hdc, 0x00F0F0F0);
                    H_BRUSH_DIALOG as isize
                }
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);

                let rc_top = RECT { left: 0, top: 0, right: 500, bottom: 72 };
                FillRect(hdc, &rc_top, H_BRUSH_WHITE);

                let rc_line = RECT { left: 0, top: 72, right: 500, bottom: 73 };
                FillRect(hdc, &rc_line, H_BRUSH_LINE);

                let rc_bottom = RECT { left: 0, top: 73, right: 500, bottom: 290 };
                FillRect(hdc, &rc_bottom, H_BRUSH_DIALOG);

                let h_inst = GetModuleHandleW(std::ptr::null());
                let h_icon = LoadImageW(h_inst, 1 as *const u16, 1, 32, 32, LR_SHARED);
                if !h_icon.is_null() {
                    DrawIconEx(hdc, 20, 20, h_icon, 32, 32, 0, std::ptr::null_mut(), 0x0003);
                }

                EndPaint(hwnd, &ps);
                0
            }
            WM_CLOSE => {
                FINAL_CONFIRMED = false;
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                KillTimer(hwnd, ID_FINAL_TIMER);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Zeigt die kompakte „Letzte Warnung“ mit einem 2-Sekunden-Countdown auf dem Bestätigen-Button an.
/// Erst nach Ablauf des Countdowns ist der Button aktiv und per `Enter` auslösbar.
pub fn show_final_warning_dialog(
    paths: &[PathBuf],
    important_reasons: &[String],
    admin_purge_active: bool,
    force_prompt: bool,
) -> (bool, bool) {
    unsafe {
        FINAL_CONFIRMED = false;
        FINAL_COUNTDOWN.store(2, Ordering::SeqCst);
        FINAL_LBL_IMPORTANT = std::ptr::null_mut();
        FINAL_BTN_CONFIRM = std::ptr::null_mut();
        FINAL_CHK_SKIP = std::ptr::null_mut();
        FINAL_SKIP_CHECKED = get_skip_safety_prompt();
        init_gui_resources();

        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide_chars("WinShredFinalWarningClass");

        let mut wnd_class: WNDCLASSEXW = std::mem::zeroed();
        wnd_class.cb_size = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wnd_class.style = 0x0002 | 0x0001;
        wnd_class.lpfn_wnd_proc = Some(final_warning_wnd_proc);
        wnd_class.h_instance = instance;
        wnd_class.h_cursor = LoadCursorW(std::ptr::null_mut(), 32512 as *const u16);
        wnd_class.h_icon = LoadImageW(instance, 1 as *const u16, 1, 32, 32, LR_SHARED);
        wnd_class.h_icon_sm = LoadImageW(instance, 1 as *const u16, 1, 16, 16, LR_SHARED);
        wnd_class.lpsz_class_name = class_name.as_ptr();

        RegisterClassExW(&wnd_class);

        let has_important = !important_reasons.is_empty();
        let client_h = if has_important { 268 } else { 222 };

        let mut rc = RECT { left: 0, top: 0, right: 500, bottom: client_h };
        let win_style = 0x00C00000 | 0x00080000; // WS_CAPTION | WS_SYSMENU
        AdjustWindowRectEx(&mut rc, win_style, 0, 0);
        let win_w = rc.right - rc.left;
        let win_h = rc.bottom - rc.top;

        let screen_w = GetSystemMetrics(0);
        let screen_h = GetSystemMetrics(1);
        let pos_x = (screen_w - win_w) / 2;
        let pos_y = (screen_h - win_h) / 2;

        let window_title = to_wide_chars("WinShred - Letzte Warnung");

        let hwnd = CreateWindowExW(
            0x00040000, // WS_EX_TOPMOST
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
            return (false, FINAL_SKIP_CHECKED);
        }

        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 1 /* ICON_BIG */, wnd_class.h_icon as isize);
        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 0 /* ICON_SMALL */, wnd_class.h_icon_sm as isize);

        let title_text = if has_important {
            "ACHTUNG: Relevanter System-/Profilordner!"
        } else {
            "Letzte Warnung: Unwiderrufliches Vernichten"
        };

        let target_label = if paths.len() == 1 {
            shorten_path_display(&paths[0].to_string_lossy(), 52)
        } else {
            format!("{} ausgewählte Elemente", paths.len())
        };
        let mode_suffix = if admin_purge_active {
            " (Admin inkl. USN-Journal)"
        } else {
            ""
        };
        let sub_text = format!("Ziel: {}{}", target_label, mode_suffix);

        FINAL_HDR_TITLE = create_ctrl(
            0, "STATIC", title_text,
            WS_CHILD | WS_VISIBLE, 64, 14, 416, 22, hwnd, 0, H_FONT_TITLE,
        );
        FINAL_HDR_SUB = create_ctrl(
            0, "STATIC", &sub_text,
            WS_CHILD | WS_VISIBLE, 64, 40, 416, 24, hwnd, 0, H_FONT_SUBTITLE,
        );

        create_ctrl(
            0,
            "STATIC",
            "Die ausgewählten Daten werden sofort überschrieben und endgültig gelöscht.\nBitte warten Sie kurz (2 Sekunden) und bestätigen Sie erst dann zum Löschen.",
            WS_CHILD | WS_VISIBLE,
            22,
            88,
            456,
            34,
            hwnd,
            0,
            H_FONT_REGULAR,
        );

        let mut next_y = 132;
        if has_important {
            let reason = important_reasons
                .first()
                .map(|s| s.as_str())
                .unwrap_or("Relevanter System-, Programm- oder Profilordner");
            let imp_text = format!(
                "Erzwungene Sicherheitsprüfung: {}",
                shorten_path_display(reason, 60)
            );
            FINAL_LBL_IMPORTANT = create_ctrl(
                0, "STATIC", &imp_text,
                WS_CHILD | WS_VISIBLE, 22, next_y, 456, 36, hwnd, 0, H_FONT_BOLD,
            );
            next_y += 44;
        }

        let skip_text = if force_prompt {
            "Normale Sicherheitsprüfung überspringen (für wichtige Ordner immer aktiv)"
        } else {
            "Sicherheitsprüfung in Zukunft überspringen"
        };
        FINAL_CHK_SKIP = create_ctrl(
            0,
            "BUTTON",
            skip_text,
            WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX | WS_TABSTOP,
            22,
            next_y,
            456,
            20,
            hwnd,
            ID_FINAL_CHK_SKIP,
            H_FONT_REGULAR,
        );
        if FINAL_SKIP_CHECKED {
            SendMessageW(FINAL_CHK_SKIP, BM_SETCHECK, BST_CHECKED, 0);
        }

        let btn_y = client_h - 46;
        FINAL_BTN_CONFIRM = create_ctrl(
            0,
            "BUTTON",
            "Bestätigen (2)",
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP | WS_DISABLED,
            210,
            btn_y,
            150,
            32,
            hwnd,
            ID_FINAL_CONFIRM,
            H_FONT_BOLD,
        );
        create_ctrl(
            0,
            "BUTTON",
            "Abbrechen",
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
            370,
            btn_y,
            108,
            32,
            hwnd,
            2, // IDCANCEL
            H_FONT_REGULAR,
        );

        // Solange der 2-Sekunden-Countdown läuft, darf Enter NICHT bestätigen
        SendMessageW(hwnd, DM_SETDEFID, 0, 0);
        SetTimer(hwnd, ID_FINAL_TIMER, 1000, std::ptr::null_mut());
        force_foreground_window(hwnd);
        SetFocus(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        (FINAL_CONFIRMED, FINAL_SKIP_CHECKED)
    }
}

// ==========================================
// GUI Hilfsfunktionen für Fortschritt
// ==========================================

pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn shorten_path_display(path_str: &str, max_chars: usize) -> String {
    let char_count = path_str.chars().count();
    if char_count <= max_chars {
        path_str.to_string()
    } else {
        let chars: Vec<char> = path_str.chars().collect();
        let prefix_len = max_chars.min(15);
        let suffix_len = max_chars.saturating_sub(prefix_len + 3);
        let prefix: String = chars[..prefix_len].iter().collect();
        let suffix: String = chars[chars.len().saturating_sub(suffix_len)..].iter().collect();
        format!("{}...{}", prefix, suffix)
    }
}

fn count_items_and_bytes(paths: &[PathBuf], num_passes: usize, cancel_flag: &AtomicBool) -> (usize, u64) {
    let mut total_items = 0;
    let mut total_bytes = 0;
    let mut drive_ssd_cache: std::collections::HashMap<String, bool> = std::collections::HashMap::new();

    fn scan_item(
        path: &Path,
        items: &mut usize,
        bytes: &mut u64,
        num_passes: usize,
        cancel: &AtomicBool,
        drive_ssd_cache: &mut std::collections::HashMap<String, bool>,
    ) {
        if cancel.load(Ordering::SeqCst) || is_dangerous_or_root_path(path) {
            return;
        }
        let sym_meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(_) => return,
        };
        *items += 1;
        if sym_meta.file_type().is_symlink() {
            return;
        }
        if sym_meta.is_dir() {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    if cancel.load(Ordering::SeqCst) {
                        return;
                    }
                    scan_item(&entry.path(), items, bytes, num_passes, cancel, drive_ssd_cache);
                }
            }
        } else {
            let is_ssd = if let Some(drive) = extract_drive_letter(path) {
                *drive_ssd_cache.entry(drive).or_insert_with(|| {
                    matches!(get_drive_media_type(path), DriveMediaType::SsdNvme { .. })
                })
            } else {
                matches!(get_drive_media_type(path), DriveMediaType::SsdNvme { .. })
            };
            *bytes += estimate_file_wipe_bytes(path, sym_meta.len(), num_passes, is_ssd);
        }
    }

    for p in paths {
        scan_item(
            p,
            &mut total_items,
            &mut total_bytes,
            num_passes,
            cancel_flag,
            &mut drive_ssd_cache,
        );
    }

    (total_items, total_bytes)
}

// ==========================================
// Fortschritts-Dialog (Progress Dialog)
// ==========================================

pub struct ShredOutcome {
    pub cancelled: bool,
    pub errors: usize,
    pub journal_purged: bool,
    pub journal_errors: usize,
    pub journal_error_details: Vec<String>,
}

struct ShredProgressShared {
    total_items: usize,
    processed_items: usize,
    total_bytes: u64,
    processed_bytes: u64,
    current_item: String,
    detail_status: String,
    cancelled: bool,
    completed: bool,
    errors: usize,
    journal_purged: bool,
    journal_errors: usize,
    journal_error_details: Vec<String>,
}

struct ProgressDialogControls {
    lbl_title: *mut c_void,
    lbl_subtitle: *mut c_void,
    prog_bar: *mut c_void,
    lbl_status: *mut c_void,
    lbl_detail: *mut c_void,
    btn_cancel: *mut c_void,
}

static mut PROG_CTRLS: ProgressDialogControls = ProgressDialogControls {
    lbl_title: std::ptr::null_mut(),
    lbl_subtitle: std::ptr::null_mut(),
    prog_bar: std::ptr::null_mut(),
    lbl_status: std::ptr::null_mut(),
    lbl_detail: std::ptr::null_mut(),
    btn_cancel: std::ptr::null_mut(),
};

static PROG_SHARED: Mutex<Option<Arc<Mutex<ShredProgressShared>>>> = Mutex::new(None);
static PROG_CANCEL: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn progress_dialog_wnd_proc(
    hwnd: *mut c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = wparam & 0xFFFF;
                if id == ID_BTN_PROG_CANCEL || id == 2 /* IDCANCEL */ {
                    PROG_CANCEL.store(true, Ordering::SeqCst);
                    EnableWindow(PROG_CTRLS.btn_cancel, 0);
                    let w_cancelling = to_wide_chars("Abbruch wird eingeleitet...");
                    SetWindowTextW(PROG_CTRLS.lbl_subtitle, w_cancelling.as_ptr());
                    SetWindowTextW(PROG_CTRLS.lbl_status, w_cancelling.as_ptr());
                }
                0
            }
            WM_CTLCOLORSTATIC => {
                let hdc = wparam as *mut c_void;
                let h_ctrl = lparam as *mut c_void;
                SetBkMode(hdc, 1 /* TRANSPARENT */);

                if h_ctrl == PROG_CTRLS.lbl_title || h_ctrl == PROG_CTRLS.lbl_subtitle {
                    SetBkColor(hdc, 0x00FFFFFF);
                    H_BRUSH_WHITE as isize
                } else {
                    SetBkColor(hdc, 0x00F0F0F0);
                    H_BRUSH_DIALOG as isize
                }
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);

                // 1. Header-Bereich (Weiss)
                let rc_top = RECT { left: 0, top: 0, right: 520, bottom: 70 };
                FillRect(hdc, &rc_top, H_BRUSH_WHITE);

                // 2. Trennlinie unter dem Header
                let rc_line = RECT { left: 0, top: 70, right: 520, bottom: 71 };
                FillRect(hdc, &rc_line, H_BRUSH_LINE);

                // 3. Unterer Bereich (Dialog-Grau)
                let rc_bottom = RECT { left: 0, top: 71, right: 520, bottom: 200 };
                FillRect(hdc, &rc_bottom, H_BRUSH_DIALOG);

                // 4. Logo / Icon im Header
                let h_inst = GetModuleHandleW(std::ptr::null());
                let h_icon = LoadImageW(h_inst, 1 as *const u16, 1, 32, 32, LR_SHARED);
                if !h_icon.is_null() {
                    DrawIconEx(hdc, 20, 18, h_icon, 32, 32, 0, std::ptr::null_mut(), 0x0003);
                }

                EndPaint(hwnd, &ps);
                0
            }
            WM_APP_PROGRESS_UPDATE => {
                if PROG_CANCEL.load(Ordering::SeqCst) {
                    return 0;
                }
                if let Ok(guard_opt) = PROG_SHARED.lock() {
                    if let Some(ref shared_arc) = *guard_opt {
                        if let Ok(guard) = shared_arc.lock() {
                            let total_b = guard.total_bytes;
                            let proc_b = guard.processed_bytes;
                            let total_i = guard.total_items;
                            let proc_i = guard.processed_items;
                            let cur_item = guard.current_item.clone();
                            let detail = guard.detail_status.clone();
                            drop(guard);

                            let w_sub = to_wide_chars(&cur_item);
                            SetWindowTextW(PROG_CTRLS.lbl_subtitle, w_sub.as_ptr());

                            if !detail.is_empty() {
                                let w_det = to_wide_chars(&detail);
                                SetWindowTextW(PROG_CTRLS.lbl_detail, w_det.as_ptr());
                            }

                            let (pct, status_text) = if total_b > 0 {
                                let p = ((proc_b as f64 / total_b as f64) * 100.0).clamp(0.0, 100.0);
                                let s = format!(
                                    "Element {} von {} ({}%) - {} von {}",
                                    proc_i.min(total_i),
                                    total_i,
                                    p as u32,
                                    format_file_size(proc_b.min(total_b)),
                                    format_file_size(total_b)
                                );
                                (p, s)
                            } else if total_i > 0 {
                                let p = ((proc_i as f64 / total_i as f64) * 100.0).clamp(0.0, 100.0);
                                let s = format!("Element {} von {} ({}%)", proc_i.min(total_i), total_i, p as u32);
                                (p, s)
                            } else {
                                (0.0, "Wird initialisiert...".to_string())
                            };

                            let bar_pos = (pct * 10.0) as usize; // Range 0..1000
                            SendMessageW(PROG_CTRLS.prog_bar, PBM_SETPOS, bar_pos, 0);

                            let w_status = to_wide_chars(&status_text);
                            SetWindowTextW(PROG_CTRLS.lbl_status, w_status.as_ptr());
                        }
                    }
                }
                0
            }
            WM_APP_SHRED_DONE => {
                DestroyWindow(hwnd);
                0
            }
            WM_CLOSE => {
                PROG_CANCEL.store(true, Ordering::SeqCst);
                EnableWindow(PROG_CTRLS.btn_cancel, 0);
                let w_cancelling = to_wide_chars("Abbruch wird eingeleitet...");
                SetWindowTextW(PROG_CTRLS.lbl_subtitle, w_cancelling.as_ptr());
                SetWindowTextW(PROG_CTRLS.lbl_status, w_cancelling.as_ptr());
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

struct GuiShredReporter {
    shared: Arc<Mutex<ShredProgressShared>>,
    hwnd_usize: usize,
    last_ui_update: Instant,
    accumulated_bytes: u64,
    pending_items: usize,
}

impl ShredReporter for GuiShredReporter {
    fn is_cancelled(&self) -> bool {
        PROG_CANCEL.load(Ordering::SeqCst)
    }

    fn on_item_start(&mut self, path: &Path, is_dir: bool) {
        if self.is_cancelled() {
            return;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let short_name = shorten_path_display(&name, 45);
        let typ = if is_dir { "Ordner" } else { "Datei" };

        if let Ok(mut g) = self.shared.lock() {
            g.current_item = format!("{}: {}", typ, short_name);
        }

        if self.last_ui_update.elapsed().as_millis() >= 30 {
            self.flush_updates();
        }
    }

    fn on_bytes_written(&mut self, bytes: u64) {
        if self.is_cancelled() {
            return;
        }
        self.accumulated_bytes += bytes;
        if self.last_ui_update.elapsed().as_millis() >= 30 || self.accumulated_bytes >= 1024 * 1024 {
            self.flush_updates();
        }
    }

    fn on_item_done(&mut self, _path: &Path, _is_dir: bool) {
        if self.is_cancelled() {
            return;
        }
        self.pending_items += 1;
        if self.last_ui_update.elapsed().as_millis() >= 30 {
            self.flush_updates();
        }
    }
}

impl GuiShredReporter {
    fn flush_updates(&mut self) {
        if self.is_cancelled() {
            return;
        }
        if let Ok(mut g) = self.shared.lock() {
            g.processed_bytes += self.accumulated_bytes;
            g.processed_items += self.pending_items;
            self.accumulated_bytes = 0;
            self.pending_items = 0;
            self.last_ui_update = Instant::now();
        }
        unsafe {
            PostMessageW(self.hwnd_usize as *mut c_void, WM_APP_PROGRESS_UPDATE, 0, 0);
        }
    }
}

pub fn show_shred_progress_dialog(
    paths: &[PathBuf],
    num_passes: usize,
    purge_journal: bool,
    is_admin: bool,
) -> ShredOutcome {
    unsafe {
        init_gui_resources();

        let icex = INITCOMMONCONTROLSEX {
            dw_size: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dw_icc: 0x00000020 | 0x00004000 | 0x00000008, // ICC_PROGRESS_CLASS | ICC_STANDARD_CLASSES | ICC_WIN95_CLASSES
        };
        InitCommonControlsEx(&icex);

        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide_chars("WinShredProgressDialogClass");

        let mut wnd_class: WNDCLASSEXW = std::mem::zeroed();
        wnd_class.cb_size = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wnd_class.style = 0x0002 | 0x0001; // CS_HREDRAW | CS_VREDRAW
        wnd_class.lpfn_wnd_proc = Some(progress_dialog_wnd_proc);
        wnd_class.h_instance = instance;
        wnd_class.h_cursor = LoadCursorW(std::ptr::null_mut(), 32512 as *const u16);
        wnd_class.h_icon = LoadImageW(instance, 1 as *const u16, 1, 32, 32, LR_SHARED);
        wnd_class.h_icon_sm = LoadImageW(instance, 1 as *const u16, 1, 16, 16, LR_SHARED);
        wnd_class.lpsz_class_name = class_name.as_ptr();

        RegisterClassExW(&wnd_class);

        // Fenstergröße (Client 520x200)
        let mut rc = RECT { left: 0, top: 0, right: 520, bottom: 200 };
        let win_style = 0x00000000 | 0x00C00000 | 0x00080000; // WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU
        AdjustWindowRectEx(&mut rc, win_style, 0, 0);
        let win_w = rc.right - rc.left;
        let win_h = rc.bottom - rc.top;

        let screen_w = GetSystemMetrics(0);
        let screen_h = GetSystemMetrics(1);
        let pos_x = (screen_w - win_w) / 2;
        let pos_y = (screen_h - win_h) / 2;

        let window_title = to_wide_chars("WinShred - Vernichtung läuft...");
        let hwnd = CreateWindowExW(
            0x00040000, // WS_EX_TOPMOST
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
            return ShredOutcome {
                cancelled: false,
                errors: 1,
                journal_purged: false,
                journal_errors: 0,
                journal_error_details: Vec::new(),
            };
        }

        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 1 /* ICON_BIG */, wnd_class.h_icon as isize);
        SendMessageW(hwnd, 0x0080 /* WM_SETICON */, 0 /* ICON_SMALL */, wnd_class.h_icon_sm as isize);

        // Medientyp für Statusanzeige ermitteln
        let mut has_ssd = false;
        let mut has_hdd = false;
        for p in paths {
            match get_drive_media_type(p) {
                DriveMediaType::SsdNvme { .. } => has_ssd = true,
                DriveMediaType::RotationalHdd => has_hdd = true,
                DriveMediaType::Unknown => (),
            }
        }
        let mode_detail = if has_ssd && !has_hdd {
            "SSD / NVMe: Flash-optimiertes Wiping & Hardware-TRIM"
        } else if has_hdd && !has_ssd {
            if num_passes > 1 {
                "HDD: 3-Pass DoD 5220.22-M Wiping"
            } else {
                "HDD: 1-Pass Krypto-Zufall (NIST SP 800-88 konform)"
            }
        } else {
            "Adaptive medienspezifische Vernichtung"
        };

        // Header Labels (Weiss)
        PROG_CTRLS.lbl_title = create_ctrl(
            0, "STATIC", "Dateien werden sicher vernichtet...",
            WS_CHILD | WS_VISIBLE, 65, 14, 435, 22, hwnd, 0, H_FONT_TITLE,
        );
        PROG_CTRLS.lbl_subtitle = create_ctrl(
            0, "STATIC", "Initialisiere...",
            WS_CHILD | WS_VISIBLE, 65, 38, 435, 20, hwnd, 0, H_FONT_SUBTITLE,
        );

        // Progress Bar
        PROG_CTRLS.prog_bar = create_ctrl(
            0, "msctls_progress32", "",
            WS_CHILD | WS_VISIBLE | PBS_SMOOTH, 25, 86, 470, 22, hwnd, ID_SHRED_PROGRESS_BAR, std::ptr::null_mut(),
        );
        SendMessageW(PROG_CTRLS.prog_bar, PBM_SETRANGE32, 0, 1000);
        SendMessageW(PROG_CTRLS.prog_bar, PBM_SETPOS, 0, 0);

        // Status Label (Element X von Y / Bytes)
        PROG_CTRLS.lbl_status = create_ctrl(
            0, "STATIC", "Element 0 von 0 (0%)",
            WS_CHILD | WS_VISIBLE, 25, 116, 470, 20, hwnd, 0, H_FONT_REGULAR,
        );

        // Detail Label (SSD / HDD Mode)
        PROG_CTRLS.lbl_detail = create_ctrl(
            0, "STATIC", mode_detail,
            WS_CHILD | WS_VISIBLE, 25, 146, 345, 30, hwnd, 0, H_FONT_REGULAR,
        );

        // Abbrechen Button
        PROG_CTRLS.btn_cancel = create_ctrl(
            0, "BUTTON", "Abbrechen",
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
            385, 146, 110, 32, hwnd, ID_BTN_PROG_CANCEL, H_FONT_REGULAR,
        );

        PROG_CANCEL.store(false, Ordering::SeqCst);

        let shared = Arc::new(Mutex::new(ShredProgressShared {
            total_items: 0,
            processed_items: 0,
            total_bytes: 0,
            processed_bytes: 0,
            current_item: "Vorbereitung...".to_string(),
            detail_status: mode_detail.to_string(),
            cancelled: false,
            completed: false,
            errors: 0,
            journal_purged: false,
            journal_errors: 0,
            journal_error_details: Vec::new(),
        }));

        if let Ok(mut guard) = PROG_SHARED.lock() {
            *guard = Some(Arc::clone(&shared));
        }

        force_foreground_window(hwnd);

        let paths_clone = paths.to_vec();
        let shared_clone = Arc::clone(&shared);
        let hwnd_usize = hwnd as usize;

        let worker_handle = std::thread::spawn(move || {
            let hwnd = hwnd_usize as *mut c_void;

            // 1. Pre-Scan: Zähle Elemente und Gesamt-Bytes
            let (total_items, total_bytes) = count_items_and_bytes(&paths_clone, num_passes, &PROG_CANCEL);

            if let Ok(mut g) = shared_clone.lock() {
                g.total_items = total_items;
                g.total_bytes = total_bytes;
                g.current_item = if total_items == 0 {
                    "Keine Elemente".to_string()
                } else {
                    "Vernichtung wird gestartet...".to_string()
                };
            }
            if !PROG_CANCEL.load(Ordering::SeqCst) {
                PostMessageW(hwnd, WM_APP_PROGRESS_UPDATE, 0, 0);
            } else {
                if let Ok(mut g) = shared_clone.lock() {
                    g.cancelled = true;
                    g.completed = true;
                }
                PostMessageW(hwnd, WM_APP_SHRED_DONE, 0, 0);
                return;
            }

            // 2. Vernichtung
            let mut reporter = GuiShredReporter {
                shared: Arc::clone(&shared_clone),
                hwnd_usize,
                last_ui_update: Instant::now(),
                accumulated_bytes: 0,
                pending_items: 0,
            };

            let mut errors = 0;
            let mut affected_volumes = std::collections::HashSet::new();

            for path in &paths_clone {
                if PROG_CANCEL.load(Ordering::SeqCst) {
                    break;
                }
                if let Some(drive) = extract_drive_letter(path) {
                    affected_volumes.insert(PathBuf::from(format!("{}\\", drive)));
                }
                shred_recursive_with_reporter(path, num_passes, &mut errors, &mut reporter);
            }

            reporter.flush_updates();

            // 3. USN Journal Purge (wenn gewünscht & Admin & nicht abgebrochen)
            let mut journal_purged = false;
            let mut journal_errors = 0usize;
            let mut journal_error_details = Vec::new();
            if purge_journal && is_admin && !PROG_CANCEL.load(Ordering::SeqCst) {
                if let Ok(mut g) = shared_clone.lock() {
                    g.detail_status = "NTFS USN-Journal ($UsnJrnl) wird bereinigt...".to_string();
                }
                PostMessageW(hwnd, WM_APP_PROGRESS_UPDATE, 0, 0);
                for vol in &affected_volumes {
                    if PROG_CANCEL.load(Ordering::SeqCst) {
                        break;
                    }
                    match purge_volume_usn_journal(vol) {
                        Ok(true) => journal_purged = true,
                        Ok(false) => {}
                        Err(e) => {
                            journal_errors += 1;
                            let detail = format!("{}: {}", vol.display(), e);
                            eprintln!("USN-Journal-Bereinigung fehlgeschlagen für {}", detail);
                            journal_error_details.push(detail);
                        }
                    }
                }
            }

            // 4. Fertigstellung
            let cancelled = PROG_CANCEL.load(Ordering::SeqCst);
            if let Ok(mut g) = shared_clone.lock() {
                g.errors = errors;
                g.cancelled = cancelled;
                g.completed = true;
                g.journal_purged = journal_purged;
                g.journal_errors = journal_errors;
                g.journal_error_details = journal_error_details;
                if !cancelled {
                    g.processed_items = g.total_items;
                    g.processed_bytes = g.total_bytes;
                }
            }

            if !cancelled {
                PostMessageW(hwnd, WM_APP_PROGRESS_UPDATE, 0, 0);
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            PostMessageW(hwnd, WM_APP_SHRED_DONE, 0, 0);
        });

        // Message Loop
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Sicherstellen, dass Worker-Thread sauber beendet ist
        PROG_CANCEL.store(true, Ordering::SeqCst);
        let _ = worker_handle.join();

        let outcome = if let Ok(guard_opt) = PROG_SHARED.lock() {
            if let Some(ref shared_arc) = *guard_opt {
                if let Ok(data) = shared_arc.lock() {
                    ShredOutcome {
                        cancelled: data.cancelled,
                        errors: data.errors,
                        journal_purged: data.journal_purged,
                        journal_errors: data.journal_errors,
                        journal_error_details: data.journal_error_details.clone(),
                    }
                } else {
                    ShredOutcome { cancelled: false, errors: 0, journal_purged: false, journal_errors: 0, journal_error_details: Vec::new() }
                }
            } else {
                ShredOutcome { cancelled: false, errors: 0, journal_purged: false, journal_errors: 0, journal_error_details: Vec::new() }
            }
        } else {
            ShredOutcome { cancelled: false, errors: 0, journal_purged: false, journal_errors: 0, journal_error_details: Vec::new() }
        };

        if let Ok(mut guard_opt) = PROG_SHARED.lock() {
            *guard_opt = None;
        }

        outcome
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedCliArgs {
    install: bool,
    uninstall: bool,
    reset_prompts: bool,
    show_help: bool,
    auto_confirm: bool,
    is_quiet: bool,
    purge_journal: bool,
    elevated_confirmed: bool,
    custom_passes: Option<usize>,
    filelists: Vec<PathBuf>,
    raw_paths: Vec<PathBuf>,
}

fn parse_passes_value(val: &str) -> Result<usize, String> {
    let n = val
        .parse::<usize>()
        .map_err(|_| format!("Ungueltiger Wert fuer --passes: '{}'", val))?;
    if n == 0 {
        return Err(String::from("Ungueltiger Wert fuer --passes: muss mindestens 1 sein."));
    }
    Ok(n)
}

fn parse_cli_args<I, S>(args: I) -> Result<ParsedCliArgs, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut parsed = ParsedCliArgs::default();
    let mut iter = args.into_iter().map(|s| s.as_ref().to_string());
    let mut end_of_options = false;

    while let Some(arg) = iter.next() {
        if end_of_options {
            parsed.raw_paths.push(PathBuf::from(arg));
            continue;
        }
        if arg == "--" {
            end_of_options = true;
            continue;
        }
        match arg.as_str() {
            "--help" | "-h" | "/?" => parsed.show_help = true,
            "--install" | "-i" => parsed.install = true,
            "--uninstall" | "-u" => parsed.uninstall = true,
            "--reset-prompts" => parsed.reset_prompts = true,
            "--quiet" => {
                parsed.auto_confirm = true;
                parsed.is_quiet = true;
            }
            "-y" | "--yes" => parsed.auto_confirm = true,
            "--admin" | "--admin-purge" => parsed.purge_journal = true,
            "--elevated-confirmed" => {
                parsed.elevated_confirmed = true;
                parsed.purge_journal = true;
            }
            "--purge-journal" | "-p" => parsed.purge_journal = true,
            "--passes" => {
                let val = iter
                    .next()
                    .ok_or_else(|| String::from("Option --passes erfordert eine Zahl (>= 1)."))?;
                parsed.custom_passes = Some(parse_passes_value(&val)?);
            }
            "--filelist" => {
                let val = iter
                    .next()
                    .ok_or_else(|| String::from("Option --filelist erfordert einen Dateipfad."))?;
                if val.trim().is_empty() {
                    return Err(String::from("Option --filelist erfordert einen nicht-leeren Pfad."));
                }
                parsed.filelists.push(PathBuf::from(val));
            }
            _ => {
                if let Some(val) = arg.strip_prefix("--passes=") {
                    parsed.custom_passes = Some(parse_passes_value(val)?);
                } else if let Some(val) = arg.strip_prefix("--filelist=") {
                    if val.trim().is_empty() {
                        return Err(String::from("Option --filelist erfordert einen nicht-leeren Pfad."));
                    }
                    parsed.filelists.push(PathBuf::from(val));
                } else if arg.starts_with('-') {
                    return Err(format!("Unbekanntes Argument: {}", arg));
                } else {
                    parsed.raw_paths.push(PathBuf::from(arg));
                }
            }
        }
    }

    Ok(parsed)
}

fn read_paths_from_filelist(list_path: &Path, delete_after: bool) -> Result<Vec<PathBuf>, String> {
    let content = fs::read_to_string(list_path)
        .map_err(|e| format!("Dateiliste '{}' konnte nicht gelesen werden: {}", list_path.display(), e))?;
    if delete_after {
        let _ = fs::remove_file(list_path);
    }
    let mut paths = Vec::new();
    for raw_line in content.lines() {
        let clean = raw_line.trim_start_matches('\u{feff}').trim();
        if !clean.is_empty() {
            paths.push(PathBuf::from(clean));
        }
    }
    Ok(paths)
}

static ELEVATE_LIST_SEQ: AtomicU32 = AtomicU32::new(1);

fn relaunch_elevated_with_paths(valid_paths: &[PathBuf], custom_passes: Option<usize>) {
    let mut parts = vec![
        String::from("--admin"),
        String::from("--elevated-confirmed"),
    ];
    if let Some(p_count) = custom_passes {
        parts.push(String::from("--passes"));
        parts.push(p_count.to_string());
    }

    let mut direct_parts = parts.clone();
    for p in valid_paths {
        direct_parts.push(quote_windows_cli_arg(&p.to_string_lossy()));
    }
    let joined_direct = direct_parts.join(" ");
    let utf16_len: usize = joined_direct.encode_utf16().count();

    if utf16_len <= 28_000 {
        elevate_relaunch(&joined_direct);
        return;
    }

    let seq = ELEVATE_LIST_SEQ.fetch_add(1, Ordering::Relaxed);
    let list_path = env::temp_dir().join(format!("winshred_elev_{}_{}.lst", std::process::id(), seq));
    let body = valid_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    if fs::write(&list_path, body).is_ok() {
        parts.push(String::from("--filelist"));
        parts.push(quote_windows_cli_arg(&list_path.to_string_lossy()));
        elevate_relaunch(&parts.join(" "));
    } else {
        elevate_relaunch(&joined_direct);
    }
}

fn cli_help_text() -> &'static str {
    "WinShred - Sichere Datei- und Ordnervernichtung\n\n\
Aufruf:\n\
  winshred.exe                        Startet den Installations-/Verwaltungsassistenten\n\
  winshred.exe [OPTIONEN] <PFADE...>  Vernichtet angegebene Dateien/Ordner sicher\n\n\
Optionen:\n\
  --install, -i         Installiert Kontextmenue (Modern + SendTo) im Hintergrund\n\
  --uninstall, -u       Deinstalliert WinShred vollstaendig im Hintergrund\n\
  --reset-prompts       Aktiviert die Sicherheitsabfrage vor dem Loeschen wieder\n\
  --passes <N>          Anzahl der Ueberschreib-Durchgaenge fuer HDDs (Standard: 3, SSD immer 1)\n\
  --filelist <DATEI>    Liest zu vernichtende Pfade zeilenweise aus UTF-8-Datei\n\
  --admin, -p           Bereinigt zusaetzlich das NTFS USN-Journal ($UsnJrnl)\n\
  --yes, -y             Ueberspringt die Sicherheitsabfrage (zeigt Fortschrittsdialog)\n\
  --quiet               Headless-Ausfuehrung ohne GUI-Dialoge (mit -y)\n\
  --help, -h            Zeigt diese Hilfe an"
}

fn report_cli_error(msg: &str, quiet: bool) {
    let has_console = attach_parent_console_if_available();
    eprintln!("[FEHLER] {}", msg);
    if !quiet && !has_console {
        show_message_box(
            msg,
            "WinShred - Fehler",
            MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
}

// ==========================================
// Entry Point (CLI / GUI / Kontextmenue)
// ==========================================
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // 1. Ohne Parameter gestartet (Doppelklick auf die EXE) -> Installer Wizard
    if args.is_empty() {
        let is_admin = unsafe { IsUserAnAdmin() } != 0;
        if !is_admin {
            elevate_relaunch("");
            return;
        }
        show_installer_wizard();
        schedule_self_delete_on_exit();
        return;
    }

    // 2. Argumente streng parsen und validieren (#8 & #9)
    let parsed = match parse_cli_args(&args) {
        Ok(p) => p,
        Err(err) => {
            let is_quiet_arg = args.iter().any(|a| a == "--quiet");
            report_cli_error(
                &format!(
                    "{}\n\nVerwenden Sie 'winshred.exe --help' fuer eine Uebersicht der Optionen.",
                    err
                ),
                is_quiet_arg,
            );
            std::process::exit(1);
        }
    };

    if parsed.show_help {
        let has_console = attach_parent_console_if_available();
        println!("{}", cli_help_text());
        if !parsed.is_quiet && !has_console {
            show_message_box(
                cli_help_text(),
                "WinShred - Hilfe",
                MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND,
            );
        }
        return;
    }

    // 3. Spezial-Flags fuer Headless CLI / Einstellungen
    if parsed.install {
        let is_admin = unsafe { IsUserAnAdmin() } != 0;
        if !is_admin {
            elevate_relaunch("--install");
            return;
        }
        let has_console = attach_parent_console_if_available();
        match run_install_backend(true, false, true) {
            Ok(msg) => {
                println!("{}", msg);
            }
            Err(e) => {
                eprintln!("[FEHLER] Installation fehlgeschlagen:\n{}", e);
                if !has_console {
                    show_message_box(
                        &format!("Installation fehlgeschlagen:\n{}", e),
                        "WinShred - Installationsfehler",
                        MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
                    );
                }
                std::process::exit(1);
            }
        }
        return;
    }

    if parsed.uninstall {
        let is_admin = unsafe { IsUserAnAdmin() } != 0;
        if !is_admin {
            elevate_relaunch("--uninstall");
            return;
        }
        let has_console = attach_parent_console_if_available();
        let res = run_uninstall_backend();
        schedule_self_delete_on_exit();
        match res {
            Ok(()) => {
                println!("WinShred wurde vollstaendig deinstalliert.");
            }
            Err(e) => {
                eprintln!("[FEHLER] Deinstallation fehlgeschlagen:\n{}", e);
                if !has_console {
                    show_message_box(
                        &format!("Deinstallation fehlgeschlagen:\n{}", e),
                        "WinShred - Deinstallationsfehler",
                        MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
                    );
                }
                std::process::exit(1);
            }
        }
        return;
    }

    if parsed.reset_prompts {
        set_skip_safety_prompt(false);
        show_message_box(
            "Die Sicherheitsprüfung vor dem Vernichten wurde wieder aktiviert.",
            "WinShred - Einstellung zurückgesetzt",
            MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND,
        );
        return;
    }

    // 4. Mit Dateipfaden gestartet -> Shredder-Modus
    let auto_confirm = parsed.auto_confirm;
    let is_quiet = parsed.is_quiet;
    let mut purge_journal = parsed.purge_journal;
    let elevated_confirmed = parsed.elevated_confirmed;
    let custom_passes = parsed.custom_passes;

    if is_quiet {
        attach_parent_console_if_available();
    }

    let mut candidate_paths = parsed.raw_paths;
    for list_path in &parsed.filelists {
        match read_paths_from_filelist(list_path, true) {
            Ok(mut listed) => candidate_paths.append(&mut listed),
            Err(err) => {
                if is_quiet {
                    eprintln!("[FEHLER] {}", err);
                } else {
                    report_cli_error(&err, false);
                }
                std::process::exit(1);
            }
        }
    }

    let paths_to_shred: Vec<PathBuf> = candidate_paths
        .into_iter()
        .filter(|p| fs::symlink_metadata(p).is_ok() || p.exists())
        .collect();

    if paths_to_shred.is_empty() {
        if is_quiet {
            eprintln!("[FEHLER] Keine gueltigen Dateien oder Ordner zum Vernichten gefunden.");
            std::process::exit(1);
        } else {
            show_message_box(
                "Keine gültigen Dateien oder Ordner zum Vernichten gefunden.",
                "WinShred - Hinweis",
                MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND,
            );
            return;
        }
    }

    // Sicherheitsprüfung (#5 & #18): Blockade für Systemkern/Roots und erzwungene Warnung für relevante Ordner
    let mut dangerous_paths: Vec<(PathBuf, String)> = Vec::new();
    let mut valid_paths: Vec<PathBuf> = Vec::new();
    let mut important_reasons: Vec<String> = Vec::new();

    for p in paths_to_shred {
        match classify_path_safety(&p) {
            PathSafetyLevel::Blocked(reason) => {
                dangerous_paths.push((p, reason));
            }
            PathSafetyLevel::Important(reason) => {
                important_reasons.push(format!("{} ({})", p.display(), reason));
                valid_paths.push(p);
            }
            PathSafetyLevel::Safe => {
                valid_paths.push(p);
            }
        }
    }

    if !dangerous_paths.is_empty() {
        let msg = format!(
            "Sicherheitsblockade:\n\nFolgende Stamm- oder Systemverzeichnisse dürfen aus Sicherheitsgründen nicht vernichtet werden:\n\n{}\n\nDer Vorgang wurde für diese Verzeichnisse blockiert.",
            dangerous_paths
                .iter()
                .map(|(p, r)| format!("- {} ({})", p.display(), r))
                .collect::<Vec<_>>()
                .join("\n")
        );
        if is_quiet {
            eprintln!("[FEHLER] {}", msg);
        } else {
            show_message_box(&msg, "WinShred - Schutzfunktion", MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND);
        }
    }

    if valid_paths.is_empty() {
        if is_quiet {
            std::process::exit(1);
        }
        return;
    }

    // Auf HDDs standardmäßig 3-Pass DoD 5220.22-M; auf SSD/NVMe & residenten Dateien intern immer 1 Pass (#7)
    let num_passes = custom_passes.unwrap_or(3);

    let is_admin = unsafe { IsUserAnAdmin() } != 0;
    let has_important = !important_reasons.is_empty();
    let skip_saved = get_skip_safety_prompt();
    let should_prompt = !auto_confirm && (has_important || !skip_saved);

    // Sicherheitsabfrage vor dem Vernichten (#17 & #18)
    if should_prompt {
        if !elevated_confirmed {
            let (choice, skip_checked) =
                show_confirmation_dialog(&valid_paths, is_admin, &important_reasons, num_passes);
            match choice {
                ConfirmationResult::Cancel => return,
                ConfirmationResult::ShredStandard => {
                    set_skip_safety_prompt(skip_checked);
                    purge_journal = false;
                }
                ConfirmationResult::ShredAsAdmin => {
                    set_skip_safety_prompt(skip_checked);
                    if !is_admin {
                        relaunch_elevated_with_paths(&valid_paths, custom_passes);
                        return;
                    } else {
                        purge_journal = true;
                    }
                }
            }
        }

        // Kompakte „Letzte Warnung“ mit 2-Sekunden-Countdown (auch direkt nach Admin-Elevation!)
        let (confirmed, skip_checked_final) = show_final_warning_dialog(
            &valid_paths,
            &important_reasons,
            purge_journal && is_admin,
            has_important,
        );
        if !confirmed {
            return;
        }
        set_skip_safety_prompt(skip_checked_final);
    }

    // Cursor auf Warte-Zustand (Sanduhr/Spinning Wheel) setzen
    let h_wait = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_WAIT as *const u16) };
    if !h_wait.is_null() {
        unsafe { SetCursor(h_wait); }
    }

    // Ausführung: Entweder Headless (wenn --quiet) oder mit grafischem Fortschrittsbalken
    let outcome = if is_quiet {
        let mut errs = 0;
        let mut affected_volumes = std::collections::HashSet::new();
        for path in &valid_paths {
            if let Some(drive) = extract_drive_letter(path) {
                affected_volumes.insert(PathBuf::from(format!("{}\\", drive)));
            }
            shred_recursive(path, num_passes, &mut errs);
        }
        let mut journal_purged = false;
        let mut journal_errors = 0usize;
        let mut journal_error_details = Vec::new();
        if purge_journal && is_admin {
            for vol in &affected_volumes {
                match purge_volume_usn_journal(vol) {
                    Ok(true) => journal_purged = true,
                    Ok(false) => {}
                    Err(e) => {
                        journal_errors += 1;
                        journal_error_details.push(format!("{}: {}", vol.display(), e));
                    }
                }
            }
        }
        ShredOutcome {
            cancelled: false,
            errors: errs,
            journal_purged,
            journal_errors,
            journal_error_details,
        }
    } else {
        show_shred_progress_dialog(
            &valid_paths,
            num_passes,
            purge_journal,
            is_admin,
        )
    };

    if is_quiet {
        if outcome.errors > 0 || outcome.journal_errors > 0 {
            eprintln!(
                "[FEHLER] Vernichtung mit {} Datei-Fehler(n) und {} Journal-Fehler(n) beendet.",
                outcome.errors, outcome.journal_errors
            );
            for detail in &outcome.journal_error_details {
                eprintln!("[FEHLER] USN-Journal: {}", detail);
            }
            std::process::exit(1);
        }
    } else {
        if outcome.cancelled {
            show_message_box(
                "Der Vernichtungsvorgang wurde durch den Benutzer abgebrochen.\nEinige Elemente wurden möglicherweise bereits vernichtet.",
                "WinShred - Abgebrochen",
                MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND,
            );
        } else if outcome.errors > 0 {
            show_message_box(
                &format!("WinShred abgeschlossen, aber bei {} Element(en) gab es Zugriffsfehler.", outcome.errors),
                "WinShred - Hinweis",
                MB_ICONERROR | MB_TOPMOST | MB_SETFOREGROUND,
            );
            std::process::exit(1);
        } else if outcome.journal_errors > 0 {
            let details_block = if outcome.journal_error_details.is_empty() {
                String::new()
            } else {
                format!("\n\nDetails:\n{}", outcome.journal_error_details.join("\n"))
            };
            show_message_box(
                &format!(
                    "Dateien wurden unwiderruflich vernichtet, aber das NTFS USN-Journal konnte auf {} Laufwerk(en) nicht bereinigt werden.{}",
                    outcome.journal_errors,
                    details_block
                ),
                "WinShred - Hinweis zur Journal-Bereinigung",
                MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND,
            );
            std::process::exit(1);
        } else if !auto_confirm {
            let msg = if outcome.journal_purged {
                "WinShred erfolgreich abgeschlossen.\nAlle ausgewählten Elemente wurden unwiderruflich vernichtet und das NTFS USN-Journal wurde bereinigt."
            } else {
                "WinShred erfolgreich abgeschlossen.\nAlle ausgewählten Elemente wurden unwiderruflich vernichtet."
            };
            show_message_box(
                msg,
                "WinShred - Fertiggestellt",
                MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND,
            );
        }
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
        shred_file(&test_file, 1).unwrap();
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
        let mut errs = 0;
        shred_recursive(&temp_dir, 1, &mut errs);
        assert_eq!(errs, 0);
        assert!(!temp_dir.exists());
    }

    #[test]
    fn test_junction_protection() {
        let temp_dir = env::temp_dir().join(format!("winshred_junc_test_{}", std::process::id()));
        let target_dir = temp_dir.join("target_folder");
        let link_dir = temp_dir.join("junction_link");
        fs::create_dir_all(&target_dir).unwrap();

        let secret_file = target_dir.join("should_survive.txt");
        fs::write(&secret_file, b"Do not delete me!").unwrap();

        // Junction erstellen (mklink /J)
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J", &link_dir.to_string_lossy(), &target_dir.to_string_lossy()])
            .status()
            .unwrap();
        assert!(status.success());

        assert!(link_dir.exists());
        assert!(secret_file.exists());

        // Vernichte die Junction
        let mut errs = 0;
        shred_recursive(&link_dir, 1, &mut errs);
        assert_eq!(errs, 0);

        // Die Junction selbst muss gelöscht sein!
        assert!(!link_dir.exists(), "Die Junction selbst sollte gelöscht sein.");

        // DAS ZIELVERZEICHNIS UND DIE DATEI DÜRFEN NICHT GELÖSCHT WORDEN SEIN!
        assert!(secret_file.exists(), "KRITISCHER SCHUTZ: Die Zieldatei darf NIEMALS gelöscht werden!");
        assert!(target_dir.exists(), "KRITISCHER SCHUTZ: Das Zielverzeichnis darf NIEMALS gelöscht werden!");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_dangerous_path_protection() {
        assert!(is_dangerous_or_root_path(Path::new("C:\\")));
        assert!(is_dangerous_or_root_path(Path::new("C:")));
        assert!(is_dangerous_or_root_path(Path::new("D:\\")));
        assert!(is_dangerous_or_root_path(Path::new("")));
        assert!(is_dangerous_or_root_path(Path::new("C:\\Windows")));
        assert!(is_dangerous_or_root_path(Path::new("C:\\Windows\\System32")));
    }

    #[test]
    fn test_file_symlink_detection() {
        let temp_dir = env::temp_dir().join(format!("winshred_fsym_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        let target_file = temp_dir.join("target.txt");
        let link_file = temp_dir.join("symlink.txt");
        fs::write(&target_file, b"secret data").unwrap();

        match std::os::windows::fs::symlink_file(&target_file, &link_file) {
            Ok(()) => {
                let meta = fs::symlink_metadata(&link_file).unwrap();
                println!("file symlink is_symlink: {}", meta.file_type().is_symlink());
                assert!(meta.file_type().is_symlink());
                let _ = fs::remove_file(&link_file);
            }
            Err(e) if e.raw_os_error() == Some(1314) => {
                // ERROR_PRIVILEGE_NOT_HELD ohne Entwicklermodus / SeCreateSymbolicLinkPrivilege
                eprintln!("Hinweis: Symlink-Test mangels SeCreateSymbolicLinkPrivilege (1314) uebersprungen.");
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&temp_dir);
                panic!("Unerwarteter Fehler beim Erstellen des Datei-Symlinks: {}", e);
            }
        }
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_trailing_slash_shred() {
        let temp_dir = env::temp_dir().join(format!("winshred_slash_test_{}", std::process::id()));
        let sub_dir = temp_dir.join("folder_with_slash");
        fs::create_dir_all(&sub_dir).unwrap();
        let file = sub_dir.join("data.txt");
        fs::write(&file, b"content").unwrap();

        let mut slash_path = sub_dir.to_string_lossy().to_string();
        slash_path.push('\\');
        let p = PathBuf::from(slash_path);

        let mut errs = 0;
        shred_recursive(&p, 1, &mut errs);
        assert_eq!(errs, 0);
        assert!(!sub_dir.exists());
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_drive_media_type_detection() {
        let probe_path = env::temp_dir();
        let media_probe = get_drive_media_type(&probe_path);
        println!("Medientyp Temp/Systemlaufwerk ({:?}): {:?}", probe_path, media_probe);
        assert_ne!(media_probe, DriveMediaType::Unknown);

        let a_drive = Path::new("A:\\");
        if a_drive.exists() {
            let media_a = get_drive_media_type(a_drive);
            println!("Medientyp A: {:?}", media_a);
            assert_ne!(media_a, DriveMediaType::Unknown);
        }
    }

    #[test]
    fn test_resident_mft_detection_and_shred() {
        let temp_dir = env::temp_dir().join(format!("winshred_res_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        let tiny_file = temp_dir.join("tiny_secret.txt");
        fs::write(&tiny_file, b"Sehr kleine geheime Datei").unwrap();

        {
            let file = OpenOptions::new().read(true).open(&tiny_file).unwrap();
            let is_res = is_file_resident_in_mft(&file, 25);
            println!("Tiny file is resident in MFT: {}", is_res);
            assert!(is_res, "Kleine Datei unter 100 Bytes sollte als resident erkannt werden");
        }

        shred_file(&tiny_file, 1).unwrap();
        assert!(!tiny_file.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_neutralize_timestamps() {
        let temp_dir = env::temp_dir().join(format!("winshred_ts_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        let test_file = temp_dir.join("ts_secret.txt");
        fs::write(&test_file, b"Timestomp Test").unwrap();

        {
            let file = OpenOptions::new().write(true).open(&test_file).unwrap();
            neutralize_timestamps(&file);
        }

        let meta = fs::metadata(&test_file).unwrap();
        let modified = meta.modified().unwrap();
        println!("Neutralisierter Zeitstempel: {:?}", modified);

        let _ = fs::remove_file(&test_file);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_secure_single_rename() {
        let temp_dir = env::temp_dir().join(format!("winshred_ren_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        let test_file = temp_dir.join("original_secret_name.docx");
        fs::write(&test_file, b"Content").unwrap();

        let renamed = secure_single_rename(&test_file).unwrap();
        assert!(!test_file.exists(), "Originalname darf nach Rename nicht mehr existieren");
        assert!(renamed.exists(), "Neuer Dateiname muss existieren");
        assert!(renamed.to_string_lossy().contains("shred_"));

        let _ = fs::remove_file(&renamed);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_shred_reporter_progress() {
        let temp_dir = env::temp_dir().join(format!("winshred_rep_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        let file1 = temp_dir.join("test_rep_1.txt");
        fs::write(&file1, b"Hello world! This is a test for shred progress reporter.").unwrap();

        struct TestReporter {
            items_started: usize,
            items_done: usize,
            bytes_written: u64,
        }
        impl ShredReporter for TestReporter {
            fn on_item_start(&mut self, _path: &Path, _is_dir: bool) {
                self.items_started += 1;
            }
            fn on_bytes_written(&mut self, bytes: u64) {
                self.bytes_written += bytes;
            }
            fn on_item_done(&mut self, _path: &Path, _is_dir: bool) {
                self.items_done += 1;
            }
        }

        let mut rep = TestReporter {
            items_started: 0,
            items_done: 0,
            bytes_written: 0,
        };

        assert!(file1.exists());
        shred_file_with_reporter(&file1, 1, &mut rep).unwrap();
        assert!(!file1.exists());
        assert_eq!(rep.items_started, 1);
        assert_eq!(rep.items_done, 1);
        assert!(rep.bytes_written > 0);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_shred_reporter_cancel() {
        let temp_dir = env::temp_dir().join(format!("winshred_cancel_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();

        let file1 = temp_dir.join("test_cancel.txt");
        fs::write(&file1, b"Confidential content that should survive cancellation").unwrap();

        struct CancelReporter;
        impl ShredReporter for CancelReporter {
            fn is_cancelled(&self) -> bool { true }
        }

        let mut rep = CancelReporter;
        let res = shred_file_with_reporter(&file1, 1, &mut rep);
        assert!(res.is_err());
        assert!(file1.exists(), "Datei darf bei Abbruch nicht gelöscht werden");

        let _ = fs::remove_file(&file1);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_usn_journal_structs_and_constants() {
        assert_eq!(std::mem::size_of::<USN_JOURNAL_DATA_V0>(), 56);
        assert_eq!(std::mem::size_of::<DELETE_USN_JOURNAL_DATA>(), 16);
        assert_eq!(std::mem::size_of::<CREATE_USN_JOURNAL_DATA>(), 16);
        assert_eq!(std::mem::size_of::<LUID>(), 8);
        assert_eq!(std::mem::size_of::<TOKEN_PRIVILEGES>(), 16);
        assert_eq!(FSCTL_DELETE_USN_JOURNAL, 0x000900F8);
        assert_eq!(FSCTL_QUERY_USN_JOURNAL, 0x000900F4);
        assert_eq!(FSCTL_CREATE_USN_JOURNAL, 0x000900E7);
        assert_eq!(FSCTL_QUERY_ALLOCATED_RANGES, 0x000940CF);
        assert_eq!(USN_DELETE_FLAG_DELETE | USN_DELETE_FLAG_NOTIFY, 0x00000003);
        assert_eq!(ERROR_JOURNAL_DELETE_IN_PROGRESS, 1178);

        // Relative Pfade müssen über extract_drive_letter trotzdem ein Laufwerk liefern
        let rel = Path::new("Cargo.toml");
        let drive = extract_drive_letter(rel);
        assert!(drive.is_some(), "Laufwerksbuchstabe für relativen Pfad sollte ermittelt werden");
    }

    #[test]
    fn test_non_resident_small_file_not_mft_resident() {
        let temp_dir = env::temp_dir().join(format!("winshred_nonres_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        let fpath = temp_dir.join("shrunk.bin");

        {
            let mut f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&fpath)
                .unwrap();
            f.write_all(&vec![b'Z'; 16384]).unwrap();
            f.sync_all().unwrap();
            f.set_len(100).unwrap();
            f.sync_all().unwrap();
        }

        {
            let f = OpenOptions::new().read(true).open(&fpath).unwrap();
            assert!(
                !is_file_resident_in_mft(&f, 100),
                "Verkleinerte Datei mit belegtem Cluster darf nicht als resident eingestuft werden"
            );
        }

        shred_file(&fpath, 1).unwrap();
        assert!(!fpath.exists());
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_sparse_file_allocated_range_shred() {
        const FSCTL_SET_SPARSE: u32 = 0x000900C4;
        let temp_dir = env::temp_dir().join(format!("winshred_sparse_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        let sp = temp_dir.join("sparse_data.bin");

        {
            let mut f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&sp)
                .unwrap();
            let mut bret = 0u32;
            let ok = unsafe {
                DeviceIoControl(
                    f.as_raw_handle(),
                    FSCTL_SET_SPARSE,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    &mut bret,
                    std::ptr::null_mut(),
                )
            };
            if ok != 0 {
                f.set_len(8 * 1024 * 1024).unwrap();
                f.seek(SeekFrom::Start(4 * 1024 * 1024)).unwrap();
                f.write_all(&vec![0xAB; 4096]).unwrap();
                f.sync_all().unwrap();

                let ranges = query_allocated_ranges(&f, 8 * 1024 * 1024).unwrap();
                assert!(!ranges.is_empty(), "Mindestens ein allokierter Bereich bei 4 MB erwartet");
                assert!(
                    ranges.iter().any(|&(off, len)| off <= 4 * 1024 * 1024 && off + len >= 4 * 1024 * 1024 + 4096),
                    "Gefundene Bereiche sollten den Block bei 4 MB abdecken: {:?}",
                    ranges
                );
            }
        }

        shred_file(&sp, 1).unwrap();
        assert!(!sp.exists());
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_move_file_no_replace_prevents_overwrite() {
        let temp_dir = env::temp_dir().join(format!("winshred_noreplace_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        let f1 = temp_dir.join("source.txt");
        let f2 = temp_dir.join("existing.txt");
        fs::write(&f1, b"source").unwrap();
        fs::write(&f2, b"must_survive").unwrap();

        let err = move_file_no_replace(&f1, &f2).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&f2).unwrap(), "must_survive");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_readonly_and_ads_preserved_when_locked() {
        use std::os::windows::fs::OpenOptionsExt;

        let temp_dir = env::temp_dir().join(format!("winshred_lock_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        let fpath = temp_dir.join("locked_ro.txt");
        fs::write(&fpath, b"locked_payload").unwrap();

        let ads_path = format!("{}:keep_ads:$DATA", fpath.to_string_lossy());
        {
            let mut ads = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&ads_path)
                .unwrap();
            ads.write_all(b"ads_content").unwrap();
        }

        // Exklusives Handle halten (share_mode(0)), danach Read-Only setzen
        let lock_handle = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&fpath)
            .unwrap();

        let wide = path_to_wide(&fpath);
        let orig_attrs = unsafe { GetFileAttributesW(wide.as_ptr()) };
        unsafe { SetFileAttributesW(wide.as_ptr(), orig_attrs | FILE_ATTRIBUTE_READONLY) };

        let res = shred_file(&fpath, 1);
        assert!(res.is_err(), "Gesperrte Datei darf nicht geschreddert werden");

        let attrs_after = unsafe { GetFileAttributesW(wide.as_ptr()) };
        assert_ne!(attrs_after & FILE_ATTRIBUTE_READONLY, 0, "ReadOnly-Attribut muss erhalten bleiben");

        drop(lock_handle);
        assert_eq!(fs::read_to_string(&ads_path).unwrap(), "ads_content");

        unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_NORMAL) };
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_classify_path_safety_rules() {
        assert!(matches!(classify_path_safety(Path::new("C:\\")), PathSafetyLevel::Blocked(_)));
        assert!(matches!(classify_path_safety(Path::new(r"\\server\share")), PathSafetyLevel::Blocked(_)));
        assert!(matches!(classify_path_safety(Path::new(r"\\server\share\sub\file.txt")), PathSafetyLevel::Safe));
        assert!(matches!(classify_path_safety(Path::new(r"C:\Windows")), PathSafetyLevel::Blocked(_)));
        assert!(matches!(classify_path_safety(Path::new(r"C:\Windows\System32")), PathSafetyLevel::Blocked(_)));
        assert!(matches!(classify_path_safety(Path::new(r"C:\Windows\SysWOW64\cmd.exe")), PathSafetyLevel::Blocked(_)));
        assert!(matches!(classify_path_safety(Path::new(r"C:\Windows\Temp\test.log")), PathSafetyLevel::Important(_)));
        assert!(matches!(classify_path_safety(Path::new(r"C:\Windows\System32_backup")), PathSafetyLevel::Important(_)));
        assert!(matches!(classify_path_safety(Path::new(r"D:\System Volume Information")), PathSafetyLevel::Blocked(_)));
        assert!(matches!(classify_path_safety(Path::new(r"C:\ProgramData")), PathSafetyLevel::Blocked(_)));
        assert!(matches!(classify_path_safety(Path::new(r"C:\ProgramData\MyApp")), PathSafetyLevel::Important(_)));

        if let Ok(profile) = env::var("USERPROFILE") {
            assert!(matches!(classify_path_safety(Path::new(&profile)), PathSafetyLevel::Important(_)));
            assert!(matches!(
                classify_path_safety(&Path::new(&profile).join("Desktop")),
                PathSafetyLevel::Important(_)
            ));
            assert!(matches!(
                classify_path_safety(&Path::new(&profile).join("Desktop").join("einzeldatei.txt")),
                PathSafetyLevel::Safe
            ));
        }

        let safe_tmp = env::temp_dir().join("winshred_safe_dummy.txt");
        assert!(matches!(classify_path_safety(&safe_tmp), PathSafetyLevel::Safe));
    }

    #[test]
    fn test_junction_to_windows_is_important_not_blocked() {
        let temp_dir = env::temp_dir().join(format!("winshred_winjunc_{}", std::process::id()));
        let link_dir = temp_dir.join("win_link");
        fs::create_dir_all(&temp_dir).unwrap();

        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J", &link_dir.to_string_lossy(), r"C:\Windows"])
            .status()
            .unwrap();
        assert!(status.success());

        let level = classify_path_safety(&link_dir);
        assert!(
            matches!(level, PathSafetyLevel::Important(_)),
            "Junction auf C:\\Windows darf nicht blockiert werden, sondern muss Important liefern: {:?}",
            level
        );

        let mut errs = 0;
        shred_recursive(&link_dir, 1, &mut errs);
        assert_eq!(errs, 0);
        assert!(!link_dir.exists());
        assert!(Path::new(r"C:\Windows").exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_quote_windows_cli_arg_trailing_backslash() {
        assert_eq!(quote_windows_cli_arg(r"C:\Simple\Path"), r"C:\Simple\Path");
        assert_eq!(quote_windows_cli_arg(r"C:\Test Ordner\"), r#""C:\Test Ordner\\""#);
        assert_eq!(quote_windows_cli_arg(""), r#""""#);
        assert_eq!(quote_windows_cli_arg(r#"a\"b"#), r#""a\\\"b""#);
    }

    #[test]
    fn test_cli_parsing_flags_and_errors() {
        let ok = parse_cli_args(["-y", "--quiet", "--passes", "3", "-p", "--", "-dash-file.txt"]).unwrap();
        assert!(ok.auto_confirm);
        assert!(ok.is_quiet);
        assert!(ok.purge_journal);
        assert_eq!(ok.custom_passes, Some(3));
        assert_eq!(ok.raw_paths, vec![PathBuf::from("-dash-file.txt")]);

        let help = parse_cli_args(["--help"]).unwrap();
        assert!(help.show_help);

        let eq = parse_cli_args(["--passes=5", "--filelist=C:\\tmp\\list.txt"]).unwrap();
        assert_eq!(eq.custom_passes, Some(5));
        assert_eq!(eq.filelists, vec![PathBuf::from(r"C:\tmp\list.txt")]);

        assert!(parse_cli_args(["--passes", "0", "a.txt"]).is_err());
        assert!(parse_cli_args(["--passes", "abc"]).is_err());
        assert!(parse_cli_args(["--passes"]).is_err());
        assert!(parse_cli_args(["--filelist"]).is_err());
        assert!(parse_cli_args(["--unbekannt"]).is_err());
    }

    #[test]
    fn test_filelist_reading_and_cleanup() {
        let temp_dir = env::temp_dir().join(format!("winshred_filelist_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        let list_path = temp_dir.join("items.lst");
        fs::write(&list_path, "\u{feff}C:\\Pfad Eins\\a.txt\n\n  D:\\Zwei\\b.bin  \n").unwrap();

        let paths = read_paths_from_filelist(&list_path, true).unwrap();
        assert_eq!(
            paths,
            vec![PathBuf::from(r"C:\Pfad Eins\a.txt"), PathBuf::from(r"D:\Zwei\b.bin")]
        );
        assert!(!list_path.exists());
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_count_items_and_bytes_cluster_accuracy() {
        let temp_dir = env::temp_dir().join(format!("winshred_count_test_{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        // 5000 Bytes -> nicht-resident, belegt auf NTFS mindestens 2 Cluster (>= 8192 Bytes)
        let big_file = temp_dir.join("nonresident.bin");
        fs::write(&big_file, vec![0x42u8; 5000]).unwrap();
        // 15 Bytes -> resident in MFT -> exakt 15 Bytes
        let tiny_file = temp_dir.join("tiny.txt");
        fs::write(&tiny_file, b"123456789012345").unwrap();

        let cancel = AtomicBool::new(false);
        let is_ssd_tmp = matches!(get_drive_media_type(&temp_dir), DriveMediaType::SsdNvme { .. });
        let (items, bytes) = count_items_and_bytes(std::slice::from_ref(&temp_dir), 3, &cancel);
        assert_eq!(items, 3); // temp_dir + big_file + tiny_file

        let expected_big = {
            let f = OpenOptions::new().read(true).open(&big_file).unwrap();
            let w = compute_non_resident_wipe_len(&big_file, &f, 5000);
            w * if is_ssd_tmp { 1 } else { 3 }
        };
        assert_eq!(bytes, expected_big + 15);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

