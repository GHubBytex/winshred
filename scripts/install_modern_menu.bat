@echo off
setlocal EnableDelayedExpansion

:: 1. Automatische UAC-Administrator-Elevation
fltmc >nul 2>&1 || (
    echo Fordere Administratorrechte an...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

cd /d "%~dp0\.."
title WinShred - Installation
echo ======================================================================
echo   WinShred - Installation ins moderne Windows 11 Kontextmenue
echo ======================================================================
echo.

if exist "winshred.exe" (
    winshred.exe --install
    echo.
    echo Installation abgeschlossen.
) else (
    echo [FEHLER] winshred.exe nicht gefunden! Bitte zuerst build.bat ausfuehren.
)

echo.
pause
