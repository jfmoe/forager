$ErrorActionPreference = "Stop"
$version = ($env:PLAN | ConvertFrom-Json).releases[0].app_version
$installDir = Join-Path $env:RUNNER_TEMP "forager-release"
$configDir = Join-Path $env:RUNNER_TEMP "forager-config"
$stateDir = Join-Path $env:RUNNER_TEMP "forager-state"
New-Item -ItemType Directory -Force $installDir, $stateDir | Out-Null
Expand-Archive "artifacts/forager-$($env:TARGET).$($env:ARCHIVE)" $installDir
$binary = Get-ChildItem $installDir -Recurse -File -Filter forager.exe |
  Select-Object -First 1
if ($null -eq $binary) {
  throw "forager.exe is missing"
}
$stream = [System.IO.File]::OpenRead($binary.FullName)
try {
  $reader = [System.Reflection.PortableExecutable.PEReader]::new($stream)
  $machine = $reader.PEHeaders.CoffHeader.Machine
  if ($machine.ToString() -ne $env:EXPECTED_MACHINE) {
    throw "unexpected PE Machine: $machine"
  }
} finally {
  $stream.Dispose()
}
$env:PATH = "$($binary.DirectoryName);$env:PATH"
if ((Get-Command forager).Source -ne $binary.FullName) {
  throw "PATH does not resolve the release binary"
}
if ((forager --version) -ne "forager $version") {
  throw "release binary version mismatch"
}
$env:FORAGER_CONFIG_DIR = $configDir
$env:XDG_STATE_HOME = $stateDir
$env:HOME = Join-Path $env:RUNNER_TEMP "forager-home"
# `config set` gives the file the owner-only ACL the report checks; a file written here would
# inherit the runner directory's ACL.
'["release-gate"]' | forager config set providers.exa.keys - | Out-Null
if ($LASTEXITCODE -ne 0) {
  throw "config set failed with exit code $LASTEXITCODE"
}
$doctorJson = forager doctor --offline
$doctorExitCode = $LASTEXITCODE
$doctor = $doctorJson | ConvertFrom-Json
$reachability = @($doctor.providers | Where-Object { $null -ne $_.PSObject.Properties["reachable"] })
if ($doctorExitCode -ne 0 -or -not $doctor.ok -or $doctor.mode -ne "offline" -or $reachability.Count -ne 0) {
  throw "doctor did not report a passing offline check"
}
if ($doctor.config.providers.exa.keys.source -ne "file") {
  throw "doctor did not load the configuration file"
}
if (@($doctor.permission_warnings).Count -ne 0) {
  throw "doctor reported permission warnings: $($doctor.permission_warnings -join '; ')"
}
exit 0
