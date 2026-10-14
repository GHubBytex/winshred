@echo off
setlocal EnableDelayedExpansion

:: Automatische UAC-Administrator-Elevation
fltmc >nul 2>&1 || (
    echo Fordere Administratorrechte an...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

cd /d "%~dp0"
title WinShred - Deinstallation aus dem modernen Windows 11 Kontextmenue
echo ======================================================================
echo   WinShred - Deinstallation aus dem modernen Kontextmenue
echo ======================================================================
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "Write-Host '[1/3] Entferne Appx-Paket...' -ForegroundColor Yellow; " ^
    "Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue; " ^
    "Write-Host '[2/3] Bereinige Zertifikatsspeicher...' -ForegroundColor Yellow; " ^
    "$cert = Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object { $_.Subject -eq 'CN=WinShred' }; " ^
    "if ($cert) { Remove-Item ('Cert:\LocalMachine\TrustedPeople\' + $cert.Thumbprint) -Force }; " ^
    "Write-Host '[3/3] Starte Explorer neu...' -ForegroundColor Yellow; " ^
    "Stop-Process -Name explorer -Force; Start-Process explorer; " ^
    "Write-Host ''; " ^
    "Write-Host 'WinShred wurde erfolgreich aus dem modernen Kontextmenue entfernt.' -ForegroundColor Green;"

echo.
pause
