<div align="center">

<img src="assets/icon.png" alt="WinShred Logo" width="128" height="128" />

# WinShred

**Forensisch sichere, unwiderrufliche Datei- und Ordnervernichtung für Windows 10 & Windows 11.**  
Vollständig integriert in das moderne Windows 11 Kontextmenü (*"Mit WinShred sicher vernichten"*) auf der ersten Menüebene – ohne *"Weitere Optionen anzeigen"*.

</div>

---

## 📁 Projektstruktur

```text
winshred/
├── .gitignore
├── Cargo.toml                  # Hauptprojekt Konfiguration
├── Cargo.lock
├── build.rs                    # Bettet Shell-DLL, Sparse-MSIX, Zertifikat & Icon ins Binary ein
├── build.bat                   # 1-Klick-Buildskript für das All-in-One Binary
├── README.md                   # Projektdokumentation
├── winshred.exe                # Portables All-in-One Binary (Release-Build-Artefakt)
│
├── assets/                     # Grafiken & Bildmaterial
│   ├── icon.png                # Quellgrafik (2048x2048)
│   ├── winshred.ico            # Multi-Resolution Windows Icon (16x16 bis 256x256)
│   └── generate_icons.py       # Icon-Generator für alle Plattformen
│
├── packaging/                  # Windows 11 Sparse MSIX Paket
│   ├── package_src/            # Paket-Inhalte (AppxManifest.xml, PNG-Kacheln)
│   ├── winshred.cer            # Selbstsigniertes Zertifikat (lokal erzeugt, per .gitignore ausgeschlossen)
│   └── winshred.msix           # Signiertes Sparse-Paket für moderne Shell-Registrierung (Build-Artefakt)
│
├── resources/                  # Windows 32 Ressourcen & Manifest
│   ├── app.rc                  # Resource-Definition (Icon & Manifest)
│   └── app.manifest            # UAC Execution Level, LongPathAware & Windows 10/11 Themes
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
   - Enthält intern die vorkompilierte Shell-DLL (`winshred_shell.dll`), das MSIX Sparse-Paket, das Zertifikat und das Icon.
2. **Setup-Assistent (Wizard97-Stil)**:
   - Doppelklick auf `winshred.exe` öffnet den Assistenten zur Installation (Modernes Windows 11 Menü, SendTo, optional klassisches Menü) oder vollständigen Deinstallation.
3. **Forensische Vernichtungs-Engine (Hybrid: DoD 5220.22-M / NIST SP 800-88)**:
   - **HDD**: Standardmäßig 3-faches Überschreiben (`0x00`, `0xFF`, kryptografischer Zufall via Win32 `BCryptGenRandom`); konfigurierbar über `--passes <N>` (der letzte Durchgang schreibt stets Krypto-Zufallsdaten).
   - **SSD/NVMe & MFT-residente Dateien**: Automatische Medienerkennung (`StorageDeviceProperty` / `DeviceSeekPenaltyProperty`) für flash-schonendes 1-Pass-Wiping inkl. `FSCTL_FILE_LEVEL_TRIM`-Deallocate bei SSD/NVMe; residente Dateien (`< 1 KB`) werden strikt in-place überschrieben.
   - **NTFS-Tiefenbereinigung**: Überschreiben von Cluster-Slack, **Alternate Data Streams (ADS)**, vorallokierten 0-Byte-Dateien (`AllocationSize > 0`), gezieltes Wiping allokierter Bereiche bei **Sparse Files**, Behandlung komprimierter Dateien, Timestomp auf `1601-01-01` und zufällige Umbenennung vor dem Löschen.
   - **Optionale USN-Journal-Bereinigung (`$UsnJrnl`)**: Bei Ausführung mit Administratorrechten (`--admin` / `-p` oder Checkbox im Bestätigungsdialog) wird das NTFS-USN-Journal betroffener Volumes per `FSCTL_DELETE_USN_JOURNAL` synchron bereinigt.
4. **Umfassende Schutzmechanismen & Dateisystem-Härtung**:
   - **Hardlink-Schutz (`nlink > 1`)**: Dateien mit mehreren NTFS-Hardlinks werden nicht in-place überschrieben oder gekürzt, um fremde Verknüpfungen auf dieselben Datenblöcke nicht zu beschädigen – nur der gewählte Verzeichniseintrag wird sicher entfernt.
   - **Symlink-, Junction-, Reparse-Point- & Cloud-Schutz**: Symlinks, NTFS-Junctions sowie allgemeine Reparse Points / Cloud-Platzhalter (z. B. OneDrive) werden niemals rekursiv betreten oder ausgelöst, sondern nur als Link entfernt.
   - **Pfad- & Systemschutz**: Blockiert Systemordner, Laufwerksstämme, UNC-/Admin-Share-Wurzeln (`\\host\C$\Windows`), `\\?\Volume{...}\`-Pfade, `\\.\`-Device-Pfade und löst 8.3-Kurznamen sicher auf; wichtige Benutzerordner (z. B. `Desktop`, `Dokumente`, `Users\Public`) lösen eine verschärfte Countdown-Warnung aus.
   - **Langpfad- & Win32-Sonderpfad-Support**: Verarbeitet lange Pfade (`>= 260` Zeichen) unabhängig von der Windows-Registry-Einstellung `LongPathsEnabled` sowie Namen mit abschließendem Punkt/Leerzeichen oder reservierten DOS-Namen über `\\?\`-Verbatim-Pfade.
5. **Multithreaded Fortschrittsdialog & Windows-Integration**:
   - Nativer Win32-Fortschrittsdialog mit Pre-Scan, Live-Byte-/Elementzählung, dynamischer Modus-/Pass-Anzeige, atomarer Abbruchfunktion und `SHChangeNotify`-Aktualisierung.
   - Modernes Windows 11 Kontextmenü (`HKCU`/`AppX`), *"Senden an"* (`SendTo`) auch über UAC-Kontowechsel hinweg sowie klassisches `HKLM`-Kontextmenü.

---

## 💻 CLI-Nutzung

```text
winshred.exe                        Startet den Installations-/Verwaltungsassistenten
winshred.exe [OPTIONEN] [--] <PFADE...>
```

| Option | Beschreibung |
| :--- | :--- |
| `--install`, `-i` | Installiert Kontextmenü (Modern + SendTo) im Hintergrund (optional mit `--quiet`) |
| `--uninstall`, `-u` | Deinstalliert WinShred vollständig im Hintergrund (optional mit `--quiet`) |
| `--reset-prompts` | Aktiviert die Sicherheitsabfrage vor dem Löschen wieder |
| `--passes <N>` | Anzahl der Überschreib-Durchgänge für HDDs (Standard: `3`, SSD/NVMe immer `1`) |
| `--filelist <DATEI>` | Liest zu vernichtende Pfade zeilenweise aus einer UTF-8-Datei |
| `--admin`, `-p` | Bereinigt zusätzlich das NTFS USN-Journal (`$UsnJrnl`) inkl. UAC-Elevation bei Bedarf |
| `--yes`, `-y` | Überspringt die Sicherheitsabfrage (zeigt den Fortschrittsdialog) |
| `--quiet` | Headless-Ausführung ohne GUI-Dialoge (inkl. `-y`) |
| `--help`, `-h`, `/?` | Zeigt die CLI-Hilfe an |

---

## 🔨 Bauen & Testen

**Voraussetzungen:** Rust-Toolchain (MSVC x64), Windows 10/11 SDK (`makeappx.exe`, `signtool.exe`), sowie optional Python (für Icon-Generierung in [build.bat](build.bat)).

### 1-Klick-Release-Build
Führen Sie das Skript [build.bat](build.bat) aus. Es prüft bzw. erzeugt das Signaturzertifikat `CN=WinShred`, kompiliert `winshred_shell.dll`, packt und signiert `packaging\winshred.msix` und baut anschließend die fertige `winshred.exe` im Projektstamm.

### Manuelles Bauen & Qualitätsprüfung

```cmd
:: 1. Shell-DLL prüfen & kompilieren
cd winshred_shell
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
cd ..

:: 2. Hauptprogramm prüfen, testen & mit eingebetteten Artefakten bauen
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

---

## 📄 Lizenz

Copyright (C) 2026 WinShred. Alle Rechte vorbehalten.  
Details finden sich in der Datei [LICENSE](LICENSE).
