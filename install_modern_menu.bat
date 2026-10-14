@echo off
setlocal EnableDelayedExpansion

:: 1. Automatische UAC-Administrator-Elevation
fltmc >nul 2>&1 || (
    echo Fordere Administratorrechte an...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

cd /d "%~dp0"
title WinShred - Installation ins moderne Windows 11 Kontextmenue
echo ======================================================================
echo   WinShred - Installation ins moderne Windows 11 Kontextmenue
echo ======================================================================
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$certPath = Join-Path (Get-Location) 'winshred.cer'; " ^
    "$msixPath = Join-Path (Get-Location) 'winshred.msix'; " ^
    "$binPath = Join-Path (Get-Location) 'bin'; " ^
    "Write-Host '[1/3] Vertraue Zertifikat im lokalen Rechnerspeicher (TrustedPeople)...' -ForegroundColor Cyan; " ^
    "Import-Certificate -FilePath $certPath -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null; " ^
    "Write-Host '[2/3] Registriere Sparse MSIX Package in Windows 11 Shell...' -ForegroundColor Cyan; " ^
    "Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue; " ^
    "Add-AppxPackage -Path $msixPath -ExternalLocation $binPath; " ^
    "Write-Host '[3/3] Aktualisiere Windows Explorer...' -ForegroundColor Cyan; " ^
    "Stop-Process -Name explorer -Force; Start-Process explorer; " ^
    "Write-Host ''; " ^
    "Write-Host '======================================================================' -ForegroundColor Green; " ^
    "Write-Host ' ERFOLG: WinShred ist jetzt direkt im modernen Menue verfuegbar!' -ForegroundColor Green; " ^
    "Write-Host '======================================================================' -ForegroundColor Green;"

echo.
pause
