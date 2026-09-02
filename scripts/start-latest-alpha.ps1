$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$desktopExecutable = Join-Path $repoRoot "target\debug\local-agent-stack-desktop.exe"

$runningDesktop = Get-CimInstance Win32_Process |
    Where-Object { $_.ExecutablePath -eq $desktopExecutable } |
    Select-Object -First 1

if ($runningDesktop) {
    $process = Get-Process -Id $runningDesktop.ProcessId -ErrorAction SilentlyContinue
    if ($process -and $process.MainWindowHandle -ne 0) {
        Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class LocalAgentStackWindow {
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);
}
"@
        [LocalAgentStackWindow]::ShowWindowAsync($process.MainWindowHandle, 9) | Out-Null
        [LocalAgentStackWindow]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
    }
    exit 0
}

Set-Location -LiteralPath $repoRoot
$corepack = "C:\Program Files\nodejs\corepack.cmd"
if (Test-Path -LiteralPath $corepack) {
    & $corepack pnpm dev
} else {
    $pnpm = (Get-Command pnpm.cmd -ErrorAction Stop).Source
    & $pnpm dev
}
exit $LASTEXITCODE
