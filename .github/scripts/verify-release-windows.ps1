$ErrorActionPreference = "Stop"
$version = ($env:PLAN | ConvertFrom-Json).releases[0].app_version
$installDir = Join-Path $env:RUNNER_TEMP "forager-release"
$configDir = Join-Path $env:RUNNER_TEMP "forager-config"
$stateDir = Join-Path $env:RUNNER_TEMP "forager-state"
New-Item -ItemType Directory -Force $installDir, $configDir, $stateDir | Out-Null
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
$config = @'
[providers.xai]
keys = ["release-gate"]
[providers.openai_compatible]
url = "https://example.invalid/v1"
keys = ["release-gate"]
model = "release-gate"
[providers.tavily]
keys = ["release-gate"]
[providers.firecrawl]
keys = ["release-gate"]
[providers.jina]
keys = ["release-gate"]
[providers.context7]
keys = ["release-gate"]
[providers.exa]
keys = ["release-gate"]
[providers.anysearch]
keys = ["release-gate"]
'@
Set-Content -Encoding utf8 (Join-Path $configDir "config.toml") $config
$env:FORAGER_CONFIG_DIR = $configDir
$env:XDG_STATE_HOME = $stateDir
$env:HOME = Join-Path $env:RUNNER_TEMP "forager-home"
$doctorJson = forager doctor --timeout 1
$doctorExitCode = $LASTEXITCODE
$doctor = $doctorJson | ConvertFrom-Json
if ($doctorExitCode -ne 4 -or $doctor.ok -or $doctor.mode -ne "shallow") {
  throw "doctor did not report the expected unreachable configuration"
}
$configured = @($doctor.providers | Where-Object configured)
if ($configured.Count -ne 8) {
  throw "doctor did not load all migrated provider credentials"
}
exit 0
