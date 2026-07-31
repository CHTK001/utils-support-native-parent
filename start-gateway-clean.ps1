$starterDir = "D:\ch\project\utils-support-parent-starter\utils-support-gateway-parent\utils-support-remote-starter"
$argsPath = "$starterDir\java.args"
$args = (Get-Content $argsPath -Raw) -split "`n"

$cpEntries = @()
foreach ($line in $args) {
    $trimmed = $line.Trim()
    if (-not $trimmed) { continue }
    if ($trimmed -eq "-cp") { continue }
    $tokens = $trimmed -split ';'
    foreach ($t in $tokens) {
        $entry = $t.Trim()
        if (-not $entry) { continue }
        if ($entry -match 'netty-(all|buffer|codec|common|handler|resolver|transport|core|http|starter)-5\.0\.0\.Alpha2') { continue }
        if ($entry -match 'netty-(all|buffer|codec|common|handler|resolver|transport|core|http|starter)-4\.2\.9\.Final') { continue }
        $cpEntries += $entry
    }
}

$cp = ($cpEntries -join ';')
Write-Host "Cleaned classpath length: $($cpEntries.Count)"

$java = "C:\Program Files\Amazon Corretto\jdk25.0.3_9\bin\java.exe"
$logFile = "D:\ch\project\gateway-clean.log"

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $java
$psi.Arguments = "-Dfile.encoding=UTF-8 -cp `"$cp`" com.chua.remote.support.gateway.GatewayStandalone"
$psi.WorkingDirectory = $starterDir
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($psi)
Start-Sleep -Seconds 2
if (-not $proc.HasExited) {
    $proc.BeginOutputReadLine()
    $proc.BeginErrorReadLine()
    Write-Host "Gateway started PID=$($proc.Id)"
} else {
    Write-Host "Gateway exited immediately"
}
