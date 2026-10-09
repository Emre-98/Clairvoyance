# Releasing a new version

Installed copies of Clairvoyance update themselves from the GitHub Releases of the public,
releases-only repository **Emre-98/Clairvoyance-releases** (no source code there; this source
repository is private). Publishing a version is one command; GitHub Actions does the building,
signing and publishing.

Before the first release there: the one-time steps in "Moving releases to Clairvoyance-releases"
at the end of this file.

## Every release

1. Make sure everything you want is committed and pushed on `main`, and CI is green.
2. In PowerShell, in the project folder:

   ```powershell
   .\scripts\release.ps1 1.0.1 -Notes "What changed", "Another change"
   ```

   (Leave out `-Notes` to be asked for them one line at a time. `-DryRun` shows what would
   change without writing anything.)

   The script:
   - raises the version in `Cargo.toml`, `Cargo.lock`, `app/tauri.conf.json` and `ui/package.json`,
   - adds a `## [1.0.1]` section to `CHANGELOG.md` (these become the release notes),
   - commits, creates the tag `v1.0.1` and pushes both.
3. Pushing the tag starts the **Release** workflow
   (https://github.com/Emre-98/Clairvoyance/actions). In about 10–15 minutes it:
   - runs the tests, builds the app and the installer on Windows,
   - seals the executable against tampering (see "The tamper seal" below),
   - signs the update with the private key stored in the repository secrets,
   - creates the GitHub Release `v1.0.1` in **Emre-98/Clairvoyance-releases** with
     `Clairvoyance_1.0.1_x64-setup.exe`, its `.sig` signature and `latest.json` (using the
     `RELEASES_TOKEN` secret), and adds a "SHA-256 checksums" table to its notes,
   - while this source repository is public, mirrors the release here too (the bridge, below).
4. Done. Installed copies check `latest.json` at start-up and every 4 hours (never during a
   game) and show "Update available: v1.0.1" with the notes and an **Update now** button.
   To see it right away: Settings > General & updates > Check now.

   The release is first created as a **draft**, the sealed executable is checked, and only then
   is the release published (installed copies never see a draft).

If the workflow fails, open it in the Actions tab, fix the problem, push the fix to `main`, and
re-run it for the same tag with **Run workflow** (Release > Run workflow > tag `v1.0.1`). If it
failed after creating the draft, delete that draft first (Clairvoyance-releases > Releases > the
draft > Delete).
Never reuse a version number that was already published.

## The signing key (important)

Updates are signed; installed copies only accept updates signed with the matching key. The
public key is in `app/tauri.conf.json` (`plugins.updater.pubkey`). The private key:

- is stored in the GitHub repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (Settings > Secrets and variables > Actions), which the
  Release workflow uses; it is never in the code;
- has a backup copy on your PC in `C:\Users\emre_\Documents\Clairvoyance-signing-key\`
  (`clairvoyance-updater.key` and `password.txt`), outside the project folder.

**Back that folder up somewhere safe (a USB stick or a password manager).** If the key is lost,
installed copies can no longer be updated: everyone would have to download and install a new
version by hand once. Never commit it, share it, or put it in the project folder.

If you ever need to put the key into the secrets again:

```powershell
$k = "$env:USERPROFILE\Documents\Clairvoyance-signing-key"
Get-Content "$k\clairvoyance-updater.key" -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY -R Emre-98/Clairvoyance
Get-Content "$k\password.txt" -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R Emre-98/Clairvoyance
```

## The tamper seal

Release builds refuse to start if their executable was changed after the build (renamed,
rebranded or patched copies): the Release workflow signs the executable with a separate seal key
(`crates/cv-seal`, run by `beforeBundleCommand` in `app/tauri.conf.json`) and the app checks
that signature at start-up (`app/src/integrity.rs`). Dev builds and `scripts/build-windows.sh`
test builds aren't sealed and don't check.

**One-time setup** (before the first release with the seal), in PowerShell in the project folder:

```powershell
$k = "$env:USERPROFILE\Documents\Clairvoyance-signing-key"
cargo run -q -p cv-seal -- keygen "$k\seal.key"      # prints the public key
Get-Content "$k\seal.key" -Raw | gh secret set CV_SEAL_KEY -R Emre-98/Clairvoyance
gh variable set CV_SEAL_PUBKEY -R Emre-98/Clairvoyance --body "<the public key it printed>"
```

`CV_SEAL_KEY` is a secret (never share or commit `seal.key`); `CV_SEAL_PUBKEY` is a plain
repository variable (Settings > Secrets and variables > Actions > Variables). The Release
workflow stops if either is missing.

Unlike the updater key, the seal key can be replaced at any time: each version carries its own
public key, so a new key only affects versions built with it. If `seal.key` is lost or leaked,
run the setup again with a new file name and release a new version.

To check a downloaded or installed executable by hand:
`cargo run -q -p cv-seal -- verify "<path>\Clairvoyance.exe" <public key>`.

## Version numbers

`MAJOR.MINOR.PATCH`: bug fixes raise PATCH (1.0.1), new features raise MINOR (1.1.0).
The updater only offers versions higher than the installed one.

## The releases token (`RELEASES_TOKEN`)

The workflow runs in this repository but publishes to Emre-98/Clairvoyance-releases, which the
built-in `GITHUB_TOKEN` can't write to. It uses a fine-grained personal access token:

1. GitHub > Settings > Developer settings > Personal access tokens > Fine-grained tokens >
   **Generate new token**. Name `Clairvoyance releases`, resource owner `Emre-98`, expiration
   1 year (put a reminder in your calendar), **Only select repositories:
   Emre-98/Clairvoyance-releases**, permissions: **Contents: Read and write**. Nothing else.
2. Store it in this source repository (not in Clairvoyance-releases):

   ```powershell
   gh secret set RELEASES_TOKEN -R Emre-98/Clairvoyance   # paste the token when asked
   ```

When it expires, the Release workflow stops at "Check the seal key is set"; make a new one the
same way, set the secret again and re-run the workflow for the tag.

## Moving releases to Clairvoyance-releases (one time, in this order)

Copies up to v1.13.0 check
`https://github.com/Emre-98/Clairvoyance/releases/latest/download/latest.json`, which only works
while this repository is public. From v1.14.0 on, the app checks
`https://github.com/Emre-98/Clairvoyance-releases/releases/latest/download/latest.json`. The
**bridge release** (1.14.0, the first with the new address) is published in both repositories, so
old copies find it at the old address, update, and from then on use the new one.

1. **Create the releases repository**: GitHub > New repository: owner `Emre-98`, name
   `Clairvoyance-releases`, **Public**, tick **Add a README file** (the workflow creates release
   tags on its `main` branch, so it needs one commit). Put `docs/releases-repo/README.md` from this
   repository in as its README.
2. **The tamper seal key** (section "The tamper seal") and **the releases token** (above).
3. **With this repository public**, release 1.14.0 (`.\scripts\release.ps1 1.14.0 ...`). Check:
   both `.../releases/latest/download/latest.json` addresses (this repository and
   Clairvoyance-releases) say `"version": "1.14.0"` and point at the installer in
   Clairvoyance-releases; a PC on 1.13.0 offers and installs the update; after the restart,
   Settings > General & updates > Check now says it's up to date (that check used the new address).
4. **Wait about a week** (copies check at start-up and every 4 hours, so anyone who opens the app
   moves over). Further releases meanwhile are mirrored here automatically.
5. **Make this repository private** again. The mirror step then skips itself. A copy still on
   1.13.0 or older can't update itself after that: download the installer from
   https://github.com/Emre-98/Clairvoyance-releases/releases/latest once and run it (settings and
   recordings are kept).
