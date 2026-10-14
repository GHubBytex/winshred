@echo off
setlocal EnableDelayedExpansion

title WinShred - Build All-in-One Binary
echo ======================================================================
echo             WinShred - Build All-in-One Binary
echo ======================================================================
echo.

cd /d "%~dp0"

:: 1. Pruefe Cargo / Rust
where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [FEHLER] Cargo/Rust wurde nicht im PATH gefunden!
    echo Bitte stelle sicher, dass Rust installiert ist.
    pause
    exit /b 1
)

:: 2. Suche Windows SDK Tools (MakeAppx, SignTool, RC)
set "SDK_BIN="
if exist "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\makeappx.exe" (
    set "SDK_BIN=C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64"
    goto :found_sdk
)
if exist "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\makeappx.exe" (
    set "SDK_BIN=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64"
    goto :found_sdk
)
if exist "C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\makeappx.exe" (
    set "SDK_BIN=C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64"
    goto :found_sdk
)

:found_sdk
if not defined SDK_BIN (
    echo [FEHLER] Windows SDK makeappx.exe oder signtool.exe nicht gefunden!
    pause
    exit /b 1
)

echo [OK] Windows SDK gefunden: %SDK_BIN%
echo.

:: 3. Icon aus Bild generieren (falls vorhanden)
if exist "assets\Gemini_Generated_Image_3wywxm3wywxm3wyw.jpg" (
    echo [1/6] Generiere gezoomte Icons aus Grafik...
    python assets\generate_icons.py
)

:: 4. Zertifikat pruefen oder erstellen
echo [2/6] Pruefe Signatur-Zertifikat CN=WinShred...
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq 'CN=WinShred' } | Select-Object -First 1; " ^
    "if (-not $cert) { " ^
    "    $cert = New-SelfSignedCertificate -Type Custom -Subject 'CN=WinShred' -KeyUsage DigitalSignature -FriendlyName 'WinShred' -CertStoreLocation 'Cert:\CurrentUser\My' -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3'); " ^
    "} " ^
    "Export-Certificate -Cert $cert -FilePath 'packaging\winshred.cer' -Force | Out-Null;"

if not exist "packaging\winshred.cer" (
    echo [FEHLER] packaging\winshred.cer konnte nicht erzeugt werden!
    pause
    exit /b 1
)

:: 5. Shell Extension DLL kompilieren
echo [3/6] Kompiliere Shell-Erweiterung winshred_shell.dll...
pushd winshred_shell
cargo build --release
if %errorlevel% neq 0 (
    echo [FEHLER] Fehler beim Kompilieren von winshred_shell!
    popd
    pause
    exit /b 1
)
popd

:: 6. Sparse Package packen und signieren
echo [4/6] Erstelle und signiere Windows 11 Sparse Package packaging\winshred.msix...
if exist "packaging\winshred.msix" del /f /q "packaging\winshred.msix"
"%SDK_BIN%\makeappx.exe" pack /d "packaging\package_src" /p "packaging\winshred.msix" /nv /o >nul
if %errorlevel% neq 0 (
    echo [FEHLER] makeappx.exe fehlgeschlagen!
    pause
    exit /b 1
)

"%SDK_BIN%\signtool.exe" sign /fd SHA256 /a /n "WinShred" "packaging\winshred.msix" >nul
if %errorlevel% neq 0 (
    echo [FEHLER] signtool.exe fehlgeschlagen!
    pause
    exit /b 1
)

:: 7. All-in-One Binary kompilieren (bettet DLL, MSIX, CER & ICO ein)
echo [5/6] Kompiliere All-in-One winshred.exe...
cargo build --release
if %errorlevel% neq 0 (
    echo [FEHLER] Fehler beim Kompilieren von winshred.exe!
    pause
    exit /b 1
)

:: 8. Fertige EXE kopieren
echo [6/6] Kopiere finale winshred.exe in das Hauptverzeichnis...
copy /y "target\release\winshred.exe" "winshred.exe" >nul

echo.
echo ======================================================================
echo   BUILD ERFOLGREICH!
echo.
echo   Die fertige All-in-One Datei mit neuem Icon liegt bereit:
echo   --^> %~dp0winshred.exe
echo ======================================================================
echo.

pause
