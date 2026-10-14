# Script zum Bauen, Signieren und Installieren des Sparse Packages für Windows 11
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$baseDir = Split-Path -Parent $scriptDir
$pkgSrcDir = Join-Path $baseDir "packaging\package_src"
$assetsDir = Join-Path $pkgSrcDir "Assets"
$makeAppx = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\makeappx.exe"
$signTool = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe"
$msixPath = Join-Path $baseDir "packaging\winshred.msix"

Write-Host "2. Kompiliere Binaries..." -ForegroundColor Cyan
Push-Location $baseDir
cargo build --release
Pop-Location

Push-Location (Join-Path $baseDir "winshred_shell")
cargo build --release
Pop-Location

Copy-Item (Join-Path $baseDir "target\release\winshred.exe") $binDir -Force
Copy-Item (Join-Path $baseDir "winshred_shell\target\release\winshred_shell.dll") $binDir -Force

Write-Host "3. Erstelle PNG-Assets..." -ForegroundColor Cyan
Add-Type -AssemblyName System.Drawing
function Create-PngIcon([string]$path, [int]$size, [System.Drawing.Color]$bgColor, [System.Drawing.Color]$fgColor) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear($bgColor)
    
    # Roter Kreis / Mülleimer-Symbol
    $brush = New-Object System.Drawing.SolidBrush $fgColor
    $pen = New-Object System.Drawing.Pen -ArgumentList @([System.Drawing.Color]::White, [float]([Math]::Max(2, [int]($size / 15))))
    
    $margin = [int]($size * 0.15)
    $rect = New-Object System.Drawing.Rectangle $margin, $margin, ($size - 2 * $margin), ($size - 2 * $margin)
    $g.FillEllipse($brush, $rect)
    
    # Kreuz oder X im Kreis
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
            Version="1.0.0.0" />
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

Write-Host "5. Packe Sparse MSIX..." -ForegroundColor Cyan
if (Test-Path $msixPath) { Remove-Item $msixPath -Force }
& $makeAppx pack /d $pkgSrcDir /p $msixPath /nv /o

Write-Host "6. Erstelle & vertraue Signatur-Zertifikat..." -ForegroundColor Cyan
$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq "CN=WinShred" } | Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate -Type Custom -Subject "CN=WinShred" `
        -KeyUsage DigitalSignature `
        -FriendlyName "WinShred" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")
}

# Installiere in Vertrauenswürdige Stammzertifizierungsstellen (CurrentUser)
$rootStore = New-Object System.Security.Cryptography.X509Certificates.X509Store "Root", "CurrentUser"
$rootStore.Open("ReadWrite")
$rootStore.Add($cert)
$rootStore.Close()

Write-Host "7. Signiere winshred.msix..." -ForegroundColor Cyan
& $signTool sign /fd SHA256 /a /n "WinShred" $msixPath

Write-Host "8. Registriere Package mit ExternalLocation..." -ForegroundColor Cyan
# Falls schon registriert, vorher deinstallieren
Get-AppxPackage -Name "WinShred.ShellExtension" -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue

Add-AppxPackage -Path $msixPath -ExternalLocation $binDir

Write-Host "`nERFOLG! WinShred ist nun offiziell im Windows 11 Modern Context Menu registriert!" -ForegroundColor Green
