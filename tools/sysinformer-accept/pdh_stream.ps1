# PS script must be pure ASCII (per AGENTS.md)
# Stream PDH % Processor Time (_Total + per core) to JSONL, stamped with the
# counter's OWN sample timestamp.
#
# Critical: each Get-Counter call takes 1000-2840ms (measured). The
# CookedValue it returns describes a window that ENDS at the end of that
# call, NOT at the moment we stamped it. Verified:
#
#   tBefore       tAfter      callMs   sampleTs    (ms since epoch)
#   ...979834     ...982686    2840     ...982678   <- sampleTs ~= tAfter
#   ...982966     ...983968    1001     ...983967   <- sampleTs ~= tAfter
#
# So stamping with DateTimeOffset.UtcNow BEFORE the call is off by 1-2.8s.
# We must use the counter's own $s.Timestamp, otherwise the consumer's
# time-alignment silently pairs the wrong instants.
#
# Output line format:
#   <unix_ms>{"_Total":17.36,"0":31.4,...}
param(
    [string]$Out = 'D:\ch\project\.tmp\sysinformer-check\pdh_stream.jsonl',
    [int]$DurationSec = 40
)

$ErrorActionPreference = 'Stop'
if (Test-Path $Out) { Remove-Item $Out -Force }

$epoch = [DateTime]::UnixEpoch
$lines = New-Object System.Collections.Generic.List[string]
$sw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sw.Elapsed.TotalSeconds -lt $DurationSec) {
    $c = Get-Counter '\Processor(*)\% Processor Time'
    # Use the first sample's timestamp as this round's stamp (all instances
    # in one Get-Counter share the same collection window).
    $stampMs = [int64](([DateTime]$c.CounterSamples[0].Timestamp).ToUniversalTime() - $epoch).TotalMilliseconds
    $parts = @()
    foreach ($s in $c.CounterSamples) {
        if ($s.Path -match '\\Processor\(([^)]+)\)\\?% Processor Time$') {
            $parts += ('"{0}":{1}' -f $Matches[1], [math]::Round([double]$s.CookedValue, 4))
        }
    }
    if ($parts.Count -gt 0) {
        $lines.Add(('{0}{{{1}}}' -f $stampMs, ($parts -join ',')))
    }
}

[System.IO.File]::WriteAllLines($Out, $lines)
Write-Output ('WROTE=' + $Out)
Write-Output ('SAMPLES=' + $lines.Count)
