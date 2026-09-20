param(
    [string]$TargetDirectory = 'D:\phoenix-target-reader-contract',
    [string]$TestLink = 'C:\phoenix-bin\reader-contracts-proof-20260904',
    [switch]$Benchmarks,
    [switch]$DeviceSmoke
)
$ErrorActionPreference = 'Stop'
$manifest = Join-Path $PSScriptRoot '..\Cargo.toml'
$packages = @('-p', 'phoenix-tts-contract', '-p', 'phoenix-reader-session', '-p', 'phoenix-audio', '-p', 'phoenix-tts-native')
$target = [IO.Path]::GetFullPath($TargetDirectory)
$link = [IO.Path]::GetFullPath($TestLink)
if (-not $target.StartsWith('D:\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Reader build target must be on D:' }
if (-not $link.StartsWith('C:\phoenix-bin\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Reader test link must be under C:\phoenix-bin' }
$previousTarget = $env:CARGO_TARGET_DIR
try {
    $env:CARGO_TARGET_DIR = $target
    & cargo fmt --manifest-path $manifest @packages -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Reader formatting failed' }
    & cargo clippy --manifest-path $manifest @packages --all-targets --no-deps -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Reader clippy failed' }
    $messages = & cargo test --manifest-path $manifest @packages --no-run --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Reader test build failed' }
    if (Test-Path -LiteralPath $link) {
        $existing = Get-Item -LiteralPath $link
        if ($existing.LinkType -ne 'Junction' -or $existing.Target -ne $target) { throw 'Existing test link has different authority' }
    } else {
        New-Item -ItemType Junction -Path $link -Target $target | Out-Null
    }
    $executables = @($messages | ForEach-Object {
        $message = $_ | ConvertFrom-Json
        if ($message.reason -eq 'compiler-artifact' -and $message.profile.test -and $message.executable) { $message.executable }
    })
    if ($executables.Count -eq 0) { throw 'No Reader test artifacts' }
    $receipts = @()
    foreach ($executable in $executables) {
        $relative = [IO.Path]::GetRelativePath($target, $executable)
        if ($relative.StartsWith('..') -or [IO.Path]::IsPathRooted($relative)) { throw 'Test executable escaped target' }
        $linkedExecutable = Join-Path $link $relative
        & $linkedExecutable
        if ($LASTEXITCODE -ne 0) { throw "Reader tests failed: $linkedExecutable" }
        $receipts += [pscustomobject]@{ path = $linkedExecutable; sha256 = (Get-FileHash -LiteralPath $linkedExecutable -Algorithm SHA256).Hash; exitCode = 0 }
    }
    $deviceReceipts = @()
    if ($DeviceSmoke) {
        $messages = & cargo build --manifest-path $manifest -p phoenix-audio --example device_smoke -p phoenix-reader-session --example reader_smoke --message-format=json
        if ($LASTEXITCODE -ne 0) { throw 'Reader device smoke build failed' }
        foreach ($line in $messages) {
            $message = $line | ConvertFrom-Json
            if ($message.reason -ne 'compiler-artifact' -or -not $message.executable -or $message.target.kind -notcontains 'example') { continue }
            $relative = [IO.Path]::GetRelativePath($target, $message.executable)
            if ($relative.StartsWith('..') -or [IO.Path]::IsPathRooted($relative)) { throw 'Device smoke escaped target' }
            $executable = Join-Path $link $relative
            & $executable
            if ($LASTEXITCODE -ne 0) { throw "Device smoke failed: $executable" }
            $deviceReceipts += [pscustomobject]@{ path = $executable; sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash; exitCode = 0 }
        }
        if ($deviceReceipts.Count -ne 2) { throw 'Expected both device smoke artifacts' }
    }
    if ($Benchmarks) {
        $messages = & cargo bench --manifest-path $manifest @packages --no-run --message-format=json
        if ($LASTEXITCODE -ne 0) { throw 'Reader benchmark build failed' }
        foreach ($line in $messages) {
            $message = $line | ConvertFrom-Json
            if ($message.reason -ne 'compiler-artifact' -or -not $message.executable -or $message.target.kind -notcontains 'bench') { continue }
            $relative = [IO.Path]::GetRelativePath($target, $message.executable)
            if ($relative.StartsWith('..') -or [IO.Path]::IsPathRooted($relative)) { throw 'Benchmark escaped target' }
            & (Join-Path $link $relative) --bench --sample-size 20 --warm-up-time 1 --measurement-time 2 --noplot
            if ($LASTEXITCODE -ne 0) { throw 'Reader benchmark failed' }
        }
    }
    [pscustomobject]@{ contract = 'phoenix.reader/v1'; utc = [DateTime]::UtcNow.ToString('o');
        target = $target; testLink = $link; tests = $receipts; benchmarksRun = [bool]$Benchmarks;
        deviceSmoke = $deviceReceipts;
        scope = 'Contract and optional silent Windows device smoke proof; no Breeze inference, audible quality, gapless playback or UI qualification' } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $target 'reader-contract-proof.json') -Encoding utf8
} finally {
    $env:CARGO_TARGET_DIR = $previousTarget
}
