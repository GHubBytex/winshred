# WinShred - Kontextmenü-Installer
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$exePath = Join-Path $scriptDir "target\release\winshred.exe"

if (-not (Test-Path $exePath)) {
    Write-Host "Kompiliere WinShred Release..." -ForegroundColor Cyan
    Push-Location $scriptDir
    cargo build --release
    Pop-Location
}

if (-not (Test-Path $exePath)) {
    Write-Error "winshred.exe wurde nicht gefunden unter: $exePath"
    exit 1
}

$iconPath = "%SystemRoot%\System32\imageres.dll,-5308" # Windows Shredder / Papierkorb Icon

Write-Host "Registriere WinShred im Kontextmenue..." -ForegroundColor Green

# 1. Dateien (*)
$fileKey = "HKCU:\Software\Classes\*\shell\WinShred"
New-Item -Path $fileKey -Force | Out-Null
Set-ItemProperty -Path $fileKey -Name "(Default)" -Value "Mit WinShred sicher vernichten"
Set-ItemProperty -Path $fileKey -Name "Icon" -Value $iconPath
$fileCmdKey = "$fileKey\command"
New-Item -Path $fileCmdKey -Force | Out-Null
Set-ItemProperty -Path $fileCmdKey -Name "(Default)" -Value "`"$exePath`" `"%1`""

# 2. Ordner (Directory)
$dirKey = "HKCU:\Software\Classes\Directory\shell\WinShred"
New-Item -Path $dirKey -Force | Out-Null
Set-ItemProperty -Path $dirKey -Name "(Default)" -Value "Mit WinShred sicher vernichten"
Set-ItemProperty -Path $dirKey -Name "Icon" -Value $iconPath
$dirCmdKey = "$dirKey\command"
New-Item -Path $dirCmdKey -Force | Out-Null
Set-ItemProperty -Path $dirCmdKey -Name "(Default)" -Value "`"$exePath`" `"%1`""

# 3. 'Senden an' (SendTo) Menü Verknüpfung
$sendToPath = [System.IO.Path]::Combine($env:APPDATA, 'Microsoft\Windows\SendTo')
if (Test-Path $sendToPath) {
    $wshShell = New-Object -ComObject WScript.Shell
    $shortcutPath = Join-Path $sendToPath "WinShred (Sicher vernichten).lnk"
    $shortcut = $wshShell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = $exePath
    $shortcut.IconLocation = "$env:SystemRoot\System32\imageres.dll,5307"
    $shortcut.Description = "Dateien und Ordner mit WinShred sicher vernichten"
    $shortcut.Save()
    Write-Host "SendTo-Eintrag erstellt: $shortcutPath" -ForegroundColor Green
}

Write-Host "`nErfolgreich installiert!" -ForegroundColor Cyan
Write-Host "Du kannst nun jede Datei oder jeden Ordner rechtsklicken und 'Mit WinShred sicher vernichten' waehlen." -ForegroundColor Yellow

