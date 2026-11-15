param(
    [string]$Output = 'dist/cutup-studio-windows'
)
$ErrorActionPreference = 'Stop'
$rustHost = rustc -vV
if ($LASTEXITCODE -ne 0 -or -not ($rustHost -contains 'host: x86_64-pc-windows-msvc')) {
    throw 'The x86_64-pc-windows-msvc Rust toolchain is required'
}
if (Test-Path -LiteralPath $Output) { throw "Output already exists: $Output" }
$before = (Get-FileHash -Algorithm SHA256 'THIRD_PARTY_LICENSES.md').Hash
& "$PSScriptRoot/generate-licenses.ps1"
if ($LASTEXITCODE -ne 0) { throw 'License generation failed' }
$after = (Get-FileHash -Algorithm SHA256 'THIRD_PARTY_LICENSES.md').Hash
if ($before -ne $after) { throw 'License notices changed. Review and commit them before packaging.' }
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
New-Item -ItemType Directory -Path $Output | Out-Null
Copy-Item -LiteralPath 'target/release/cutup-studio.exe', 'LICENSE', 'THIRD_PARTY_LICENSES.md', 'README.md' -Destination $Output
$demoFiles = @(git ls-files -- 'demo/README.md' 'demo/materials/*.wav')
if ($LASTEXITCODE -ne 0 -or $demoFiles.Count -eq 0) { throw 'Tracked DEMO files could not be listed' }
foreach ($file in $demoFiles) {
    $destination = Join-Path $Output $file
    $parent = Split-Path -Parent $destination
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    Copy-Item -LiteralPath $file -Destination $destination
}
$setupDoc = 'src/infra/local_ai/README.md'
$setupDest = Join-Path $Output $setupDoc
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $setupDest) | Out-Null
Copy-Item -LiteralPath $setupDoc -Destination $setupDest
Write-Output "Created: $Output"