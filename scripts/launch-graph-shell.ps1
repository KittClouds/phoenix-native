param(
    [Parameter(Mandatory = $true)]
    [string]$Binary,

    [Parameter(Mandatory = $true)]
    [string]$ScenePublicationRoot,

    [switch]$Wait
)

$ErrorActionPreference = 'Stop'

$resolvedBinary = (Resolve-Path -LiteralPath $Binary).Path
$resolvedRoot = (Resolve-Path -LiteralPath $ScenePublicationRoot).Path
$manifest = Join-Path $resolvedRoot 'current.pspm'

if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
    throw "PHOENIX_SCENE_PUBLICATION_MISSING: $manifest"
}

$arguments = @(
    '--scene-publication-root',
    $resolvedRoot,
    '--require-full-scene'
)

$process = Start-Process -FilePath $resolvedBinary -ArgumentList $arguments -PassThru
Write-Host "PHOENIX_GRAPH_SHELL_STARTED pid=$($process.Id) root=$resolvedRoot"

if ($Wait) {
    $process.WaitForExit()
    exit $process.ExitCode
}
