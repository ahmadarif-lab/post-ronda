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
