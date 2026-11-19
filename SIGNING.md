# Code signing VetCord (Azure Trusted Signing)

Unsigned installers trigger Windows SmartScreen ("Windows protected
your PC"). The fix is a signature from a certificate issued to a
**verified identity**. A self-signed certificate does *not* remove the
warning on other people's PCs — it is only useful for testing the
pipeline. VetCord uses **Azure Trusted Signing** (Microsoft's managed
signing service, ~$10/month).

Public releases are only published from trusted-signed builds.

## How it is wired in this repo

- `src-tauri/tauri.conf.json` stays **unsigned** so anyone can build.
- [`signing/Invoke-SignedBuild.ps1`](signing/Invoke-SignedBuild.ps1)
  generates `src-tauri/tauri.signing.generated.conf.json` (git-ignored)
  with a Tauri `signCommand` that calls `signtool` + the Trusted
  Signing dlib. Tauri signs the app binary *before* NSIS packaging and
  the installer afterwards, with SHA-256 + RFC 3161 timestamping
  (`timestamp.acs.microsoft.com`), so signatures stay valid after the
  short-lived signing certificate expires.
- [`.github/workflows/release.yml`](.github/workflows/release.yml)
  runs on `v*` tags: Azure OIDC login → tests → signed build →
  `Get-AuthenticodeSignature` must report `Valid` for both the app and
  the installer → GitHub release. If the Trusted Signing configuration
  is missing, the workflow fails with a clear error instead of
  publishing an unsigned build.
- No certificate files or passwords exist anywhere: Trusted Signing
  keys stay in Azure; CI authenticates with OIDC federation.

## One-time Azure setup (repo owner)

1. Create an Azure subscription (if needed) and a **Trusted Signing
   account** in a supported region (e.g. East US →
   endpoint `https://eus.codesigning.azure.net/`).
2. In the account, complete **Identity validation** for
   VetTV-Studios (organization) or yourself (individual, where
   available). This is the step that makes SmartScreen trust the
   publisher name.
3. Create a **Certificate profile** (Public Trust) on the validated
   identity. Note the account name and profile name.
4. Create an **App registration** (or managed identity) for GitHub:
   add a **federated credential** for this repo's tags
   (`repo:VetTV-Studios/VetCord:ref:refs/tags/v*`), and assign it the
   **Trusted Signing Certificate Profile Signer** role on the Trusted
   Signing account.
5. In this GitHub repo, set:
   - Secrets: `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`,
     `AZURE_SUBSCRIPTION_ID`
   - Variables: `AZURE_TS_ENDPOINT`, `AZURE_TS_ACCOUNT`,
     `AZURE_TS_PROFILE`
6. Release: `git tag v0.4.0 && git push origin v0.4.0`.

## Local signed build (after the Azure setup)

```powershell
az login
nuget install Microsoft.Trusted.Signing.Client -Version 1.0.86 -OutputDirectory $env:TEMP\tsclient -ExcludeVersion
$dlib = Get-ChildItem $env:TEMP\tsclient -Recurse -Filter Azure.CodeSigning.Dlib.dll |
  Where-Object FullName -match x64 | Select-Object -First 1 -ExpandProperty FullName
./signing/Invoke-SignedBuild.ps1 -Endpoint https://eus.codesigning.azure.net/ `
  -Account <account> -Profile <profile> -DlibPath $dlib -RunBuild
Get-AuthenticodeSignature src-tauri\target\release\bundle\nsis\VetCord_*-setup.exe
```

## Testing the mechanics without Azure (self-signed)

This proves signtool/pipeline behaviour only — other PCs will still
warn, because the root is untrusted:

```powershell
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=VetCord Test (self-signed)" `
  -CertStoreLocation Cert:\CurrentUser\My -KeyUsage DigitalSignature
& "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe" sign `
  /sha1 $cert.Thumbprint /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 <file.exe>
Get-AuthenticodeSignature <file.exe>   # Status shows the untrusted-root warning, as expected
```

Remove the test certificate afterwards:
`Remove-Item Cert:\CurrentUser\My\<thumbprint>`.
