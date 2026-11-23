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

# 2. SendTo Verknüpfung löschen (aktiver Desktop-Benutzer + aktueller Kontext)
$sendToDirs = @([System.IO.Path]::Combine($env:APPDATA, 'Microsoft\Windows\SendTo'))
try {
    $activeUser = (Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue).UserName
    if ($activeUser) {
        $nt = New-Object System.Security.Principal.NTAccount($activeUser)
        $sid = $nt.Translate([System.Security.Principal.SecurityIdentifier]).Value
        $prof = (Get-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\$sid" -ErrorAction SilentlyContinue).ProfileImagePath
        if ($prof) {
            $userSendTo = [System.IO.Path]::Combine($prof, 'AppData\Roaming\Microsoft\Windows\SendTo')
            if ($sendToDirs -notcontains $userSendTo) {
                $sendToDirs += $userSendTo
            }
        }
    }
} catch {}

foreach ($dir in $sendToDirs) {
    $sendToShortcut = [System.IO.Path]::Combine($dir, 'WinShred (Sicher vernichten).lnk')
    if (Test-Path $sendToShortcut) {
        Remove-Item -Path $sendToShortcut -Force
    }
}

# 3. Explorer neu starten (ohne Elevation, bevorzugt über automatischen Shell-Respawn / Medium-Integrity)
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 1200
if (-not (Get-Process -Name explorer -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath "runas.exe" -ArgumentList "/trustlevel:0x20000 explorer.exe" -WindowStyle Hidden -ErrorAction SilentlyContinue
}

Write-Host "WinShred wurde vollstaendig aus dem Kontextmenue entfernt." -ForegroundColor Green
Start-Sleep -Seconds 2
