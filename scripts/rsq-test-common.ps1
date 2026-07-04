# Shared helpers for RSQ local test scripts (PID tracking only).

$script:RsqOwnedPids = [System.Collections.Generic.List[int]]::new()

function Add-RsqOwnedPid {
    param([int]$ProcessId)
    if ($ProcessId -gt 0) {
        [void]$script:RsqOwnedPids.Add($ProcessId)
    }
}

function Stop-RsqOwned {
    param([int[]]$Except = @())
    $toRemove = @()
    foreach ($procId in @($script:RsqOwnedPids)) {
        if ($Except -contains $procId) { continue }
        Stop-Process -Id $procId -Force -ErrorAction SilentlyContinue
        $toRemove += $procId
    }
    foreach ($procId in $toRemove) {
        [void]$script:RsqOwnedPids.Remove($procId)
    }
    if ($toRemove.Count -gt 0) {
        Start-Sleep -Seconds 1
    }
}
