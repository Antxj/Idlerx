# Builds the official release: runs the tests, builds Idlerx without local
# machine paths and checks the result. Used by the release workflow and for
# local builds.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$crateRoot = (Resolve-Path $PSScriptRoot).Path.TrimEnd('\')
$userRoot = if ($env:USERPROFILE) {
    $env:USERPROFILE.TrimEnd('\')
} else {
    [Environment]::GetFolderPath("UserProfile").TrimEnd('\')
}
$cargoRoot = if ($env:CARGO_HOME) {
    (Resolve-Path $env:CARGO_HOME).Path.TrimEnd('\')
} else {
    (Join-Path $userRoot ".cargo").TrimEnd('\')
}
$targetRoot = (Join-Path $crateRoot "target").TrimEnd('\')

# Cargo uses ASCII unit separator so paths containing spaces remain one rustc
# argument. CARGO_ENCODED_RUSTFLAGS replaces .cargo/config.toml, so the
# static C runtime flag is repeated here.
$separator = [char]0x1f
$flags = @(
    "-Ctarget-feature=+crt-static"
    "--remap-path-prefix=$userRoot=home"
    "--remap-path-prefix=$cargoRoot=cargo"
    "--remap-path-prefix=$crateRoot=src"
    "--remap-path-prefix=$targetRoot=target"
)
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join $separator

Push-Location $crateRoot
try {
    cargo test --locked
    if ($LASTEXITCODE -ne 0) {
        throw "The tests failed. The release was not built."
    }

    cargo clean --release
    if ($LASTEXITCODE -ne 0) {
        throw "Could not clean old release artifacts."
    }

    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "The release build failed."
    }

    $binary = Join-Path $targetRoot "release\idlerx.exe"
    $bytes = [IO.File]::ReadAllBytes($binary)
    $singleByteText = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    $utf16Text = [Text.Encoding]::Unicode.GetString($bytes)
    $forbidden = @($userRoot, $crateRoot, $env:USERNAME, $env:COMPUTERNAME) |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
        Select-Object -Unique

    foreach ($value in $forbidden) {
        $comparison = [StringComparison]::OrdinalIgnoreCase
        if ($singleByteText.IndexOf($value, $comparison) -ge 0 -or
            $utf16Text.IndexOf($value, $comparison) -ge 0) {
            throw "The executable contains local build information: $value"
        }
    }

    # a single file: no Visual C++ runtime or WebView2Loader.dll next to it
    foreach ($dll in @("VCRUNTIME140.dll", "WebView2Loader.dll", "MSVCP140.dll")) {
        if ($singleByteText.IndexOf($dll, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            throw "The executable depends on $dll (it must be linked statically)."
        }
    }

    $hash = (Get-FileHash -Algorithm SHA256 $binary).Hash
    Write-Host "Release verified: $binary"
    Write-Host "SHA256: $hash"
} finally {
    Pop-Location
}
