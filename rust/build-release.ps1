# Builds the official release: runs the tests, builds Idlerx without local
# machine paths and checks the result. Used by the release workflow and for
# local builds.
[CmdletBinding()]
param(
    # builds without Idlerx's signed Firefox extension (only to test the
    # build: such an executable cannot install its extension)
    [switch]$AllowUnsigned
)

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

# The official OpenVPN installer, embedded for the VPN of groups. It is not
# in the repository: it is downloaded from the official server and verified
# by SHA-256 and by its Authenticode signature (OpenVPN Inc.) before going
# in. Keep the version in sync with build.rs.
$msiVersion = "2.7.6-I001"
$msiUrl = "https://build.openvpn.net/downloads/releases/OpenVPN-$msiVersion-amd64.msi"
$msiSha256 = "48C96AC092A81C6303F059EACE51C7BF878794E9575B329E82894848E0E529FC"
$msiPath = Join-Path $crateRoot "assets\openvpn.msi"
if (-not (Test-Path $msiPath) -or
    (Get-FileHash -Algorithm SHA256 $msiPath).Hash -ne $msiSha256) {
    Write-Host "Downloading the official OpenVPN installer ($msiVersion)..."
    Invoke-WebRequest -Uri $msiUrl -OutFile $msiPath -UseBasicParsing -TimeoutSec 300
}
$msiHash = (Get-FileHash -Algorithm SHA256 $msiPath).Hash
if ($msiHash -ne $msiSha256) {
    Remove-Item $msiPath
    throw "OpenVPN installer SHA-256 does not match (expected $msiSha256, got $msiHash)."
}
$signature = Get-AuthenticodeSignature $msiPath
if ($signature.Status -ne "Valid" -or $signature.SignerCertificate.Subject -notmatch "OpenVPN") {
    throw "Invalid OpenVPN installer signature or another publisher: $($signature.Status)"
}
Write-Host "OpenVPN installer verified (SHA-256 and OpenVPN Inc. signature)."

# Idlerx's Firefox extension, signed by Mozilla (made by the release
# workflow, see release.yml). Firefox only installs signed extensions.
$xpiPath = Join-Path $crateRoot "assets\idlerx-firefox.xpi"
if (Test-Path $xpiPath) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($xpiPath)
    try {
        $signed = @($zip.Entries | Where-Object { $_.FullName -match '^META-INF/(mozilla\.rsa|cose\.sig)$' }).Count -gt 0
    } finally {
        $zip.Dispose()
    }
    if (-not $signed) { throw "assets\idlerx-firefox.xpi is not signed by Mozilla." }
    Write-Host "Firefox extension: signed."
} elseif (-not $AllowUnsigned) {
    throw "Missing assets\idlerx-firefox.xpi (Idlerx's Firefox extension signed by Mozilla)."
} else {
    Write-Warning "Building without the signed Firefox extension (test build)."
}

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
    $latin1 = [Text.Encoding]::GetEncoding(28591)
    $singleByteText = $latin1.GetString($bytes)
    # the embedded OpenVPN installer is checked by its own hash and signature;
    # the checks below look at Idlerx's own code only
    $msiText = $latin1.GetString([IO.File]::ReadAllBytes($msiPath))
    $msiAt = $singleByteText.IndexOf($msiText, [StringComparison]::Ordinal)
    if ($msiAt -lt 0) {
        throw "The OpenVPN installer was not embedded in the executable."
    }
    $singleByteText = $singleByteText.Remove($msiAt, $msiText.Length)
    $utf16Text = [Text.Encoding]::Unicode.GetString($latin1.GetBytes($singleByteText))
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

    # a single file: no Visual C++ runtime next to it
    foreach ($dll in @("VCRUNTIME140.dll", "MSVCP140.dll")) {
        if ($singleByteText.IndexOf($dll, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            throw "The executable depends on $dll (it must be linked statically)."
        }
    }

    $hash = (Get-FileHash -Algorithm SHA256 $binary).Hash
    Write-Host "Release verified: $binary"
    Write-Host "Embedded OpenVPN installer: $msiVersion"
    Write-Host "SHA256: $hash"
} finally {
    Pop-Location
}
