param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^phoenix-[a-z0-9-]+-[0-9]{8}$')]
    [string] $Name,
    [string] $TargetDir = 'D:\phoenix-target-product-20260923'
)

$ErrorActionPreference = 'Stop'
$sourceRoot = Split-Path -Parent $PSScriptRoot
$targetRoot = [IO.Path]::GetFullPath($TargetDir)
if ([IO.Path]::GetPathRoot($targetRoot) -ne 'D:\') {
    throw 'Phoenix build artifacts must use the D: target drive.'
}
$stageRoot = Join-Path 'C:\phoenix-bin' $Name
$stagedExe = Join-Path $stageRoot 'phoenix-shell.exe'
$running = Get-CimInstance Win32_Process -Filter "Name = 'phoenix-shell.exe'" |
    Where-Object { $_.ExecutablePath -eq $stagedExe }
if ($running) {
    throw "Close the staged app before replacing it: $stagedExe"
}

Push-Location $sourceRoot
try {
    $head = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source revision.' }
    & cargo build --locked --release -p phoenix-shell --target-dir $targetRoot
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed; staged app was not replaced.' }
    if ((& git rev-parse HEAD).Trim() -ne $head) {
        throw 'Source revision changed during the build; stage again from a stable checkout.'
    }
    # Deliberately construct the release path here; callers cannot supply a debug executable.
    $builtExe = Join-Path $targetRoot 'release\phoenix-shell.exe'
    if (-not (Test-Path -LiteralPath $builtExe -PathType Leaf)) {
        throw "Release output missing: $builtExe"
    }
    $dirtyPaths = @(& git status --porcelain)
    New-Item -ItemType Directory -Path $stageRoot -Force | Out-Null
    Copy-Item -LiteralPath $builtExe -Destination $stagedExe -Force
    $builtHash = (Get-FileHash -LiteralPath $builtExe -Algorithm SHA256).Hash
    $stagedHash = (Get-FileHash -LiteralPath $stagedExe -Algorithm SHA256).Hash
    if ($builtHash -ne $stagedHash) { throw 'Staged executable hash mismatch.' }
    $receipt = [ordered]@{
        format = 'phoenix.release-stage/v1'
        profile = 'release'
        source_root = $sourceRoot
        source_head = $head
        source_dirty = $dirtyPaths.Count -gt 0
        source_status = $dirtyPaths
        target_executable = $builtExe
        staged_executable = $stagedExe
        sha256 = $stagedHash
        staged_utc = [DateTime]::UtcNow.ToString('o')
    }
    $receiptPath = Join-Path $stageRoot 'release-stage.json'
    $temporaryReceipt = "$receiptPath.tmp"
    $receipt | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $temporaryReceipt -Encoding utf8
    Move-Item -LiteralPath $temporaryReceipt -Destination $receiptPath -Force
    $receipt | ConvertTo-Json -Depth 4
} finally {
    Pop-Location
}
