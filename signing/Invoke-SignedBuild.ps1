<#!
.SYNOPSIS
  Generates the Tauri signing config for Azure Trusted Signing and (optionally) runs the build.
.DESCRIPTION
  Tauri's default config stays unsigned so contributors can build without
  an Azure account. Release builds merge a generated config containing a
  signCommand that calls signtool with the Azure Trusted Signing dlib:
  Tauri then signs the app binary before NSIS packaging and the installer
  afterwards. Authentication is handled by the Azure SDK credential chain
  (azure/login OIDC in CI, or `az login` locally) - no secrets in this repo.
.EXAMPLE
  ./signing/Invoke-SignedBuild.ps1 -Endpoint https://eus.codesigning.azure.net/ -Account myaccount -Profile myprofile -DlibPath C:\ts\Azure.CodeSigning.Dlib.dll -RunBuild
#>
param(
  [Parameter(Mandatory = $true)][string]$Endpoint,
  [Parameter(Mandatory = $true)][string]$Account,
  [Parameter(Mandatory = $true)][string]$Profile,
  [Parameter(Mandatory = $true)][string]$DlibPath,
  [string]$SignToolPath = "signtool.exe",
  [string]$TimestampUrl = "http://timestamp.acs.microsoft.com",
  [switch]$RunBuild
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path $PSScriptRoot -Parent
$srcTauri = Join-Path $repoRoot "src-tauri"

if (-not (Test-Path $DlibPath)) {
  throw "Trusted Signing dlib not found at '$DlibPath'. Install the Microsoft.Trusted.Signing.Client NuGet package (see SIGNING.md)."
}

$metadataPath = Join-Path $srcTauri "signing-metadata.generated.json"
@{
  Endpoint               = $Endpoint
  CodeSigningAccountName = $Account
  CertificateProfileName = $Profile
} | ConvertTo-Json | Set-Content -Path $metadataPath

$configPath = Join-Path $srcTauri "tauri.signing.generated.conf.json"
$config = @{
  bundle = @{
    windows = @{
      digestAlgorithm = "sha256"
      timestampUrl    = $TimestampUrl
      tsp             = $true
      signCommand     = @{
        cmd  = $SignToolPath
        args = @(
          "sign", "/v",
          "/fd", "SHA256",
          "/tr", $TimestampUrl, "/td", "SHA256",
          "/dlib", $DlibPath,
          "/dmdf", $metadataPath,
          "%1"
        )
      }
    }
  }
}
$config | ConvertTo-Json -Depth 6 | Set-Content -Path $configPath
Write-Output "Wrote $configPath"
Write-Output "Wrote $metadataPath"

if ($RunBuild) {
  Push-Location $srcTauri
  try {
    npx --yes @tauri-apps/cli@2 build --config tauri.signing.generated.conf.json
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
  } finally {
    Pop-Location
  }
}
