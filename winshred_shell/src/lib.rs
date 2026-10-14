use std::ffi::{c_void, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

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
pub const E_NOINTERFACE: HRESULT = -2147467262; // 0x80004002
pub const E_POINTER: HRESULT = -2147467261;     // 0x80004003
pub const E_FAIL: HRESULT = -2147467259;        // 0x80004005
pub const E_NOTIMPL: HRESULT = -2147467263;     // 0x80004001
pub const CLASS_E_NOAGGREGATION: HRESULT = -2147221232; // 0x80040110

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
    let obj = &*this;
    let count = obj.ref_count.fetch_sub(1, Ordering::SeqCst) - 1;
    if count == 0 {
        SERVER_LOCKS.fetch_sub(1, Ordering::SeqCst);
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
    *ppsz_icon = alloc_co_string("imageres.dll,-5308");
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

    // Ermittle den Pfad zur winshred.exe (liegt neben dieser DLL)
    let mut exe_path = PathBuf::from("A:\\Code\\01 TEMP\\REA\\winshred\\target\\release\\winshred.exe");
    let mut mod_buf = [0u16; 1024];
    let len = GetModuleFileNameW(std::ptr::null_mut(), mod_buf.as_mut_ptr(), 1024);
    if len > 0 {
        let dll_path = String::from_utf16_lossy(&mod_buf[..len as usize]);
        let dir = PathBuf::from(dll_path).parent().map(|p| p.to_path_buf());
        if let Some(d) = dir {
            let candidate = d.join("winshred.exe");
            if candidate.exists() {
                exe_path = candidate;
            }
        }
    }

    let mut cmd = Command::new(&exe_path);
    for p in &paths {
        cmd.arg(p);
    }
    let _ = cmd.spawn();

    S_OK
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
    let obj = &*this;
    let count = obj.ref_count.fetch_sub(1, Ordering::SeqCst) - 1;
    if count == 0 {
        SERVER_LOCKS.fetch_sub(1, Ordering::SeqCst);
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
        SERVER_LOCKS.fetch_sub(1, Ordering::SeqCst);
    }
    S_OK
}

// ==========================================
// DLL Exports
// ==========================================
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }

    if *rclsid != CLSID_WINSHREDCOMMAND {
        *ppv = std::ptr::null_mut();
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
