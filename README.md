<div align="center">

<img src="assets/icon.png" alt="WinShred Logo" width="128" height="128" />

# WinShred

**Forensisch sichere, unwiderrufliche Datei- und Ordnervernichtung für Windows 11.**  
Vollständig integriert in das moderne Windows 11 Kontextmenü (*"Mit WinShred sicher vernichten"*) auf der ersten Menüebene – ohne *"Weitere Optionen anzeigen"*.

</div>

---

## 📁 Projektstruktur

```text
winshred/
├── .gitignore
├── Cargo.toml                  # Hauptprojekt Konfiguration
├── Cargo.lock
├── README.md                   # Projektdokumentation
├── build.bat                   # 1-Klick-Buildskript für das All-in-One Binary
├── winshred.exe                # Portables, lauffähiges Endprodukt (Single-File Binary)
│
├── assets/                     # Grafiken & Bildmaterial
│   ├── icon.png                # Quellgrafik (2048x2048)
│   ├── winshred.ico            # Multi-Resolution Windows Icon (16x16 bis 256x256)
│   └── generate_icons.py       # Icon-Generator für alle Plattformen
│
├── packaging/                  # Windows 11 Sparse MSIX Paket
│   ├── package_src/            # Paket-Inhalte (AppxManifest.xml, PNG-Kacheln)
│   ├── winshred.cer            # Selbstsigniertes Zertifikat (TrustedPeople)
│   └── winshred.msix           # Signiertes Sparse-Paket für moderne Shell-Registrierung
│
├── resources/                  # Windows 32 Ressourcen & Manifest
│   ├── app.rc                  # Resource-Definition (Icon & Manifest)
│   └── app.manifest            # UAC Execution Level & Windows 10/11 Themes
│
├── scripts/                    # Wartungs- & Standalone-Skripte
│   ├── install_modern_menu.bat
│   ├── uninstall_modern_menu.bat
│   ├── install_classic_menu.bat
│   ├── uninstall_classic_menu.bat
│   ├── install_context_menu.ps1
│   ├── uninstall_context_menu.ps1
│   └── build_sparse_package.ps1
│
├── src/                        # Quellcode Hauptprogramm
│   └── main.rs                 # WinShred Core Engine, GUI Setup Wizard, CLI & Shredder
│
└── winshred_shell/             # Native Windows 11 IExplorerCommandHandler Shell Extension
    ├── Cargo.toml
    ├── build.rs
    ├── app.rc
    └── src/
        └── lib.rs              # COM-Server DLL Implementierung
```

---

## ⚡ Kernfunktionen

1. **All-in-One Single-File Executable (`winshred.exe`)**:
   - Keine externen DLLs, Installer oder MSI-Dateien nötig.
   - Enthält intern die vorkompilierte Shell-DLL, das MSIX Sparse-Paket, Zertifikate und Icons.
2. **Setup-Assistent (Wizard97-Stil)**:
   - Doppelklick auf `winshred.exe` öffnet den Assistenten für Neuinstallation, Reparatur oder Deinstallation.
3. **Forensische Vernichtungs-Engine (DoD 5220.22-M)**:
   - 3-faches Überschreiben (0x00, 0xFF, kryptografischer Zufall via Win32 BCrypt).
   - Cluster-Slack-Bereinigung & Ausnullen von Alternate Data Streams (ADS).
   - SSD/NVMe TRIM Deallocate-Signal zur Controller-Tilgung.
   - Mehrfache Namens-Anonymisierung im NTFS MFT-Dateisystem.
4. **Windows 11 Integration**:
   - Modernes Windows 11 Kontextmenü direkt auf oberster Ebene.
   - Windows Explorer *"Senden an"* (SendTo) Kontextmenü.
   - Fallback für klassische Windows 10 / WinPE Menüs.

---

## 🔨 Bauen (Build)

Führen Sie einfach das Skript [build.bat](file:///a:/Code/winshred/build.bat) aus oder führen Sie die Schritte manuell aus:

```cmd
:: 1. Shell-DLL kompilieren
cd winshred_shell
cargo build --release
cd ..

:: 2. Hauptprogramm mit eingebetteter DLL bauen
cargo build --release
```

---

## 📄 Lizenz

Copyright (C) 2026 WinShred. Alle Rechte vorbehalten.  
Details finden sich in der Datei [LICENSE](file:///a:/Code/winshred/LICENSE).


