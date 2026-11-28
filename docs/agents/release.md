# Merging and releasing

## Merging to main

- A branch reaches `main` through a pull request. Once every check the pull request runs is green, merge with `gh pr merge <N> --rebase --delete-branch`. Rebase keeps every Conventional Commit, and release-plz builds the changelog from them. The `plan` job of `release.yml` also runs on pull requests but is not a merge condition.
- A branch that changes the product skill's routing or structure carries the fresh-agent trial that [`product-skill.md`](product-skill.md) requires before it merges.
- A GitHub rebase merge rewrites commit hashes. After merging, confirm that the local and remote trees match, then run `git pull --rebase`; local commits with the same content are skipped.

## Releasing

release-plz generates the version number and the changelog; `CHANGELOG.md` is not edited by hand.

Before triggering a release, run the real-browser tests that are ignored by default, as [`browser-routes.md`](browser-routes.md) describes, on this machine. Continue only when all of them pass; CI does not run them.

`release-plz.yml` is triggered only by hand, twice per release:

1. Run `gh workflow run release-plz.yml --ref main` and wait for the run to finish. It opens a `chore: release vX.Y.Z` pull request; check its version number and changelog.
2. When that release pull request's checks pass, run `gh pr merge <N> --squash --delete-branch`. Release pull requests are always squash-merged.
3. Run `git pull --rebase`, then trigger `release-plz.yml` again. This run tags the release, creates a draft Release and triggers `release.yml` (the cargo-dist build). Wait for the build with `gh run list --workflow release.yml --limit 1` and `gh run watch <ID> --exit-status`.

The release is done when `gh release view vX.Y.Z --json isDraft` shows `isDraft` as `false`: `release.yml` publishes the Release only after the artifacts for every platform pass their checks.

## Completion

After a release, finish these steps in order. The task is complete only when all of them are done.

1. **Update the local CLI.** The local forager was installed into `~/.cargo/bin` by the cargo-dist installer, whose receipt is `~/.config/forager/forager-receipt.json`. Update it with the same installer:

   ```sh
   curl --proto '=https' --tlsv1.2 -LsSf \
     https://github.com/jfmoe/forager/releases/download/vX.Y.Z/forager-installer.sh | sh
   ```

   Then confirm that `forager --version` shows the new version. `cargo install --path .` cannot replace the installer here: it refuses to overwrite a binary of the same name that cargo does not manage.
2. **Update the skill.** Run `npx skills add jfmoe/forager -g -s forager -a '*' -y`. Agents that do not support a global install report a failure; that is expected.
3. **Update the Managed Browser.** Run `forager browser install`. When the new version pins the same browser version, it downloads nothing; otherwise it downloads, verifies and installs the new one. It also removes older versions of the browser.
4. **Run the Live Checks, with the user present.** Run `forager doctor --live --tier free,metered,attended` with the real configuration. Selecting every tier runs every declared check, the expensive and manual-only ones included, and selecting `attended` is the user's consent to the checks that open pages of a logged-in site or put the egress address at risk; the user stays at the machine to handle a login or verification page as [`browser-routes.md`](browser-routes.md) describes. Run it in a shell where `CI` is unset or `false`: a CI environment counts as unattended and refuses the attended tier with exit 2 before any request. Give the tool running the command a timeout longer than the run's time limit, whose default `forager doctor --help` shows. Report the status of every check.

   [`live-checks.md`](live-checks.md) defines each status, cause and exit code; what to do about each result:

   - **`not_started`, cause `deadline`**: rerun with a larger `--timeout`; the run counts it as failed.
   - **`not_started`, cause `site_stop`**: do not rerun; follow the stop rules in [`browser-routes.md`](browser-routes.md). Another run only triggers the block again.
   - **`not_started`, cause `no_input`**: investigate the check it depends on; a larger time limit does not help.
   - **`unconfigured`**: its `message` names the prerequisite this machine lacks and its fix. The completion record lists every unconfigured check with its reason. For a prerequisite this machine could meet, such as installing the browser or a login the user does in person, either meet it and rerun that provider's checks with `forager doctor --live --provider <provider>`, or open an issue to track it. Keep the check unconfigured, with its reason in the record, only when this machine genuinely cannot meet the prerequisite, such as a key for a paid service it has no account for.
5. **Clean build artifacts.** Run `cargo clean -p forager`. forager's own test binaries and incremental compilation directories keep accumulating; this command removes only forager's own artifacts and keeps the compiled third-party dependencies.
