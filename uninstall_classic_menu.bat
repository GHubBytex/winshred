@echo off
setlocal EnableDelayedExpansion

:: Automatische UAC-Administrator-Elevation
fltmc >nul 2>&1 || (
    echo Fordere Administratorrechte an...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

cd /d "%~dp0"
title WinShred - Deinstallation aus dem klassischen Kontextmenue
echo ======================================================================
echo   WinShred - Deinstallation aus dem klassischen Kontextmenue
echo ======================================================================
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "Write-Host '[1/3] Entferne Registrierungseintraege (Dateien & Ordner)...' -ForegroundColor Yellow; " ^
    "Remove-Item -Path 'HKCU:\Software\Classes\*\shell\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; " ^
    "Remove-Item -Path 'HKCU:\Software\Classes\Directory\shell\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; " ^
    "Remove-Item -Path 'HKLM:\Software\Classes\*\shell\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; " ^
    "Remove-Item -Path 'HKLM:\Software\Classes\Directory\shell\WinShred' -Recurse -Force -ErrorAction SilentlyContinue; " ^
    "Write-Host '[2/3] Entferne Senden-an (SendTo) Verknuepfung...' -ForegroundColor Yellow; " ^
    "$sendTo = Join-Path $env:APPDATA 'Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk'; " ^
    "if (Test-Path $sendTo) { Remove-Item -Path $sendTo -Force -ErrorAction SilentlyContinue }; " ^
    "Write-Host '[3/3] Aktualisiere Windows Explorer...' -ForegroundColor Yellow; " ^
    "Stop-Process -Name explorer -Force; Start-Process explorer; " ^
    "Write-Host ''; " ^
    "Write-Host 'WinShred wurde erfolgreich aus dem klassischen Kontextmenue entfernt.' -ForegroundColor Green;"

echo.
pause
