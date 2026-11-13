# PS script must be pure ASCII (per AGENTS.md)
# Stream PDH % Processor Time with a LONG-LIVED query and proper warm-up.
#
# Why this rewrite (2026-10-01):
#   The previous version called Get-Counter once per line, i.e. **每轮都新开查询**。
#   PDH's CookedValue is (raw_now - raw_first) / (t_now - t_first), where
#   raw_first/t_first come from the query's own previous sample. On a freshly
#   opened query that baseline is not settled, so **every emitted value is a
#   first-sample value**. That systematically biases the reference low, which
#   showed up as the library reading ~+16pp "too high" on the 4-core CI runner
#   while the same judgement passed locally.
#
#   Fix: build the counters once, prime each with NextValue() (establishing the
#   baseline), then loop with a sleep so every emitted value spans a full,
#   settled interval. Stamp with the wall clock at the moment of the *read*
#   (not before the Get-Counter call, which took 1000-2840ms in earlier
#   measurements and shifted the stamp by 1-2.8s).
#
# Output line format:
#   <unix_ms>{"_total":53.6,"0":31.4,"1":48.5,...}
param(
    [string]$Out = 'D:\ch\project\.tmp\sysinformer-check\pdh_stream.jsonl',
    [int]$DurationSec = 40,
    [int]$IntervalMs = 1000,
    [int]$Ncpu = 0
)

$ErrorActionPreference = 'Stop'
if (Test-Path $Out) { Remove-Item $Out -Force }

if ($Ncpu -le 0) {
    $Ncpu = [int]((Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors)
}

# ---- kernel-number probes (2026-10-01) --------------------------------
# .NET scales "% Processor Time" cooked values by Environment.ProcessorCount,
# while the library reads the raw counter through PDH_FMT_DOUBLE and does not
# go through that step. If the runner's ProcessorCount differs from the real
# logical count, the two sides differ by a fixed ratio -- which is exactly the
# unexplained ~10x seen on the 4-core CI runner (the 12-core host agrees to
# 0.000). Emit the numbers so one CI run decides it.
# These are emitted on EVERY line (obj.get tolerates absence elsewhere).
$PC_ENV = [System.Environment]::ProcessorCount
try {
    $PC_WMI = [int]((Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors)
} catch {
    $PC_WMI = -1
}

# PerformanceCounter ctor is (categoryName, counterName, instanceName).
$counters = @{}
$all = New-Object System.Collections.Generic.List[object]
$c = New-Object System.Diagnostics.PerformanceCounter('Processor', '% Processor Time', '_Total', $true)
$all.Add(@('_total', $c))
for ($i = 0; $i -lt $Ncpu; $i++) {
    $cc = New-Object System.Diagnostics.PerformanceCounter('Processor', '% Processor Time', "$i", $true)
    $all.Add(@("$i", $cc))
}

# Prime: the first NextValue() only establishes the baseline.
foreach ($p in $all) { $p[1].NextValue() | Out-Null }
Start-Sleep -Milliseconds $IntervalMs

$lines = New-Object System.Collections.Generic.List[string]
$sw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sw.Elapsed.TotalSeconds -lt $DurationSec) {
    Start-Sleep -Milliseconds $IntervalMs
    # Stamp at the moment of read, after the sleep.
    $nowMs = [int64]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    $parts = @()
    foreach ($p in $all) {
        $v = [double]$p[1].NextValue()
        if ([double]::IsNaN($v)) { $v = 0.0 }
        $parts += ('"{0}":{1}' -f $p[0], [math]::Round($v, 4))
    }
    $parts += ('"_pc":{0}' -f $PC_ENV)
    $parts += ('"_pdl":{0}' -f $Ncpu)
    $parts += ('"_pcw":{0}' -f $PC_WMI)
    $lines.Add(('{0}{{{1}}}' -f $nowMs, ($parts -join ',')))
}

[System.IO.File]::WriteAllLines($Out, $lines)
Write-Output ('WROTE=' + $Out)
Write-Output ('SAMPLES=' + $lines.Count)
Write-Output ('NCPU=' + $Ncpu)
Write-Output ('PROCCOUNT env=' + $PC_ENV + ' pdh_instances=' + $Ncpu +
    ' wmi=' + $PC_WMI)
