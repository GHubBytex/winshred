@echo off
setlocal EnableDelayedExpansion

:: Automatische UAC-Administrator-Elevation
fltmc >nul 2>&1 || (
    echo Fordere Administratorrechte an...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

cd /d "%~dp0\.."
title WinShred - Deinstallation
echo ======================================================================
echo   WinShred - Deinstallation
echo ======================================================================
echo.

if exist "winshred.exe" (
    winshred.exe --uninstall
    echo.
    echo Deinstallation abgeschlossen.
) else (
    echo [FEHLER] winshred.exe nicht gefunden! Bitte zuerst build.bat ausfuehren.
)

echo.
pause
