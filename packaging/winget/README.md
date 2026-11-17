# Windows Package Manager (winget)

Blitz is not yet in the public `winget` catalog until a manifest PR merges. Use this folder as the source of truth when submitting.

## Submit

1. Copy `packaging/winget/<version>/` (three YAML files). Bump version, `InstallerUrl`, `InstallerSha256` from the GitHub release.
2. Fork [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs).
3. Add those files under `manifests/h/h3ma209/Blitz/<version>/` — **multi-file only** (`version` + `locale` + `installer`), `ManifestVersion: 1.12.0`, schema headers included. Do not use singleton manifests.
4. On the PR, comment once (personal account): `@microsoft-github-policy-service agree` unless your employer owns the contribution (then use `agree company="..."` per the bot message).
5. Open or update PR; after merge users can run:

```powershell
winget install --id h3ma209.Blitz -e
```

## Local validation

```powershell
winget validate --manifest .\packaging\winget\0.2.6\
```

## SmartScreen

/winget installs from trusted catalog reduce SmartScreen friction versus raw browser downloads. Users should still prefer the PowerShell installer or `Unblock-File` only when installing manually from Releases.
