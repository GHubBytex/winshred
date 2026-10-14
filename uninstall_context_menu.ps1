# WinShred - Kontextmenü-Deinstaller
$ErrorActionPreference = "SilentlyContinue"

Write-Host "Entferne WinShred aus dem Kontextmenue..." -ForegroundColor Yellow

# 1. Registry Einträge löschen
Remove-Item -Path "HKCU:\Software\Classes\*\shell\WinShred" -Recurse -Force
Remove-Item -Path "HKCU:\Software\Classes\Directory\shell\WinShred" -Recurse -Force

# 2. SendTo Verknüpfung löschen
$sendToShortcut = [System.IO.Path]::Combine($env:APPDATA, 'Microsoft\Windows\SendTo\WinShred (Sicher vernichten).lnk')
if (Test-Path $sendToShortcut) {
    Remove-Item -Path $sendToShortcut -Force
}

Write-Host "WinShred wurde vollstaendig aus dem Kontextmenue entfernt." -ForegroundColor Green

