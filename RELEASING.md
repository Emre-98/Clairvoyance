# Releasing a new version

Installed copies of Clairvoyance update themselves from the GitHub Releases of the public,
releases-only repository **Emre-98/Clairvoyance-releases** (no source code there). This source
repository builds them. Publishing a version is one command; GitHub Actions does the building,
signing and publishing.

Until the move is finished, do the one-time **Moving releases to Clairvoyance-releases** steps at
the end of this file first.

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
   - creates the GitHub Release `v1.0.1` in **Emre-98/Clairvoyance-releases**
     (https://github.com/Emre-98/Clairvoyance-releases/releases) with
     `Clairvoyance_1.0.1_x64-setup.exe`, its `.sig` signature and `latest.json`, using the
     `RELEASES_TOKEN` secret,
   - adds a "SHA-256 checksums" table (every file of the release) to the release notes on GitHub
     (the in-app update card shows the notes without it),
   - while this source repository is still public: mirrors the same release here, so copies that
     still check the old address find it (see the move below). Once this repository is private
     that step skips itself.
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
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` of this source repository (Settings > Secrets and
  variables > Actions), which the Release workflow uses; it is never in the code. It stays the
  same through the move to Clairvoyance-releases (never put it into that repository);
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

## The releases token (`RELEASES_TOKEN`)

The workflow runs in this repository but publishes to Emre-98/Clairvoyance-releases, which the
built-in `GITHUB_TOKEN` can't write to. It uses a fine-grained personal access token instead:

1. GitHub > Settings > Developer settings > Personal access tokens > Fine-grained tokens >
   **Generate new token**. Name `Clairvoyance releases`, resource owner `Emre-98`, expiration
   1 year (put a reminder in your calendar), **Only select repositories:
   Emre-98/Clairvoyance-releases**, permissions: **Contents: Read and write** (Metadata: read is
   added automatically). Nothing else.
2. Store it in this source repository (not in Clairvoyance-releases):

   ```powershell
   gh secret set RELEASES_TOKEN -R Emre-98/Clairvoyance   # paste the token when asked
   ```

When it expires, the Release workflow fails at "Build, sign and publish"; make a new one the same
way, set the secret again and re-run the workflow for the tag.

## Moving releases to Clairvoyance-releases (one time, in this order)

Copies installed up to v1.10.0 check
`https://github.com/Emre-98/Clairvoyance/releases/latest/download/latest.json`. That address stops
working when this repository becomes private. From v1.11.0 on, the app checks
`https://github.com/Emre-98/Clairvoyance-releases/releases/latest/download/latest.json`
(`app/tauri.conf.json`, `plugins.updater.endpoints`). The **bridge release** is the first version
with the new address: it is published in **both** repositories, so old copies find it at the old
address, update to it, and from then on check the new one. Do these steps in this order:

1. **Create the releases repository.** GitHub > New repository: owner `Emre-98`, name
   `Clairvoyance-releases`, **Public**, tick **Add a README file** (the workflow creates release
   tags on its `main` branch, so it must have one commit). Replace its README with
   `docs/releases-repo/README.md` from this repository and add `PRIVACY.md` and `LICENSE` from
   this repository next to it. Turn on Issues (PRIVACY.md points questions there); turn off
   Wiki, Projects and Discussions if you like.
2. **Create the token** and the `RELEASES_TOKEN` secret (section above).
3. **Merge the change with the new address into `main`** and wait for CI to be green.
4. **Publish the bridge release** (with the source repository still public):

   ```powershell
   .\scripts\release.ps1 1.11.0 -Notes "Updates now come from github.com/Emre-98/Clairvoyance-releases", "..."
   ```

   When the Release workflow is green, check:
   - https://github.com/Emre-98/Clairvoyance-releases/releases/tag/v1.11.0 has the installer,
     the `.sig`, `latest.json` and the SHA-256 table in its notes;
   - https://github.com/Emre-98/Clairvoyance/releases/tag/v1.11.0 has the same files (the
     mirror);
   - both `.../releases/latest/download/latest.json` addresses (old and new repository) open in
     a browser, say `"version": "1.11.0"`, and their `url` points to
     `github.com/Emre-98/Clairvoyance-releases/...`;
   - on a PC with v1.10.0 (or older): Settings > General & updates > **Check now** offers
     v1.11.0, **Update now** installs it, and after the restart **Check now** says it's up to
     date (that check already used the new address; an error there means the new address is
     wrong).
5. **Wait** so the copies out there update. Running copies check at start-up and every 4 hours,
   so anyone who opens Clairvoyance during the wait gets the bridge. Wait **at least 4 weeks**
   (longer is safer; nothing breaks while you wait). Any further releases during the wait are
   fine: the workflow keeps mirroring them into this repository while it's public.
6. **Make this source repository private**: Settings > General > Danger Zone > Change repository
   visibility > Make private. From then on the mirror step skips itself and nothing else changes
   for you: `scripts/release.ps1` works the same.

After step 6, a copy still on v1.10.0 or older gets an error from "Check now" (its address no
longer exists) and must be updated by hand once: download the installer from
https://github.com/Emre-98/Clairvoyance-releases/releases/latest and run it over the old one
(settings and recordings are kept).

Note: GitHub Actions in a private repository uses the account's included minutes (Windows runners
count double). A release takes about 15 minutes, so a release costs about 30 of them.
