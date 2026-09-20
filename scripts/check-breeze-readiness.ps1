param([string]$OutputPath)
$ErrorActionPreference = 'Stop'
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$machine = Get-CimInstance Win32_ComputerSystem
$board = Get-CimInstance Win32_BaseBoard
$gpuRows = @(& nvidia-smi --query-gpu=name,driver_version,memory.total,memory.used,memory.free --format=csv,noheader,nounits)
if ($LASTEXITCODE -ne 0 -or $gpuRows.Count -eq 0) { throw 'GPU query failed' }
$gpu = $gpuRows[0].Split(',').Trim()
$wslStatus = ((& wsl --status 2>&1) -join "`n").Replace([string][char]0, '')
$wslExit = $LASTEXITCODE
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
$blockers = @()
if (-not $cpu.VirtualizationFirmwareEnabled -and -not $machine.HypervisorPresent) { $blockers += 'FIRMWARE_VIRTUALIZATION_DISABLED' }
if ($wslExit -ne 0) { $blockers += 'WSL_NOT_READY' }
if ([double]$gpu[4] -lt (7.7 * 1024)) { $blockers += 'FREE_VRAM_BELOW_PUBLISHED_EAGER_FOOTPRINT' }
$report = [ordered]@{
    schema = 'phoenix.breeze-readiness/v1'
    utc = [DateTime]::UtcNow.ToString('o')
    scope = 'Host readiness only; CUDA inside Linux and model inference are not qualified'
    gpu = @{ name=$gpu[0]; driver=$gpu[1]; totalMiB=[int]$gpu[2]; usedMiB=[int]$gpu[3]; freeMiB=[int]$gpu[4] }
    firmwareVirtualizationEnabled = $cpu.VirtualizationFirmwareEnabled
    hypervisorPresent = $machine.HypervisorPresent
    motherboard = "$($board.Manufacturer) $($board.Product)"
    elevated = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    wsl = @{ status=$wslStatus; exitCode=$wslExit }
    storage = @(Get-PSDrive -PSProvider FileSystem | Where-Object Name -in @('C','D') | ForEach-Object { @{ drive=$_.Name; freeBytes=$_.Free } })
    blockers = $blockers
    inferenceQualified = $false
}
$json = $report | ConvertTo-Json -Depth 6
if ($OutputPath) {
    $resolved = [IO.Path]::GetFullPath($OutputPath)
    if (Test-Path -LiteralPath $resolved) { throw 'Refusing to overwrite an existing readiness receipt' }
    [IO.File]::WriteAllText($resolved, $json)
}
$json
