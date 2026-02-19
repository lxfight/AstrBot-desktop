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

Get-CimInstance Win32_Process -Filter "Name='python.exe' OR Name='pythonw.exe'" |
    Where-Object { $_.ExecutablePath } |
    ForEach-Object {
        try {
            $exePath = [System.IO.Path]::GetFullPath($_.ExecutablePath).ToLowerInvariant()
            if ($exePath -eq $installRoot -or $exePath.StartsWith($installRootWithSep)) {
                Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
            }
        } catch {
            # Ignore per-process path errors and continue cleanup.
        }
    }
