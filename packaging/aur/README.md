# Arch Linux (AUR) packaging

`signalkit-bin` is a binary AUR package that pulls the official GitHub release
artifacts and installs them on Arch Linux. The PKGBUILD lives in this directory
and is mirrored to the AUR git repo at `ssh://aur@aur.archlinux.org/signalkit-bin.git`.

## First-time AUR submission (one-time, manual)

1. Make sure you have an Arch Linux user account on https://aur.archlinux.org/
   and an SSH key registered there (Settings → SSH Public Key).

2. Clone the empty AUR repo into a sibling directory:

   ```bash
   cd /tmp
   git clone ssh://aur@aur.archlinux.org/signalkit-bin.git
   cp /steam/rotko/signal-mcp-server/packaging/aur/signalkit-bin/PKGBUILD signalkit-bin/
   cd signalkit-bin
   ```

3. Compute and fill in the real `sha256sums`:

   ```bash
   updpkgsums
   ```

4. Generate `.SRCINFO` (AUR requires it):

   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```

5. Push:

   ```bash
   git add PKGBUILD .SRCINFO
   git commit -m "Initial import of signalkit-bin v0.1.0"
   git push
   ```

Users can then install with their AUR helper:

```bash
yay -S signalkit-bin
# or: paru -S signalkit-bin
```

## Subsequent releases

When you cut a new GitHub release (e.g. `v0.1.1`):

1. Bump `pkgver` in `signalkit-bin/PKGBUILD`. Reset `pkgrel=1`.
2. Re-run `updpkgsums` to refresh `sha256sums`.
3. `makepkg --printsrcinfo > .SRCINFO`.
4. Commit and push to the AUR repo.

## Why `-bin` and not source build?

Building presage + libsignal-service from source on every install adds ~5–10 min
to install time. The binary release is already AGPL-3.0 (the same license as the
project), so distributing it via AUR is fine.

If a `signalkit` (source) AUR package is ever wanted, it would build via
`cargo build --release` and bundle the Tauri app similarly.
