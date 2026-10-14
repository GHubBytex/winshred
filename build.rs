fn main() {
    println!("cargo:rerun-if-changed=assets/winshred.ico");
    println!("cargo:rerun-if-changed=resources/app.rc");
    println!("cargo:rerun-if-changed=resources/app.manifest");

    let rc_candidates = [
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\rc.exe",
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\rc.exe",
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\rc.exe",
    ];

    let rc_path = rc_candidates.iter().find(|p| std::path::Path::new(p).exists());

    if let Some(&rc_exe) = rc_path {
        if let Ok(out_dir) = std::env::var("OUT_DIR") {
            let res_path = format!("{}/app.res", out_dir);
            let status = std::process::Command::new(rc_exe)
                .args(["/fo", &res_path, "resources/app.rc"])
                .status();

            if let Ok(s) = status {
                if s.success() {
                    println!("cargo:rustc-link-arg={}", res_path);
                }
            }
        }
    }
}
