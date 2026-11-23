# Script zum Bauen, Signieren und Installieren des Sparse Packages für Windows 11
param(
    [string]$BinDir = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$baseDir = Split-Path -Parent $scriptDir
if ([string]::IsNullOrWhiteSpace($BinDir)) {
    $BinDir = Join-Path $baseDir "packaging\dist"
}
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

$pkgSrcDir = Join-Path $baseDir "packaging\package_src"
$assetsDir = Join-Path $pkgSrcDir "Assets"
New-Item -ItemType Directory -Force -Path $assetsDir | Out-Null

$sdkRoot = "C:\Program Files (x86)\Windows Kits\10\bin"
$sdkDirs = Get-ChildItem -Path $sdkRoot -Directory -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName "x64\makeappx.exe") } |
    Sort-Object {
        try { [version]$_.Name } catch { [version]"0.0.0.0" }
    } -Descending

if (-not $sdkDirs) {
    throw "Windows SDK (makeappx.exe / signtool.exe) wurde unter $sdkRoot nicht gefunden."
}

$sdkX64 = Join-Path ($sdkDirs | Select-Object -First 1).FullName "x64"
$makeAppx = Join-Path $sdkX64 "makeappx.exe"
$signTool = Join-Path $sdkX64 "signtool.exe"
$msixPath = Join-Path $baseDir "packaging\winshred.msix"
$cerPath = Join-Path $baseDir "packaging\winshred.cer"

Write-Host "1. Erstelle / prüfe Signatur-Zertifikat CN=WinShred..." -ForegroundColor Cyan
$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq "CN=WinShred" } | Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate -Type Custom -Subject "CN=WinShred" `
        -KeyUsage DigitalSignature `
        -FriendlyName "WinShred" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")
}
Export-Certificate -Cert $cert -FilePath $cerPath -Force | Out-Null

$rootStore = New-Object System.Security.Cryptography.X509Certificates.X509Store "Root", "CurrentUser"
$rootStore.Open("ReadWrite")
$rootStore.Add($cert)
$rootStore.Close()

Write-Host "2. Kompiliere Shell-Extension winshred_shell.dll..." -ForegroundColor Cyan
Push-Location (Join-Path $baseDir "winshred_shell")
cargo build --release
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo build (winshred_shell) fehlgeschlagen." }
Pop-Location

Write-Host "3. Erstelle PNG-Assets..." -ForegroundColor Cyan
$iconPng = Join-Path $baseDir "assets\icon.png"
$iconGen = Join-Path $baseDir "assets\generate_icons.py"
if ((Test-Path $iconPng) -and (Test-Path $iconGen) -and (Get-Command python -ErrorAction SilentlyContinue)) {
    python $iconGen
} else {
    Add-Type -AssemblyName System.Drawing
    function Create-PngIcon([string]$path, [int]$size, [System.Drawing.Color]$bgColor, [System.Drawing.Color]$fgColor) {
        $bmp = New-Object System.Drawing.Bitmap $size, $size
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.Clear($bgColor)
        $brush = New-Object System.Drawing.SolidBrush $fgColor
        $pen = New-Object System.Drawing.Pen -ArgumentList @([System.Drawing.Color]::White, [float]([Math]::Max(2, [int]($size / 15))))
        $margin = [int]($size * 0.15)
        $rect = New-Object System.Drawing.Rectangle $margin, $margin, ($size - 2 * $margin), ($size - 2 * $margin)
        $g.FillEllipse($brush, $rect)
        $p1 = New-Object System.Drawing.Point ([int]($size * 0.35)), ([int]($size * 0.35))
        $p2 = New-Object System.Drawing.Point ([int]($size * 0.65)), ([int]($size * 0.65))
        $p3 = New-Object System.Drawing.Point ([int]($size * 0.65)), ([int]($size * 0.35))
        $p4 = New-Object System.Drawing.Point ([int]($size * 0.35)), ([int]($size * 0.65))
        $g.DrawLine($pen, $p1, $p2)
        $g.DrawLine($pen, $p3, $p4)
        $g.Dispose()
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
    }
    $red = [System.Drawing.Color]::FromArgb(220, 50, 50)
    $trans = [System.Drawing.Color]::Transparent
    Create-PngIcon (Join-Path $assetsDir "Square150x150Logo.png") 150 $trans $red
    Create-PngIcon (Join-Path $assetsDir "Square44x44Logo.png") 44 $trans $red
    Create-PngIcon (Join-Path $assetsDir "StoreLogo.png") 50 $trans $red
}

Write-Host "4. Erstelle AppxManifest.xml..." -ForegroundColor Cyan
$manifestContent = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap2="http://schemas.microsoft.com/appx/manifest/uap/windows10/2"
  xmlns:uap3="http://schemas.microsoft.com/appx/manifest/uap/windows10/3"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"
  xmlns:desktop4="http://schemas.microsoft.com/appx/manifest/desktop/windows10/4"
  xmlns:desktop5="http://schemas.microsoft.com/appx/manifest/desktop/windows10/5"
  xmlns:desktop6="http://schemas.microsoft.com/appx/manifest/desktop/windows10/6"
  xmlns:desktop10="http://schemas.microsoft.com/appx/manifest/desktop/windows10/10"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"
  IgnorableNamespaces="uap uap2 uap3 rescap desktop uap10 desktop10">
  <Identity Name="WinShred.ShellExtension"
            ProcessorArchitecture="x64"
            Publisher="CN=WinShred"
            Version="1.1.0.0" />
  <Properties>
    <DisplayName>WinShred</DisplayName>
    <PublisherDisplayName>WinShred</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
    <uap10:AllowExternalContent>true</uap10:AllowExternalContent>
    <desktop6:RegistryWriteVirtualization>disabled</desktop6:RegistryWriteVirtualization>
    <desktop6:FileSystemWriteVirtualization>disabled</desktop6:FileSystemWriteVirtualization>
  </Properties>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19000.0" MaxVersionTested="10.0.22621.0" />
  </Dependencies>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
    <rescap:Capability Name="unvirtualizedResources" />
  </Capabilities>
  <Applications>
    <Application Id="WinShred" Executable="winshred.exe" uap10:TrustLevel="mediumIL" uap10:RuntimeBehavior="win32App">
      <uap:VisualElements DisplayName="WinShred" Description="WinShred" AppListEntry="none" BackgroundColor="transparent" Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png" />
      <Extensions>
        <desktop4:Extension Category="windows.fileExplorerContextMenus">
          <desktop4:FileExplorerContextMenus>
            <desktop5:ItemType Type="*">
              <desktop5:Verb Id="WinShredVerb" Clsid="8a76b81c-0a87-4348-9f7c-79f8815e7c81" />
            </desktop5:ItemType>
            <desktop5:ItemType Type="Directory">
              <desktop5:Verb Id="WinShredVerb" Clsid="8a76b81c-0a87-4348-9f7c-79f8815e7c81" />
            </desktop5:ItemType>
          </desktop4:FileExplorerContextMenus>
        </desktop4:Extension>
        <com:Extension Category="windows.comServer">
          <com:ComServer>
            <com:SurrogateServer DisplayName="WinShred ContextMenu handler">
              <com:Class Id="8a76b81c-0a87-4348-9f7c-79f8815e7c81" Path="winshred_shell.dll" ThreadingModel="STA" />
            </com:SurrogateServer>
          </com:ComServer>
        </com:Extension>
      </Extensions>
    </Application>
  </Applications>
</Package>
"@
Set-Content -Path (Join-Path $pkgSrcDir "AppxManifest.xml") -Value $manifestContent -Encoding UTF8

Write-Host "5. Packe & signiere Sparse MSIX..." -ForegroundColor Cyan
if (Test-Path $msixPath) { Remove-Item $msixPath -Force }
& $makeAppx pack /d $pkgSrcDir /p $msixPath /nv /o
if ($LASTEXITCODE -ne 0) { throw "makeappx.exe fehlgeschlagen." }
& $signTool sign /fd SHA256 /a /n "WinShred" $msixPath
if ($LASTEXITCODE -ne 0) { throw "signtool.exe fehlgeschlagen." }

Write-Host "6. Kompiliere Hauptprogramm winshred.exe..." -ForegroundColor Cyan
Push-Location $baseDir
cargo build --release
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo build (winshred) fehlgeschlagen." }
Pop-Location

Copy-Item (Join-Path $baseDir "target\release\winshred.exe") (Join-Path $BinDir "winshred.exe") -Force
Copy-Item (Join-Path $baseDir "winshred_shell\target\release\winshred_shell.dll") (Join-Path $BinDir "winshred_shell.dll") -Force
if (Test-Path (Join-Path $baseDir "assets\winshred.ico")) {
    Copy-Item (Join-Path $baseDir "assets\winshred.ico") (Join-Path $BinDir "winshred.ico") -Force
}

Write-Host "7. Registriere Package mit ExternalLocation ($BinDir)..." -ForegroundColor Cyan
Get-AppxPackage -Name "WinShred.ShellExtension" -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue
Add-AppxPackage -Path $msixPath -ExternalLocation $BinDir

Write-Host "`nERFOLG! WinShred ist nun offiziell im Windows 11 Modern Context Menu registriert!" -ForegroundColor Green
