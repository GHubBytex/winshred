@echo off
:: WinShred - Modern Context Menu Uninstaller
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -Command "Start-Process cmd -ArgumentList '/c \"\"%~f0\"\"' -Verb RunAs"
    exit /b
)

cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "Get-AppxPackage -Name 'WinShred.ShellExtension' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue; " ^
    "$cert = Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object { $_.Subject -eq 'CN=WinShred' }; " ^
    "if ($cert) { Remove-Item ('Cert:\LocalMachine\TrustedPeople\' + $cert.Thumbprint) -Force }; " ^
    "Stop-Process -Name explorer -Force; Start-Process explorer; " ^
    "Write-Host 'WinShred wurde erfolgreich aus dem modernen Kontextmenue entfernt.' -ForegroundColor Green;"

pause

