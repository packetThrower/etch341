# Winget manifests

Templates for etch341's entry in the [Windows Package Manager
community repository](https://github.com/microsoft/winget-pkgs)
(`manifests/p/packetThrower/etch341/`). winget doesn't read this
directory; it reads the manifests merged upstream.

## Every stable release (automatic)

`.github/workflows/after_release.yml` fires when `release.yml`
succeeds on a stable `v*` tag. It syncs the `packetThrower/winget-pkgs`
fork and runs `winget-releaser` (komac), which copies the previous
version's manifests, swaps in the new amd64 `.msi` URL, hash and
ProductCode, and opens the PR. Re-run by hand with:

```
gh workflow run after_release.yml -f tag_name=vX.Y.Z
```

Needs the `WINGET_TOKEN` repo secret: the same PAT the other
packetThrower apps use (Contents + Pull requests read/write and
Workflows read/write on the fork).

## First submission (manual, once)

komac can only *update* a package that already exists upstream, so
the first version goes in as a new-package PR. On Windows:

```powershell
winget install Microsoft.WingetCreate
$v = "0.8.0"
wingetcreate new "https://github.com/packetThrower/etch341/releases/download/v$v/etch341-$v-amd64-windows.msi"
```

Fill the prompts from `packetThrower.etch341.locale.en-US.yaml`
(identifier `packetThrower.etch341`), keep only the x64 installer
(see the installer template for why), and let wingetcreate submit.
