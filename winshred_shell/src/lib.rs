use std::ffi::{c_void, OsStr};
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const IID_IUNKNOWN: GUID = GUID {
    data1: 0x00000000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

pub const IID_ICLASSFACTORY: GUID = GUID {
    data1: 0x00000001,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

pub const IID_IEXPLORERCOMMAND: GUID = GUID {
    data1: 0xa08ce4d0,
    data2: 0xfa25,
    data3: 0x44ab,
    data4: [0xb5, 0x7c, 0xc7, 0xb1, 0xc3, 0x23, 0xe0, 0xb9],
};

pub const CLSID_WINSHREDCOMMAND: GUID = GUID {
    data1: 0x8a76b81c,
    data2: 0x0a87,
    data3: 0x4348,
    data4: [0x9f, 0x7c, 0x79, 0xf8, 0x81, 0x5e, 0x7c, 0x81],
};

pub type HRESULT = i32;
pub const S_OK: HRESULT = 0;
pub const S_FALSE: HRESULT = 1;
pub const E_NOINTERFACE: HRESULT = -2147467262;
pub const E_POINTER: HRESULT = -2147467261;
pub const E_FAIL: HRESULT = -2147467259;
pub const E_NOTIMPL: HRESULT = -2147467263;
pub const CLASS_E_NOAGGREGATION: HRESULT = -2147221232;

static HMODULE_INSTANCE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Windows DLL Entry Point.
///
/// # Safety
/// Wird ausschließlich vom Windows-Loader beim Laden/Entladen der DLL aufgerufen.
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    h_inst: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    if reason == 1 {
        // DLL_PROCESS_ATTACH
        HMODULE_INSTANCE.store(h_inst, Ordering::SeqCst);
    }
    1
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemAlloc(cb: usize) -> *mut c_void;
    fn CoTaskMemFree(pv: *mut c_void);
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleFileNameW(
        h_module: *mut c_void,
        lp_filename: *mut u16,
        n_size: u32,
    ) -> u32;
}

static SERVER_LOCKS: AtomicU32 = AtomicU32::new(0);
static FILELIST_SEQ: AtomicU32 = AtomicU32::new(0);

fn dec_server_locks() {
    let _ = SERVER_LOCKS.try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
        Some(v.saturating_sub(1))
    });
}

/// Sucht eine zugehörige Datei (`winshred.exe` / `winshred.ico`) zuerst neben dieser DLL
/// und fällt bei Bedarf auf das Standard-Installationsverzeichnis zurück.
fn find_companion_file(filename: &str) -> Option<PathBuf> {
    let hmod = HMODULE_INSTANCE.load(Ordering::SeqCst);
    if !hmod.is_null() {
        let mut mod_buf = [0u16; 1024];
        let len = unsafe { GetModuleFileNameW(hmod, mod_buf.as_mut_ptr(), mod_buf.len() as u32) };
        if len > 0 {
            let dll_path = String::from_utf16_lossy(&mod_buf[..len as usize]);
            if let Some(dir) = PathBuf::from(dll_path).parent() {
                let candidate = dir.join(filename);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    let fallback = PathBuf::from(r"C:\Program Files\WinShred").join(filename);
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

/// Berechnet die benötigte Kommandozeilenlänge in UTF-16-Code-Units inkl. Quoting-Reserve.
fn estimate_utf16_cmdline_len(exe_path: &Path, args: &[String]) -> usize {
    let mut total = exe_path.as_os_str().encode_wide().count() + 4;
    for a in args {
        total += a.encode_utf16().count() + 4;
    }
    total
}

fn alloc_co_string(s: &str) -> *mut u16 {
    let wide: Vec<u16> = OsStr::new(s).encode_wide().chain(Some(0)).collect();
    let bytes = wide.len() * 2;
    unsafe {
        let mem = CoTaskMemAlloc(bytes) as *mut u16;
        if !mem.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), mem, wide.len());
        }
        mem
    }
}

// ==========================================
// IShellItemArray and IShellItem definitions
// ==========================================
#[repr(C)]
struct IShellItemArrayVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    bind_to_handler: *const c_void,
    get_property_store: *const c_void,
    get_property_description_list: *const c_void,
    get_attributes: *const c_void,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    get_item_at: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IShellItemVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    bind_to_handler: *const c_void,
    get_parent: *const c_void,
    get_display_name: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> HRESULT,
}

const SIGDN_FILESYSPATH: u32 = 0x80058000;

// ==========================================
// IExplorerCommand implementation
// ==========================================
#[repr(C)]
struct IExplorerCommandVtbl {
    query_interface: unsafe extern "system" fn(*mut WinShredCommand, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut WinShredCommand) -> u32,
    release: unsafe extern "system" fn(*mut WinShredCommand) -> u32,
    get_title: unsafe extern "system" fn(*mut WinShredCommand, *mut c_void, *mut *mut u16) -> HRESULT,
    get_icon: unsafe extern "system" fn(*mut WinShredCommand, *mut c_void, *mut *mut u16) -> HRESULT,
    get_tool_tip: unsafe extern "system" fn(*mut WinShredCommand, *mut c_void, *mut *mut u16) -> HRESULT,
    get_canonical_name: unsafe extern "system" fn(*mut WinShredCommand, *mut GUID) -> HRESULT,
    get_state: unsafe extern "system" fn(*mut WinShredCommand, *mut c_void, i32, *mut u32) -> HRESULT,
    invoke: unsafe extern "system" fn(*mut WinShredCommand, *mut c_void, *mut c_void) -> HRESULT,
    get_flags: unsafe extern "system" fn(*mut WinShredCommand, *mut u32) -> HRESULT,
    enum_sub_commands: unsafe extern "system" fn(*mut WinShredCommand, *mut *mut c_void) -> HRESULT,
}

static COMMAND_VTBL: IExplorerCommandVtbl = IExplorerCommandVtbl {
    query_interface: cmd_query_interface,
    add_ref: cmd_add_ref,
    release: cmd_release,
    get_title: cmd_get_title,
    get_icon: cmd_get_icon,
    get_tool_tip: cmd_get_tool_tip,
    get_canonical_name: cmd_get_canonical_name,
    get_state: cmd_get_state,
    invoke: cmd_invoke,
    get_flags: cmd_get_flags,
    enum_sub_commands: cmd_enum_sub_commands,
};

#[repr(C)]
struct WinShredCommand {
    vtbl: &'static IExplorerCommandVtbl,
    ref_count: AtomicU32,
}

impl WinShredCommand {
    fn new() -> *mut WinShredCommand {
        let obj = Box::new(WinShredCommand {
            vtbl: &COMMAND_VTBL,
            ref_count: AtomicU32::new(1),
        });
        SERVER_LOCKS.fetch_add(1, Ordering::SeqCst);
        Box::into_raw(obj)
    }
}

unsafe extern "system" fn cmd_query_interface(
    this: *mut WinShredCommand,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    let guid = *riid;
    if guid == IID_IUNKNOWN || guid == IID_IEXPLORERCOMMAND {
        *ppv = this as *mut c_void;
        cmd_add_ref(this);
        S_OK
    } else {
        *ppv = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn cmd_add_ref(this: *mut WinShredCommand) -> u32 {
    let obj = &*this;
    obj.ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn cmd_release(this: *mut WinShredCommand) -> u32 {
    if this.is_null() {
        return 0;
    }
    let obj = &*this;
    let count = obj.ref_count.fetch_sub(1, Ordering::SeqCst) - 1;
    if count == 0 {
        dec_server_locks();
        drop(Box::from_raw(this));
    }
    count
}

unsafe extern "system" fn cmd_get_title(
    _this: *mut WinShredCommand,
    _items: *mut c_void,
    ppsz_name: *mut *mut u16,
) -> HRESULT {
    if ppsz_name.is_null() {
        return E_POINTER;
    }
    *ppsz_name = alloc_co_string("Mit WinShred sicher vernichten");
    if (*ppsz_name).is_null() {
        E_FAIL
    } else {
        S_OK
    }
}

unsafe extern "system" fn cmd_get_icon(
    _this: *mut WinShredCommand,
    _items: *mut c_void,
    ppsz_icon: *mut *mut u16,
) -> HRESULT {
    if ppsz_icon.is_null() {
        return E_POINTER;
    }
    if let Some(p) = find_companion_file("winshred.ico") {
        *ppsz_icon = alloc_co_string(&p.to_string_lossy());
    } else {
        *ppsz_icon = alloc_co_string("imageres.dll,-5308");
    }
    if (*ppsz_icon).is_null() {
        E_FAIL
    } else {
        S_OK
    }
}

unsafe extern "system" fn cmd_get_tool_tip(
    _this: *mut WinShredCommand,
    _items: *mut c_void,
    ppsz_info: *mut *mut u16,
) -> HRESULT {
    if ppsz_info.is_null() {
        return E_POINTER;
    }
    *ppsz_info = alloc_co_string("Dateien und Ordner unwiderruflich vernichten");
    if (*ppsz_info).is_null() {
        E_FAIL
    } else {
        S_OK
    }
}

unsafe extern "system" fn cmd_get_canonical_name(
    _this: *mut WinShredCommand,
    pguid_name: *mut GUID,
) -> HRESULT {
    if pguid_name.is_null() {
        return E_POINTER;
    }
    *pguid_name = CLSID_WINSHREDCOMMAND;
    S_OK
}

unsafe extern "system" fn cmd_get_state(
    _this: *mut WinShredCommand,
    _items: *mut c_void,
    _ok_slow: i32,
    p_cmd_state: *mut u32,
) -> HRESULT {
    if p_cmd_state.is_null() {
        return E_POINTER;
    }
    *p_cmd_state = 0; // ECS_ENABLED
    S_OK
}

unsafe extern "system" fn cmd_get_flags(
    _this: *mut WinShredCommand,
    p_flags: *mut u32,
) -> HRESULT {
    if p_flags.is_null() {
        return E_POINTER;
    }
    *p_flags = 0; // ECF_DEFAULT
    S_OK
}

unsafe extern "system" fn cmd_enum_sub_commands(
    _this: *mut WinShredCommand,
    pp_enum: *mut *mut c_void,
) -> HRESULT {
    if !pp_enum.is_null() {
        *pp_enum = std::ptr::null_mut();
    }
    E_NOTIMPL
}

unsafe extern "system" fn cmd_invoke(
    _this: *mut WinShredCommand,
    items: *mut c_void,
    _pbc: *mut c_void,
) -> HRESULT {
    let mut paths: Vec<String> = Vec::new();

    if !items.is_null() {
        let array = items as *mut *mut IShellItemArrayVtbl;
        let vtbl = &**array;
        let mut count: u32 = 0;
        if (vtbl.get_count)(items, &mut count) == S_OK {
            for i in 0..count {
                let mut shell_item: *mut c_void = std::ptr::null_mut();
                if (vtbl.get_item_at)(items, i, &mut shell_item) == S_OK && !shell_item.is_null() {
                    let item_ptr = shell_item as *mut *mut IShellItemVtbl;
                    let item_vtbl = &**item_ptr;
                    let mut path_ptr: *mut u16 = std::ptr::null_mut();
                    if (item_vtbl.get_display_name)(shell_item, SIGDN_FILESYSPATH, &mut path_ptr) == S_OK && !path_ptr.is_null() {
                        let mut len = 0;
                        while *path_ptr.add(len) != 0 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(path_ptr, len);
                        let s = String::from_utf16_lossy(slice);
                        paths.push(s);
                        CoTaskMemFree(path_ptr as *mut c_void);
                    }
                    (item_vtbl.release)(shell_item);
                }
            }
        }
    }

    // Verhindere Start ohne Argumente (wuerde sonst Setup-Assistent starten!)
    if paths.is_empty() {
        return S_OK;
    }

    // Ermittle den Pfad zur winshred.exe (primär neben dieser DLL, sonst Installationsordner)
    let exe_path = find_companion_file("winshred.exe")
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\WinShred\winshred.exe"));

    // Windows Command-Line Limit (32.767 UTF-16-Code-Units) beachten:
    // Bei großen Auswahlmengen per temporärer Dateiliste übergeben, damit nur ein einzelner Prozess startet.
    const MAX_CMDLINE_UTF16: usize = 30000;
    if estimate_utf16_cmdline_len(&exe_path, &paths) <= MAX_CMDLINE_UTF16 {
        let mut cmd = Command::new(&exe_path);
        cmd.arg("--");
        for p in &paths {
            cmd.arg(p);
        }
        return if cmd.spawn().is_ok() { S_OK } else { E_FAIL };
    }

    let seq = FILELIST_SEQ.fetch_add(1, Ordering::Relaxed);
    let list_path = std::env::temp_dir().join(format!(
        "winshred_invoke_{}_{}.lst",
        std::process::id(),
        seq
    ));
    let content = paths.join("\n");
    if fs::write(&list_path, content.as_bytes()).is_ok() {
        let mut cmd = Command::new(&exe_path);
        cmd.arg("--cleanup-filelist").arg(&list_path);
        if cmd.spawn().is_ok() {
            return S_OK;
        }
        let _ = fs::remove_file(&list_path);
        return E_FAIL;
    }

    E_FAIL
}

// ==========================================
// IClassFactory implementation
// ==========================================
#[repr(C)]
struct IClassFactoryVtbl {
    query_interface: unsafe extern "system" fn(*mut WinShredClassFactory, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut WinShredClassFactory) -> u32,
    release: unsafe extern "system" fn(*mut WinShredClassFactory) -> u32,
    create_instance: unsafe extern "system" fn(*mut WinShredClassFactory, *mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    lock_server: unsafe extern "system" fn(*mut WinShredClassFactory, i32) -> HRESULT,
}

static FACTORY_VTBL: IClassFactoryVtbl = IClassFactoryVtbl {
    query_interface: factory_query_interface,
    add_ref: factory_add_ref,
    release: factory_release,
    create_instance: factory_create_instance,
    lock_server: factory_lock_server,
};

#[repr(C)]
struct WinShredClassFactory {
    vtbl: &'static IClassFactoryVtbl,
    ref_count: AtomicU32,
}

unsafe extern "system" fn factory_query_interface(
    this: *mut WinShredClassFactory,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    let guid = *riid;
    if guid == IID_IUNKNOWN || guid == IID_ICLASSFACTORY {
        *ppv = this as *mut c_void;
        factory_add_ref(this);
        S_OK
    } else {
        *ppv = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn factory_add_ref(this: *mut WinShredClassFactory) -> u32 {
    let obj = &*this;
    obj.ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn factory_release(this: *mut WinShredClassFactory) -> u32 {
    if this.is_null() {
        return 0;
    }
    let obj = &*this;
    let count = obj.ref_count.fetch_sub(1, Ordering::SeqCst) - 1;
    if count == 0 {
        dec_server_locks();
        drop(Box::from_raw(this));
    }
    count
}

unsafe extern "system" fn factory_create_instance(
    _this: *mut WinShredClassFactory,
    punk_outer: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if !punk_outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }

    let cmd = WinShredCommand::new();
    let hr = cmd_query_interface(cmd, riid, ppv);
    cmd_release(cmd);
    hr
}

unsafe extern "system" fn factory_lock_server(_this: *mut WinShredClassFactory, lock: i32) -> HRESULT {
    if lock != 0 {
        SERVER_LOCKS.fetch_add(1, Ordering::SeqCst);
    } else {
        dec_server_locks();
    }
    S_OK
}

// ==========================================
// DLL Exports
// ==========================================

/// COM Class Factory Export.
///
/// # Safety
/// `rclsid`, `riid` und `ppv` müssen gültige Zeiger gemäß COM-Spezifikation sein.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();
    if rclsid.is_null() || riid.is_null() {
        return E_POINTER;
    }

    if *rclsid != CLSID_WINSHREDCOMMAND {
        return -2147221231; // CLASS_E_CLASSNOTAVAILABLE
    }

    let factory = Box::into_raw(Box::new(WinShredClassFactory {
        vtbl: &FACTORY_VTBL,
        ref_count: AtomicU32::new(1),
    }));
    SERVER_LOCKS.fetch_add(1, Ordering::SeqCst);

    let hr = factory_query_interface(factory, riid, ppv);
    factory_release(factory);
    hr
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if SERVER_LOCKS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_server_underflow_protection() {
        SERVER_LOCKS.store(0, Ordering::SeqCst);
        unsafe {
            assert_eq!(factory_lock_server(std::ptr::null_mut(), 0), S_OK);
        }
        assert_eq!(SERVER_LOCKS.load(Ordering::SeqCst), 0);
        assert_eq!(DllCanUnloadNow(), S_OK);

        unsafe {
            assert_eq!(factory_lock_server(std::ptr::null_mut(), 1), S_OK);
        }
        assert_eq!(DllCanUnloadNow(), S_FALSE);
        unsafe {
            assert_eq!(factory_lock_server(std::ptr::null_mut(), 0), S_OK);
        }
        assert_eq!(DllCanUnloadNow(), S_OK);
    }

    #[test]
    fn test_utf16_cmdline_estimation() {
        let exe = PathBuf::from(r"C:\Program Files\WinShred\winshred.exe");
        let args = vec![r"C:\Temp\äöü_datei.txt".to_string()];
        let len = estimate_utf16_cmdline_len(&exe, &args);
        assert!(len > 40 && len < 100);
    }
}
