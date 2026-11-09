# Windows Package Manager (winget)

Blitz is not yet in the public `winget` catalog until a manifest PR merges. Use this folder as the source of truth when submitting.

## Submit

1. Update `h3ma209.Blitz.yaml`: `PackageVersion`, `InstallerUrl`, `InstallerSha256` (from GitHub release).
2. Fork [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs).
3. Add manifests under `manifests/h/h3ma209/Blitz/<version>/` (singleton or multi-file set per winget schema).
4. Open PR; after merge users can run:

```powershell
winget install --id h3ma209.Blitz -e
```

## Local validation

```powershell
winget validate .\packaging\winget\h3ma209.Blitz.yaml
```

## SmartScreen

/winget installs from trusted catalog reduce SmartScreen friction versus raw browser downloads. Users should still prefer the PowerShell installer or `Unblock-File` only when installing manually from Releases.
