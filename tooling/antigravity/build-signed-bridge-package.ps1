param(
  [Parameter(Mandatory = $true)]
  [string]$SigningKeyFile,
  [string]$Target = "x86_64-pc-windows-msvc",
  [string]$PackageVersion = "0.1.0"
)

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\.."))
$package = Join-Path $repo "integrations\antigravity\plugin\beta-package"
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo "target" }
$bridge = Join-Path $targetRoot "$Target\release\relintor-antigravity-bridge.exe"

if (-not (Test-Path -LiteralPath $SigningKeyFile -PathType Leaf)) {
  throw "The signing key must be supplied from a protected external location."
}

$cargoArgs = @(
  "+1.96.0-$Target", "build", "--release", "-p", "relintor-antigravity",
  "--bin", "relintor-antigravity-bridge", "--target", $Target, "--locked"
)
cargo @cargoArgs
if (-not (Test-Path -LiteralPath $bridge -PathType Leaf)) {
  throw "The bridge binary was not produced for the requested target."
}

New-Item -ItemType Directory -Force -Path $package | Out-Null
Get-ChildItem -LiteralPath $package -Force | Remove-Item -Recurse -Force
Copy-Item -LiteralPath $bridge -Destination (Join-Path $package "relintor-antigravity-bridge.exe")

@'
{
  "$schema": "https://antigravity.google/schemas/v1/plugin.json",
  "name": "relintor-authority",
  "description": "Relintor signed execution authority bridge"
}
'@ | Set-Content -LiteralPath (Join-Path $package "plugin.json") -Encoding utf8NoBOM

@'
{
  "relintor-authority": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "\"%RELINTOR_ANTIGRAVITY_BRIDGE_PATH%\"",
            "timeout": 30
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "\"%RELINTOR_ANTIGRAVITY_BRIDGE_PATH%\"",
            "timeout": 30
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"%RELINTOR_ANTIGRAVITY_BRIDGE_PATH%\"",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
'@ | Set-Content -LiteralPath (Join-Path $package "hooks.json") -Encoding utf8NoBOM

$hash = (Get-FileHash -LiteralPath (Join-Path $package "relintor-antigravity-bridge.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
@"
{
  "schema_version": 1,
  "adapter_version": "0.1.0",
  "expected_executable": "relintor-antigravity-bridge.exe",
  "expected_sha256": "$hash",
  "supported_antigravity_versions": ["1."],
  "hook_protocol": "relintor-antigravity-hooks-v1",
  "headless_protocol": "agy --print <authorized-task> --add-dir <workspace> --mode accept-edits --output-format stream-json --print-timeout 15m",
  "signing_status": "verified"
}
"@ | Set-Content -LiteralPath (Join-Path $package "bridge-manifest.json") -Encoding utf8NoBOM

cargo +1.96.0-$Target run -p relintor-antigravity --bin sign_bridge_package --locked -- --package-dir $package --key-file $SigningKeyFile --version $PackageVersion
cargo +1.96.0-$Target run -p relintor-antigravity --bin sign_bridge_package --locked -- verify --package-dir $package --key-file $SigningKeyFile
Write-Host "Signed bridge package prepared at $package"
Write-Host "The private signing key was read only from the external path and was not copied."
