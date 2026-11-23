use std::path::{Path, PathBuf};

fn find_rc_exe() -> Option<PathBuf> {
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => "x86",
        Ok("aarch64") => "arm64",
        _ => "x64",
    };

    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = std::fs::read_dir(kits_bin) {
        let mut versions: Vec<(Vec<u32>, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if !p.is_dir() {
                    return None;
                }
                let name = e.file_name().to_string_lossy().to_string();
                let parsed: Vec<u32> = name
                    .split('.')
                    .filter_map(|seg| seg.parse::<u32>().ok())
                    .collect();
                if parsed.is_empty() {
                    return None;
                }
                let rc = p.join(arch).join("rc.exe");
                rc.exists().then_some((parsed, rc))
            })
            .collect();
        versions.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, best)) = versions.into_iter().next() {
            return Some(best);
        }
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("rc.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn main() {
    println!("cargo:rerun-if-changed=winshred.ico");
    println!("cargo:rerun-if-changed=app.rc");

    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=Windows SDK rc.exe nicht gefunden – DLL-Icon wird nicht eingebettet.");
        return;
    };

    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let res_path = format!("{}/app.res", out_dir);
        let status = std::process::Command::new(&rc_exe)
            .args(["/fo", &res_path, "app.rc"])
            .status();

        match status {
            Ok(s) if s.success() => {
                println!("cargo:rustc-link-arg={}", res_path);
            }
            Ok(s) => {
                println!(
                    "cargo:warning=rc.exe ({:?}) fehlgeschlagen mit Status {:?}",
                    rc_exe,
                    s.code()
                );
            }
            Err(e) => {
                println!("cargo:warning=rc.exe ({:?}) konnte nicht gestartet werden: {}", rc_exe, e);
            }
        }
    }
}
