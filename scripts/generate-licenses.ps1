$ErrorActionPreference = 'Stop'

# Run from the repository root after installing cargo-about 0.8.4.
cargo about generate --locked --fail -o THIRD_PARTY_LICENSES.md about.hbs
if ($LASTEXITCODE -ne 0) { throw 'cargo-about failed' }
$metadata = cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
$fontPackage = @($metadata.packages | Where-Object { $_.name -eq 'epaint_default_fonts' })
if ($fontPackage.Count -ne 1) { throw 'epaint_default_fonts not found uniquely' }
$fontDir = Join-Path (Split-Path $fontPackage[0].manifest_path) 'fonts'
$fontLicenses = @('Hack-Regular.txt', 'OFL.txt', 'UFL.txt', 'emoji-icon-font-mit-license.txt')
$append = [System.Text.StringBuilder]::new()
[void]$append.AppendLine("`n## Bundled egui fonts and icons`n")
foreach ($name in $fontLicenses) {
    [void]$append.AppendLine("### epaint_default_fonts: $name`n`n``````text")
    [void]$append.AppendLine([IO.File]::ReadAllText((Join-Path $fontDir $name)))
    [void]$append.AppendLine("```````n")
}
foreach ($name in @('lucide', 'ollama')) {
    [void]$append.AppendLine("### $name icons`n`n``````text")
    [void]$append.AppendLine([IO.File]::ReadAllText((Join-Path "assets/icons/$name" 'LICENSE')))
    [void]$append.AppendLine("```````n")
}
[IO.File]::AppendAllText((Join-Path (Get-Location) 'THIRD_PARTY_LICENSES.md'), ($append.ToString().TrimEnd() + "`n"), [Text.UTF8Encoding]::new($false))