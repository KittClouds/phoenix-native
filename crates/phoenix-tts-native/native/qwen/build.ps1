param(
    [string]$Qwentts = 'C:\phoenix-tts\qwentts.cpp',
    [string]$Out = 'C:\phoenix-tts\qwen-worker-build',
    [string]$Breeze = 'D:\phoenix-tts\breeze-native-20260906'
)
$ErrorActionPreference = 'Stop'
# Vulkan build of phoenix-qwen-worker inside a qwentts.cpp checkout (commit in
# QWENTTS_COMMIT, qwentts.patch applied: QT_MAX_CTX and the worker target),
# using the Breeze native toolchain (Visual Studio 18, self-built glslc,
# Vulkan headers and import library).
$vs = 'C:\Program Files\Microsoft Visual Studio\18\Community'
$vars = & cmd.exe /d /c "$Breeze\vs-env.cmd"
if ($LASTEXITCODE -ne 0) { throw 'Visual Studio environment failed' }
foreach ($line in $vars) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process') }
}
$cmake = "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
$env:PATH = "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;$env:PATH"
& $cmake -S $Qwentts -B $Out -G Ninja -DCMAKE_BUILD_TYPE=Release -DGGML_VULKAN=ON `
    "-DPHOENIX_QWEN_WORKER_SOURCE=$PSScriptRoot/qwen_worker.cpp" `
    "-DVulkan_INCLUDE_DIR=$Breeze/Vulkan-Headers/include" `
    "-DVulkan_LIBRARY=$Breeze/sdk/vulkan-1.lib" `
    "-DVulkan_GLSLC_EXECUTABLE=$Breeze/shaderc-build/glslc/glslc.exe" `
    "-DCMAKE_PREFIX_PATH=$Breeze/sdk" `
    "-DCMAKE_CXX_FLAGS=/I$Breeze/sdk/include /EHsc"
if ($LASTEXITCODE -ne 0) { throw 'CMake configure failed' }
& $cmake --build $Out --target phoenix-qwen-worker --parallel 8
if ($LASTEXITCODE -ne 0) { throw 'CMake build failed' }
