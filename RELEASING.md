# Releasing a new version

Installed copies of Clairvoyance update themselves from this repository's GitHub Releases.
Publishing a version is one command; GitHub Actions does the building and signing.

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
   - signs the update with the private key stored in the repository secrets,
   - creates the GitHub Release `v1.0.1` with `Clairvoyance_1.0.1_x64-setup.exe`,
     its `.sig` signature and `latest.json`.
4. Done. Installed copies check `latest.json` at start-up and every 4 hours (never during a
   game) and show "Update available: v1.0.1" with the notes and an **Update now** button.
   To see it right away: Settings > General & updates > Check now.

If the workflow fails, open it in the Actions tab, fix the problem, push the fix to `main`, and
re-run it for the same tag with **Run workflow** (Release > Run workflow > tag `v1.0.1`).
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

## Version numbers

`MAJOR.MINOR.PATCH`: bug fixes raise PATCH (1.0.1), new features raise MINOR (1.1.0).
The updater only offers versions higher than the installed one.
