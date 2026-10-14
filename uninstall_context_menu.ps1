# WinShred - Kontextmenü-Deinstaller
$ErrorActionPreference = "SilentlyContinue"

# Automatische UAC-Administrator-Elevation
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "Fordere Administratorrechte an..." -ForegroundColor Yellow
    Start-Process powershell.exe -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Verb RunAs
    exit
}

Write-Host "Entferne WinShred aus dem Kontextmenue..." -ForegroundColor Yellow

# 1. Registry Einträge löschen (direkt via reg.exe ohne Wildcard-Hang)
& reg delete "HKLM\Software\Classes\*\shell\WinShred" /f 2>$null
& reg delete "HKLM\Software\Classes\Directory\shell\WinShred" /f 2>$null
& reg delete "HKCU\Software\Classes\*\shell\WinShred" /f 2>$null
& reg delete "HKCU\Software\Classes\Directory\shell\WinShred" /f 2>$null
& reg delete "HKCR\*\shell\WinShred" /f 2>$null
& reg delete "HKCR\Directory\shell\WinShred" /f 2>$null

# 2. SendTo Verknüpfung löschen
$sendToShortcut = [System.IO.Path]::Combine($env:APPDATA, 'Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk')
if (Test-Path $sendToShortcut) {
    Remove-Item -Path $sendToShortcut -Force
}

# 3. Explorer neu starten
Stop-Process -Name explorer -Force
Start-Process explorer

Write-Host "WinShred wurde vollstaendig aus dem Kontextmenue entfernt." -ForegroundColor Green
Start-Sleep -Seconds 2
