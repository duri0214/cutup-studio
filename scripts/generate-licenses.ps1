$ErrorActionPreference = 'Stop'

# Run from the repository root after installing cargo-about 0.8.4.
$version = cargo about --version
if ($LASTEXITCODE -ne 0 -or $version -ne 'cargo-about 0.8.4') { throw 'cargo-about 0.8.4 is required' }
cargo about generate --locked --fail -o THIRD_PARTY_LICENSES.md about.hbs
if ($LASTEXITCODE -ne 0) { throw 'cargo-about failed' }
$metadata = cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
$upstream = Get-Content -Raw -Encoding utf8 'licenses/upstream-notices.json' | ConvertFrom-Json
$known = @{}
foreach ($record in $upstream) { foreach ($package in $record.packages) { $known[$package] = $true } }
$licensePath = Join-Path (Get-Location) 'THIRD_PARTY_LICENSES.md'
$generated = [IO.File]::ReadAllText($licensePath)
$pattern = '(?ms)^## MIT License\s+((?:- [^\r\n]+\r?\n)+)```text\s+MIT License\s+Copyright \(c\) <year> <copyright holders>'
foreach ($match in [regex]::Matches($generated, $pattern)) {
    foreach ($line in ($match.Groups[1].Value -split "`n")) {
        if ($line -match '^- (\S+) (\S+) \(') {
            $package = "$($Matches[1]) $($Matches[2])"
            if (-not $known.ContainsKey($package)) { throw "Missing verified upstream notice for $package" }
        }
    }
}
$generated = $generated.Replace('Copyright (c) <year> <copyright holders>', 'Copyright notices: see Upstream copyright notices below.')
[IO.File]::WriteAllText($licensePath, $generated, [Text.UTF8Encoding]::new($false))
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
[void]$append.AppendLine("### Font copyright metadata`n`n- Hack: Copyright (c) 2018 Source Foundry Authors; Copyright (c) 2003 Bitstream, Inc.`n- Noto Emoji: Copyright 2013 Google Inc.`n- Ubuntu Light: Copyright 2011 Canonical Ltd.`n")
[void]$append.AppendLine("## Upstream copyright notices`n")
foreach ($record in $upstream) {
    [void]$append.AppendLine("### $($record.packages -join ', ')`n`nSource: $($record.source)`n")
    foreach ($notice in $record.notices) { [void]$append.AppendLine("- $notice") }
    [void]$append.AppendLine("`n``````text")
    [void]$append.AppendLine($record.text)
    [void]$append.AppendLine("```````n")
}
$cfgPackage = @($metadata.packages | Where-Object { $_.name -eq 'cfg_aliases' -and $_.version -eq '0.2.2' })
if ($cfgPackage.Count -ne 1) { throw 'cfg_aliases notice needs review for the current version' }
$cfgNotice = Join-Path (Split-Path $cfgPackage[0].manifest_path) 'NOTICES.md'
[void]$append.AppendLine("### cfg_aliases 0.2.2 third-party notices`n")
[void]$append.AppendLine([IO.File]::ReadAllText($cfgNotice))
foreach ($name in @('lucide', 'ollama')) {
    [void]$append.AppendLine("### $name icons`n`n``````text")
    [void]$append.AppendLine([IO.File]::ReadAllText((Join-Path "assets/icons/$name" 'LICENSE')))
    [void]$append.AppendLine("```````n")
}
[IO.File]::AppendAllText((Join-Path (Get-Location) 'THIRD_PARTY_LICENSES.md'), ($append.ToString().TrimEnd() + "`n"), [Text.UTF8Encoding]::new($false))