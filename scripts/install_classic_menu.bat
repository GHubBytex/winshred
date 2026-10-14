@echo off
setlocal EnableDelayedExpansion

:: Automatische UAC-Administrator-Elevation
fltmc >nul 2>&1 || (
    echo Fordere Administratorrechte an...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

cd /d "%~dp0"
title WinShred - Installation ins klassische Kontextmenue
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install_context_menu.ps1"
pause

