#![windows_subsystem = "windows"]

use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        hwnd: *mut std::ffi::c_void,
        lp_text: *const u16,
        lp_caption: *const u16,
        u_type: u32,
    ) -> i32;
}

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(
        h_algorithm: *mut std::ffi::c_void,
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

const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn FindFirstStreamW(
        lpFileName: *const u16,
        InfoLevel: u32,
        lpFindStreamData: *mut std::ffi::c_void,
        dwFlags: u32,
    ) -> *mut std::ffi::c_void;

    fn FindNextStreamW(
        hFindStream: *mut std::ffi::c_void,
        lpFindStreamData: *mut std::ffi::c_void,
    ) -> i32;

    fn FindClose(hFindFile: *mut std::ffi::c_void) -> i32;

    fn DeleteFileW(lpFileName: *const u16) -> i32;

    fn DeviceIoControl(
        hDevice: *mut std::ffi::c_void,
        dwIoControlCode: u32,
        lpInBuffer: *const std::ffi::c_void,
        nInBufferSize: u32,
        lpOutBuffer: *mut std::ffi::c_void,
        nOutBufferSize: u32,
        lpBytesReturned: *mut u32,
        lpOverlapped: *mut std::ffi::c_void,
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
const MB_ICONINFORMATION: u32 = 0x00000040;
const MB_ICONERROR: u32 = 0x00000010;
const IDYES: i32 = 6;

// FSCTL_FILE_LEVEL_TRIM = CTL_CODE(FILE_DEVICE_FILE_SYSTEM, 130, METHOD_BUFFERED, FILE_WRITE_DATA)
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

/// Ermittelt die Clustergröße des Dateisystems für Cluster-Slack-Berechnungen.
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
        4096 // Standard 4 KB für NTFS
    }
}

/// Sendet einen Deallocate/TRIM-Befehl an den Datenträger-Treiber (SSD / NVMe).
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

    let handle = file.as_raw_handle() as *mut std::ffi::c_void;
    unsafe {
        DeviceIoControl(
            handle,
            FSCTL_FILE_LEVEL_TRIM,
            &trim_input as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<FileLevelTrim>() as u32,
            &mut trim_output as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<FileLevelTrimOutput>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        );
    }
}

/// Überschreibt den Inhalt eines geöffneten Datei- oder Stream-Handles mit 3 Durchgängen,
/// flusht Caches, sendet SSD-TRIM und schneidet die Größe auf 0 ab.
fn wipe_open_file(file: &mut fs::File, wipe_len: u64) -> io::Result<()> {
    if wipe_len == 0 {
        return Ok(());
    }

    const CHUNK_SIZE: usize = 64 * 1024; // 64 KB Chunks
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

/// Sucht alle Alternate Data Streams (ADS) einer Datei.
fn get_file_streams(path: &Path) -> Vec<String> {
    let mut streams = Vec::new();
    let wide_path = path_to_wide(path);

    let mut stream_data: WIN32_FIND_STREAM_DATA = unsafe { std::mem::zeroed() };
    let handle = unsafe {
        FindFirstStreamW(
            wide_path.as_ptr(),
            0, // FindStreamInfoStandard
            &mut stream_data as *mut _ as *mut std::ffi::c_void,
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

        // "::$DATA" ist der Hauptstrom; wir interessieren uns hier für alle alternativen Ströme
        if stream_name != "::$DATA" && !stream_name.is_empty() {
            streams.push(stream_name);
        }

        let next = unsafe {
            FindNextStreamW(
                handle,
                &mut stream_data as *mut _ as *mut std::ffi::c_void,
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

/// Überschreibt und löscht alle Alternate Data Streams (ADS).
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

        // Stream über DeleteFileW entfernen
        let wide_stream_path = path_to_wide(&stream_path);
        unsafe {
            DeleteFileW(wide_stream_path.as_ptr());
        }
    }
    Ok(())
}

/// Anonymisiert Dateinamen / Ordnernamen durch mehrfaches Umbenennen vor dem endgültigen Löschen.
/// Dies überschreibt die Namensattribute ($FILE_NAME bzw. $INDEX_ROOT) im NTFS MFT-Record.
fn secure_multi_rename(path: &Path) -> io::Result<PathBuf> {
    let parent = match path.parent() {
        Some(p) => p,
        None => return Ok(path.to_path_buf()),
    };

    let mut current_path = path.to_path_buf();

    // Schreibschutz vor dem Umbenennen aufheben
    if let Ok(meta) = fs::metadata(&current_path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(&current_path, perms);
        }
    }

    let mut rand_bytes = [0u8; 12];
    let _ = fill_random_bytes(&mut rand_bytes);

    // Pass 1: Einheitliche Füllzeichen 'a'
    let pass1_name = format!(
        "shred_aaaa_{:02x}{:02x}{:02x}{:02x}.tmp",
        rand_bytes[0], rand_bytes[1], rand_bytes[2], rand_bytes[3]
    );
    let p1 = parent.join(pass1_name);
    if fs::rename(&current_path, &p1).is_ok() {
        current_path = p1;
    }

    // Pass 2: Einheitliche Füllzeichen '0'
    let pass2_name = format!(
        "shred_0000_{:02x}{:02x}{:02x}{:02x}.tmp",
        rand_bytes[4], rand_bytes[5], rand_bytes[6], rand_bytes[7]
    );
    let p2 = parent.join(pass2_name);
    if fs::rename(&current_path, &p2).is_ok() {
        current_path = p2;
    }

    // Pass 3: Reiner kryptografischer Zufallsname
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

    // 1. Alternate Data Streams (ADS) enumerieren und vernichten
    let _ = shred_alternate_streams(path);

    // 2. Hauptdatei öffnen
    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .open(path)?;

    let len = file.metadata()?.len();
    if len > 0 {
        // Cluster-Slack berücksichtigen: Überschreiben bis zur Cluster-Grenze
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

    // 3. Multi-Pass Rename zur MFT-Verschleierung
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
        // Ordner vor dem Löschen mehrfach anonymisieren
        let final_dir_path = secure_multi_rename(path)?;
        fs::remove_dir(final_dir_path)?;
    } else if path.is_file() {
        shred_file(path)?;
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        show_message_box(
            "WinShred - Sicheres Vernichten von Dateien & Ordnern\n\n\
            Verwendung:\n\
            1. Rechtsklick auf eine Datei oder einen Ordner -> 'Mit WinShred sicher vernichten'\n\
            2. Oder Datei direkt auf diese Anwendung ziehen.",
            "WinShred",
            MB_ICONINFORMATION,
        );
        return;
    }

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

    // Sicherheitsabfrage
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
                - Cluster-Slack & Alternate Data Streams (ADS) werden bereinigt\n\
                - SSD/NVMe TRIM Deallocate-Signale werden gesendet\n\
                - Mehrfache Namens-Anonymisierung im Dateisystem\n\n\
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

    // Durchführen
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

        // Alternate Data Stream anlegen
        let ads_path_str = format!("{}:ads_secret:$DATA", test_file.to_string_lossy());
        if let Ok(mut ads_file) = OpenOptions::new().write(true).create(true).open(&ads_path_str) {
            ads_file.write_all(b"Geheimer Alternate Data Stream").unwrap();
        }

        // Prüfen, dass Datei existiert
        assert!(test_file.exists());

        // Shreddern
        shred_file(&test_file).unwrap();

        // Prüfen, dass weder Datei noch ADS mehr existieren
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
