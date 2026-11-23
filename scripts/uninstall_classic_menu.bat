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

echo [1/3] Entferne Registrierungseintraege (HKLM, HKCU, HKCR)...
reg delete "HKLM\Software\Classes\*\shell\WinShred" /f >nul 2>&1
reg delete "HKLM\Software\Classes\Directory\shell\WinShred" /f >nul 2>&1
reg delete "HKCU\Software\Classes\*\shell\WinShred" /f >nul 2>&1
reg delete "HKCU\Software\Classes\Directory\shell\WinShred" /f >nul 2>&1
reg delete "HKCR\*\shell\WinShred" /f >nul 2>&1
reg delete "HKCR\Directory\shell\WinShred" /f >nul 2>&1

echo [2/3] Entferne Senden-an (SendTo) Verknuepfung...
del /f /q "%APPDATA%\Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk" >nul 2>&1
powershell -NoProfile -ExecutionPolicy Bypass -Command "try { $u = (Get-CimInstance Win32_ComputerSystem).UserName; if ($u) { $sid = (New-Object System.Security.Principal.NTAccount($u)).Translate([System.Security.Principal.SecurityIdentifier]).Value; $p = (Get-ItemProperty -Path \"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$sid\").ProfileImagePath; if ($p) { $f = Join-Path $p 'AppData\Roaming\Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk'; if (Test-Path $f) { Remove-Item -Path $f -Force } } } } catch {}" >nul 2>&1

echo [3/3] Starte Windows Explorer neu...
taskkill /f /im explorer.exe >nul 2>&1
ping -n 3 127.0.0.1 >nul
tasklist /fi "imagename eq explorer.exe" 2>nul | find /i "explorer.exe" >nul
if errorlevel 1 (
    runas /trustlevel:0x20000 explorer.exe >nul 2>&1 || start "" explorer.exe
)

echo.
echo ======================================================================
echo   WinShred wurde erfolgreich aus dem klassischen Menue entfernt!
echo ======================================================================
echo.
pause
