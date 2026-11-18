param([string[]]$CargoArguments = @(), [string]$OutputDirectory = "")
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'target/release-notices' }
Push-Location $root
try {
    $metadataText = & cargo metadata --locked --format-version=1 --filter-platform=x86_64-pc-windows-msvc @CargoArguments
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
    $metadata = $metadataText | ConvertFrom-Json
    $supplements = Get-Content -LiteralPath (Join-Path $root 'docs/third_party/notices/sources.json') -Raw | ConvertFrom-Json
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $text = New-Object System.Text.StringBuilder
    [void]$text.AppendLine([IO.File]::ReadAllText((Join-Path $root 'LICENSE')))
    [void]$text.AppendLine('Terminder third-party notices. Generated from Cargo.lock; includes build/test dependencies as a conservative superset.')
    $inventory = @()
    foreach ($package in @($metadata.packages | Where-Object { $_.source } | Sort-Object name,version)) {
        $directory = Split-Path -Parent $package.manifest_path
        $files = @(Get-ChildItem -LiteralPath $directory -File | Where-Object { $_.Name -match '^(licen[sc]e|copying|copyright|notice)([-_.]|$)' } | ForEach-Object { $_.FullName })
        if ($package.license_file) {
            $declared = Join-Path $directory $package.license_file
            if ($files -notcontains $declared) { $files += $declared }
        }
        $source = "https://crates.io/api/v1/crates/$($package.name)/$($package.version)/download"
        [void]$text.AppendLine("`n========================================================================`n$($package.name) $($package.version)`nLicense: $($package.license)`nSource: $source")
        $notices = @()
        $supplement = $null
        foreach ($file in $files) {
            [void]$text.AppendLine("--- $([IO.Path]::GetFileName($file)) ---")
            [void]$text.AppendLine([IO.File]::ReadAllText($file))
            $notices += [IO.Path]::GetFileName($file)
        }
        if ($files.Count -eq 0) {
            $supplement = $supplements | Where-Object { $_.name -eq $package.name -and $_.version -eq $package.version } | Select-Object -First 1
            if (-not $supplement) { throw "Missing upstream license notice: $($package.name) $($package.version)" }
            $file = Join-Path $root "docs/third_party/notices/$($supplement.file)"
            if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $supplement.sha256) { throw "Notice checksum mismatch: $file" }
            [void]$text.AppendLine("Supplement source: $($supplement.source)")
            if ($supplement.note) { [void]$text.AppendLine($supplement.note) }
            [void]$text.AppendLine([IO.File]::ReadAllText($file))
            $notices += "supplement:$($supplement.file)"
        }
        $inventory += [ordered]@{ name=$package.name; version=$package.version; license=$package.license; repository=$package.repository; sourceArchive=$source; noticeFiles=$notices; supplement=$supplement }
    }
    [IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'THIRD-PARTY-NOTICES.txt'),$text.ToString(),$utf8)
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'dependency-inventory.json'),($inventory | ConvertTo-Json -Depth 8),$utf8)
    Write-Output "Recorded $($inventory.Count) locked Rust dependencies in $OutputDirectory"
} finally { Pop-Location }
