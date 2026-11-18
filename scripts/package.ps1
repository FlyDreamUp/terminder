param([string]$TargetDirectory = "", [string[]]$CargoArguments = @())
$ErrorActionPreference = 'Stop'
for ($argumentIndex = 0; $argumentIndex -lt $CargoArguments.Count; $argumentIndex++) {
    $argument = $CargoArguments[$argumentIndex]
    if ($argument -eq '--config') {
        $argumentIndex++
        if ($argumentIndex -ge $CargoArguments.Count) { throw '--config requires a value' }
    } elseif ($argument -notin @('--offline', '--frozen', '--locked') -and -not $argument.StartsWith('--config=')) {
        throw "Packaging only accepts Cargo cache/config flags; feature/target overrides are not allowed: $argument"
    }
}
$root = Split-Path -Parent $PSScriptRoot
if (-not $TargetDirectory) { $TargetDirectory = Join-Path $root 'target' }
$TargetDirectory = [IO.Path]::GetFullPath($TargetDirectory)
Push-Location $root
try {
    # Always build ordinary release features; test-support must never enter distribution.
    & cargo build -p terminder-desktop --release --locked --target-dir $TargetDirectory @CargoArguments
    if ($LASTEXITCODE -ne 0) { throw 'Native release build failed' }
    & (Join-Path $PSScriptRoot 'release-notices.ps1') -CargoArguments $CargoArguments
    $metadata = (& cargo metadata --no-deps --locked --format-version=1 @CargoArguments) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Version metadata failed' }
    $version = ($metadata.packages | Where-Object { $_.name -eq 'terminder-desktop' }).version
    $packageRoot = Join-Path $root 'target/package'
    $staging = Join-Path $packageRoot "Terminder-$version-windows-x64"
    [IO.Directory]::CreateDirectory($staging) | Out-Null
    Copy-Item -LiteralPath (Join-Path $TargetDirectory 'release/terminder-desktop.exe') -Destination (Join-Path $staging 'terminder-desktop.exe')
    $conptyRoot = Join-Path $root 'vendor/microsoft-conpty'
    $conptyManifest = Get-Content -LiteralPath (Join-Path $conptyRoot 'manifest.json') -Raw | ConvertFrom-Json
    foreach ($file in $conptyManifest.files) {
        $source = Join-Path $TargetDirectory (Join-Path 'release' ([IO.Path]::GetFileName($file.path)))
        $actual = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $file.sha256) { throw "ConPTY package hash mismatch: $source" }
        Copy-Item -LiteralPath $source -Destination $staging
    }
    Copy-Item -LiteralPath (Join-Path $conptyRoot 'LICENSE') -Destination (Join-Path $staging 'CONPTY-LICENSE.txt')
    Copy-Item -LiteralPath (Join-Path $conptyRoot 'manifest.json') -Destination (Join-Path $staging 'conpty-manifest.json')
    Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination $staging
    Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination $staging
    Copy-Item -LiteralPath (Join-Path $root 'target/release-notices/THIRD-PARTY-NOTICES.txt') -Destination $staging
    Copy-Item -LiteralPath (Join-Path $root 'target/release-notices/dependency-inventory.json') -Destination $staging
    $archive = "$staging.zip"
    $packageFiles = @('terminder-desktop.exe','conpty.dll','OpenConsole.exe','CONPTY-LICENSE.txt','conpty-manifest.json','LICENSE','README.md','THIRD-PARTY-NOTICES.txt','dependency-inventory.json') | ForEach-Object { Join-Path $staging $_ }
    Compress-Archive -LiteralPath $packageFiles -DestinationPath $archive -Force
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$archive.sha256","$hash  $([IO.Path]::GetFileName($archive))`n")
    Write-Output "Created $archive"
} finally { Pop-Location }
