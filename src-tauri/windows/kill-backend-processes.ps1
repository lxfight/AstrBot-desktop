param(
    [Parameter(Mandatory = $false)]
    [string]$InstallDir
)

$ErrorActionPreference = "SilentlyContinue"

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    exit 0
}

try {
    $installRoot = [System.IO.Path]::GetFullPath($InstallDir).TrimEnd([char]92).ToLowerInvariant()
} catch {
    exit 0
}

if ([string]::IsNullOrWhiteSpace($installRoot)) {
    exit 0
}

$installRootWithSep = $installRoot + [string][char]92
$currentPid = $PID
$commandLineFallbackNames = @(
    "python.exe",
    "pythonw.exe",
    "astrbot-desktop-tauri.exe",
    "astrbot.exe"
)

Get-CimInstance Win32_Process |
    ForEach-Object {
        if ($_.ProcessId -eq $currentPid) {
            return
        }

        $shouldStop = $false

        try {
            if ($_.ExecutablePath) {
                $exePath = [System.IO.Path]::GetFullPath($_.ExecutablePath).TrimEnd([char]92).ToLowerInvariant()
                if ($exePath -eq $installRoot -or $exePath.StartsWith($installRootWithSep)) {
                    $shouldStop = $true
                }
            }
        } catch {
            # Ignore per-process executable path errors and continue cleanup.
        }

        if (-not $shouldStop) {
            try {
                $name = [string]$_.Name
                $nameLower = $name.ToLowerInvariant()
                if ($_.CommandLine -and ($commandLineFallbackNames -contains $nameLower)) {
                    $commandLine = [string]$_.CommandLine
                    $commandLineLower = $commandLine.ToLowerInvariant()
                    if ($commandLineLower.Contains($installRootWithSep) -or $commandLineLower.Contains($installRoot)) {
                        $shouldStop = $true
                    }
                }
            } catch {
                # Ignore per-process command line errors and continue cleanup.
            }
        }

        if ($shouldStop) {
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }
