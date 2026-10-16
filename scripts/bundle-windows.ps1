$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
$package = 'dist/Post-Ronda-windows-x64'
New-Item -ItemType Directory -Force -Path $package | Out-Null
Copy-Item 'target/release/post-ronda.exe' "$package/PostRonda.exe"
Copy-Item 'README.md','LICENSE','assets/Inter-LICENSE.txt','assets/Tabler-LICENSE.txt' $package
if (Test-Path 'assets/Bruno-LICENSE.txt') { Copy-Item 'assets/Bruno-LICENSE.txt' $package }
Compress-Archive -Path $package -DestinationPath 'dist/Post-Ronda-windows-x64.zip' -CompressionLevel Optimal -Force
Write-Output 'Built dist/Post-Ronda-windows-x64.zip'

# The installer needs Inno Setup (choco install innosetup); without it only the zip is built.
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    foreach ($candidate in 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe', 'C:\Program Files\Inno Setup 6\ISCC.exe') {
        if (Test-Path $candidate) { $iscc = $candidate; break }
    }
}
if ($iscc) {
    $version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    & $iscc "/DAppVersion=$version" 'scripts/PostRonda.iss'
    if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
    Write-Output 'Built dist/Post-Ronda-Setup-x64.exe'
} else {
    Write-Warning 'Inno Setup not found; the installer was skipped.'
}
