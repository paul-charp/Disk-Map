# Distribution

Release assets, the updater and its signing key. The Release workflow (`.github/workflows/release.yml`) runs on a `v*` tag.

## Release assets

| Asset | Made when |
| --- | --- |
| `diskmap-<tag>-windows-x64.zip` | always |
| `diskmap-<tag>-windows-x64-setup.exe` | always |
| `diskmap-<tag>-windows-x64-setup.exe.sig` | signing key set |
| `latest.json` | signing key set |

A tag with a `-` (e.g. `v0.3.0-rc.1`) is published as a pre-release. GitHub's `releases/latest` skips pre-releases, so the updater never offers them.

## Updater

`tauri-plugin-updater` reads `https://github.com/paul-charp/Disk-Map/releases/latest/download/latest.json`, set in `crates/desktop/tauri.conf.json` under `plugins.updater`. The installer it downloads must carry a signature made with the private key that matches `plugins.updater.pubkey`; `requireSignedVersion` also binds the signature to the version, which blocks downgrades.

- `pubkey` empty: updates are off. The app builds and runs; About shows `Off`. The workflow publishes the zip and the setup without updater files and prints a warning.
- Installed build: checks at startup when `check_updates` is on (Settings, About, Auto check) and from Check now. A newer version shows an `Update x.y.z` chip in the status bar and Install in About, which downloads, verifies, runs the installer in passive mode and restarts.
- Portable build: same check; About shows Release, which opens the latest release page.
- Failures go to the log (Output panel), never to a dialog.

The workflow writes `latest.json`:

```json
{
  "version": "0.2.0",
  "notes": "CHANGELOG section of 0.2.0, or the tag",
  "pub_date": "2026-11-13T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<content of the .sig file>",
      "url": "https://github.com/paul-charp/Disk-Map/releases/download/v0.2.0/diskmap-v0.2.0-windows-x64-setup.exe"
    }
  }
}
```

The version comes from `tauri.conf.json`, so the tag must match it (`docs/branching.md`).

### One-time key setup

Done once by the maintainer on a trusted machine. The private key never goes into the repository.

1. Install the Tauri CLI: `cargo install tauri-cli --version "^2" --locked`.
2. Generate the pair and choose a password:

   ```powershell
   cargo tauri signer generate -w ~/.tauri/diskmap.key
   ```

   This writes `~/.tauri/diskmap.key` (private) and `~/.tauri/diskmap.key.pub` (public). Back up the private key and its password: without them no installed build can be updated again.
3. Paste the content of `diskmap.key.pub` (one base64 line) into `crates/desktop/tauri.conf.json`, `plugins.updater.pubkey`, and commit it on `develop`.
4. On GitHub, Settings, Secrets and variables, Actions, add two repository secrets:
   - `TAURI_SIGNING_PRIVATE_KEY`: the content of `diskmap.key`.
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password.
5. The next tagged release ships the `.sig` and `latest.json`. Builds released before step 3 have no key and cannot update themselves; users install that release once by hand.

Local installer builds use `cargo tauri build --bundles nsis --no-sign`, or set the two variables to sign them.

## Code signing

The exe and the installer are not Authenticode-signed. SmartScreen may show "Windows protected your PC" when a download is first run, until the file gains reputation (More info, Run anyway). The updater signature is separate and does not change this. SignPath, which signs open-source projects for free, is an option for later.
