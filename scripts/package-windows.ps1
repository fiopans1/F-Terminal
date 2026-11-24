param(
    [switch]$SkipBuild,
    [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw "The Windows installer must be built on Windows."
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$CargoToml = Join-Path $RepoRoot "Cargo.toml"
$VersionMatch = [regex]::Match(
    (Get-Content -Raw $CargoToml),
    '(?m)^version\s*=\s*"([^"]+)"'
)
if (-not $VersionMatch.Success) {
    throw "Could not read the version from Cargo.toml."
}
$Version = $VersionMatch.Groups[1].Value

$NumericMatch = [regex]::Match($Version, '^(\d+)\.(\d+)\.(\d+)')
if (-not $NumericMatch.Success) {
    throw "Unsupported version format: $Version (use X.Y.Z)."
}
$VersionInfo = "$($NumericMatch.Groups[1].Value).$($NumericMatch.Groups[2].Value).$($NumericMatch.Groups[3].Value).0"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo was not found on PATH."
}

if (-not $SkipBuild) {
    $rustup = Get-Command rustup -ErrorAction SilentlyContinue
    if ($rustup) {
        $installedTargets = & rustup target list --installed 2>$null
        if ($LASTEXITCODE -eq 0 -and $installedTargets -notcontains $Target) {
            throw "Rust target $Target is not installed. Run: rustup target add $Target"
        }
    }

    $rustFlags = $env:RUSTFLAGS
    if ($rustFlags -notmatch 'target-feature=\+crt-static') {
        $env:RUSTFLAGS = "$rustFlags -C target-feature=+crt-static".Trim()
    }

    Write-Host "[package] building f-terminal for $Target"
    & cargo build --locked --release --target $Target
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed."
    }
}

$Binary = Join-Path $RepoRoot "target\$Target\release\f-terminal.exe"
if (-not (Test-Path $Binary)) {
    throw "Missing release binary: $Binary (run without -SkipBuild)."
}

$Dist = Join-Path $RepoRoot "dist\windows"
$BuildDir = Join-Path $Dist "build"
if (Test-Path $BuildDir) {
    Remove-Item -Recurse -Force $BuildDir
}
New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null
Get-ChildItem -Path $Dist -Filter "*.exe" -File -ErrorAction SilentlyContinue |
    Remove-Item -Force
$Checksums = Join-Path $Dist "SHA256SUMS"
if (Test-Path $Checksums) {
    Remove-Item -Force $Checksums
}

$StagedExe = Join-Path $BuildDir "f-terminal.exe"
Copy-Item $Binary $StagedExe -Force

if ($env:WINDOWS_CERT_BASE64) {
    $CertificatePath = Join-Path $BuildDir "certificate.pfx"
    [System.IO.File]::WriteAllBytes(
        $CertificatePath,
        [Convert]::FromBase64String($env:WINDOWS_CERT_BASE64)
    )
    $env:WINDOWS_CERT_FILE = $CertificatePath
}

if ($env:REQUIRE_WINDOWS_SIGNING -eq "1" -and -not $env:WINDOWS_CERT_FILE) {
    throw "Windows code signing is required for release tags. Configure WINDOWS_CERT_BASE64 and WINDOWS_CERT_PASSWORD in the repository secrets."
}
if ($env:WINDOWS_CERT_FILE -and -not (Test-Path $env:WINDOWS_CERT_FILE)) {
    throw "WINDOWS_CERT_FILE does not exist: $($env:WINDOWS_CERT_FILE)"
}

$TimestampUrl = if ($env:WINDOWS_TIMESTAMP_URL) {
    $env:WINDOWS_TIMESTAMP_URL
} else {
    "http://timestamp.digicert.com"
}

function Find-InnoSetup {
    $candidates = @()
    if ($env:INNO_SETUP_COMPILER) {
        $candidates += $env:INNO_SETUP_COMPILER
    }
    if (${env:ProgramFiles(x86)}) {
        $candidates += Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
    }
    if ($env:ProgramFiles) {
        $candidates += Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"
    }
    if ($env:LOCALAPPDATA) {
        $candidates += Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"
    }
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) {
            return $candidate
        }
    }
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    return $null
}

$Iscc = Find-InnoSetup
if (-not $Iscc) {
    throw "Inno Setup 6 was not found. Install it or set INNO_SETUP_COMPILER to ISCC.exe."
}

function Get-SignTool {
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    $kits = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending
    foreach ($kit in $kits) {
        $candidate = Join-Path $kit.FullName "x64\signtool.exe"
        if (Test-Path $candidate) {
            return $candidate
        }
    }
    return $null
}

function Sign-File([string]$Path) {
    if (-not $env:WINDOWS_CERT_FILE) {
        return
    }
    if (-not (Test-Path $env:WINDOWS_CERT_FILE)) {
        throw "WINDOWS_CERT_FILE does not exist: $($env:WINDOWS_CERT_FILE)"
    }
    $signtool = Get-SignTool
    if (-not $signtool) {
        throw "WINDOWS_CERT_FILE is set but signtool.exe was not found."
    }
    Write-Host "[package] signing $Path"
    $arguments = @(
        "sign", "/fd", "SHA256", "/td", "SHA256",
        "/tr", $TimestampUrl,
        "/f", $env:WINDOWS_CERT_FILE
    )
    if ($env:WINDOWS_CERT_PASSWORD) {
        $arguments += @("/p", $env:WINDOWS_CERT_PASSWORD)
    }
    $arguments += $Path
    & $signtool @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "signtool failed for $Path"
    }
}

function Verify-Signature([string]$Path) {
    if (-not $env:WINDOWS_CERT_FILE) {
        return
    }
    $signtool = Get-SignTool
    if (-not $signtool) {
        throw "WINDOWS_CERT_FILE is set but signtool.exe was not found."
    }
    Write-Host "[package] verifying signature for $Path"
    & $signtool verify /pa /v $Path
    if ($LASTEXITCODE -ne 0) {
        throw "signature verification failed for $Path"
    }
}

Sign-File $StagedExe
Verify-Signature $StagedExe

$SignToolDirective = ""
$IsccSignArgs = @()
if ($env:WINDOWS_CERT_FILE) {
    $signtool = Get-SignTool
    if (-not $signtool) {
        throw "WINDOWS_CERT_FILE is set but signtool.exe was not found."
    }

    # Inno Setup signs the generated uninstaller during compilation. Use a
    # tiny wrapper so the certificate password never appears on the ISCC
    # command line (and therefore never in the build log).
    $env:WINDOWS_SIGNTOOL = $signtool
    $env:WINDOWS_TIMESTAMP_URL = $TimestampUrl
    $wrapperLines = @(
        'param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Files)',
        '$arguments = @(',
        '    "sign", "/fd", "SHA256", "/td", "SHA256",',
        '    "/tr", $env:WINDOWS_TIMESTAMP_URL,',
        '    "/f", $env:WINDOWS_CERT_FILE',
        ')',
        'if ($env:WINDOWS_CERT_PASSWORD) {',
        '    $arguments += @("/p", $env:WINDOWS_CERT_PASSWORD)',
        '}',
        '$arguments += $Files',
        '& $env:WINDOWS_SIGNTOOL @arguments',
        'exit $LASTEXITCODE'
    )
    $SignWrapper = Join-Path $BuildDir "sign-uninstaller.ps1"
    [System.IO.File]::WriteAllText(
        $SignWrapper,
        (($wrapperLines -join "`r`n") + "`r`n"),
        [System.Text.Encoding]::ASCII
    )

    # `$q` and `$f` are Inno placeholders for a quote and the file being signed.
    $SignToolDirective = "SignTool=signtool`r`nSignedUninstaller=yes"
    $signCommand = '$q' + (Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe") + '$q -NoProfile -ExecutionPolicy Bypass -File $q' + $SignWrapper + '$q $f'
    $IsccSignArgs = @("/Ssigntool=$signCommand")
}

$Template = Get-Content -Raw (Join-Path $RepoRoot "scripts\windows\f-terminal.iss")
$GeneratedIss = $Template
$GeneratedIss = $GeneratedIss.Replace("@APP_VERSION@", $Version)
$GeneratedIss = $GeneratedIss.Replace("@VERSION_INFO@", $VersionInfo)
$GeneratedIss = $GeneratedIss.Replace("@REPO_ROOT@", $RepoRoot)
$GeneratedIss = $GeneratedIss.Replace("@SOURCE_EXE@", $StagedExe)
$GeneratedIss = $GeneratedIss.Replace("@OUTPUT_DIR@", $Dist)
$GeneratedIss = $GeneratedIss.Replace("@SIGNTOOL_DIRECTIVE@", $SignToolDirective)
$GeneratedPath = Join-Path $BuildDir "f-terminal.generated.iss"
[System.IO.File]::WriteAllText(
    $GeneratedPath,
    $GeneratedIss,
    [System.Text.UTF8Encoding]::new($true)
)

Write-Host "[package] compiling Inno Setup installer"
& $Iscc @IsccSignArgs $GeneratedPath
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup compilation failed."
}

$Setup = Join-Path $Dist "F-Terminal-Setup-$Version-x64.exe"
if (-not (Test-Path $Setup)) {
    throw "Expected installer was not produced: $Setup"
}

# Inno Setup signs Setup, its temporary self-copies and the uninstaller while
# compiling (SignTool directive above). Only verify the final artifact here.
Verify-Signature $Setup

if ($env:WINDOWS_CERT_BASE64 -and (Test-Path $CertificatePath)) {
    Remove-Item -Force $CertificatePath
}

$Hash = (Get-FileHash -Algorithm SHA256 $Setup).Hash.ToLowerInvariant()
Set-Content -Path (Join-Path $Dist "SHA256SUMS") `
    -Value "$Hash  $(Split-Path $Setup -Leaf)" `
    -Encoding ASCII

Write-Host "[package] installer: $Setup"
