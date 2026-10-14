@echo off
:: WinShred - Windows 11 Modern Context Menu Installer
:: Benötigt Administratorrechte, um das lokale Zertifikat für Windows 11 AppX zu hinterlegen.

net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Fordere Administratorrechte an...
    powershell -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

cd /d "%~dp0"
echo ===================================================
echo   WinShred - Installation ins moderne Windows 11 Menue
echo ===================================================
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$certPath = Join-Path (Get-Location) 'winshred.cer'; " ^
    "$msixPath = Join-Path (Get-Location) 'winshred.msix'; " ^
    "$binPath = Join-Path (Get-Location) 'bin'; " ^
    "Write-Host '1. Vertraue Zertifikat in LocalMachine\TrustedPeople...' -ForegroundColor Cyan; " ^
    "Import-Certificate -FilePath $certPath -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null; " ^
    "Write-Host '2. Registriere Windows 11 Modern Context Menu...' -ForegroundColor Cyan; " ^
    "Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue; " ^
    "Add-AppxPackage -Path $msixPath -ExternalLocation $binPath; " ^
    "Write-Host '3. Starte Explorer neu, um Menue zu aktualisieren...' -ForegroundColor Cyan; " ^
    "Stop-Process -Name explorer -Force; Start-Process explorer; " ^
    "Write-Host 'FERTIG! WinShred ist jetzt im modernen Kontextmenue!' -ForegroundColor Green;"

echo.
pause

