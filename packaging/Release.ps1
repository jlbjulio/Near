param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $root
try {
    $logRoot=Join-Path $root 'evidence'
    New-Item -ItemType Directory -Path $logRoot -Force | Out-Null
    if(-not $SkipBuild) {
        $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
        $installation=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if(-not $installation){throw 'Microsoft C++ Build Tools not found.'}
        $setup=Join-Path $installation 'VC\Auxiliary\Build\vcvars64.bat'
        cmd /c "`"$setup`" >nul && set" | ForEach-Object {
            if($_ -match '^([^=]+)=(.*)$'){[Environment]::SetEnvironmentVariable($matches[1],$matches[2],'Process')}
        }
        & cargo fmt --all -- --check *> (Join-Path $logRoot 'release-format.log')
        if($LASTEXITCODE){throw 'Formatting failed. See evidence/release-format.log.'}
        & cargo test --locked --quiet *> (Join-Path $logRoot 'release-tests.log')
        if($LASTEXITCODE){throw 'Tests failed. See evidence/release-tests.log.'}
        & cargo build --locked --release --quiet *> (Join-Path $logRoot 'release-build.log')
        if($LASTEXITCODE){throw 'Build failed. See evidence/release-build.log.'}
    }
    $version=(Select-String -LiteralPath (Join-Path $root 'Cargo.toml') -Pattern '^version = "([^"]+)"$').Matches[0].Groups[1].Value
    $release=Join-Path $root 'release'
    $bundle=Join-Path $root "packaging\work\Near-$version-windows-x64"
    $binaryZip=Join-Path $release "Near-$version-windows-x64.zip"
    if(Test-Path -LiteralPath $binaryZip){throw 'Release output already exists. Archive it before rebuilding.'}
    New-Item -ItemType Directory -Path $release,$bundle -Force | Out-Null
    Copy-Item -LiteralPath 'target\release\Near.exe','LICENSE','THIRD-PARTY-NOTICES.md' -Destination $bundle
    Copy-Item -LiteralPath 'licenses' -Destination $bundle -Recurse
    Compress-Archive -LiteralPath (Get-ChildItem -LiteralPath $bundle).FullName -DestinationPath $binaryZip -CompressionLevel Optimal
    Copy-Item -LiteralPath 'RELEASE.md' -Destination (Join-Path $release "Near-$version-release.md")
    @('target\release\Near.exe',$binaryZip) | ForEach-Object {
        $hash=Get-FileHash -LiteralPath $_ -Algorithm SHA256
        "$($hash.Hash.ToLowerInvariant())  $([IO.Path]::GetFileName($hash.Path))"
    } | Set-Content -LiteralPath (Join-Path $release 'SHA256SUMS.txt') -Encoding ascii
    "Release prepared: $binaryZip"
} finally {Pop-Location}
