# Merging and releasing

## Merging to main

- A branch reaches `main` through a pull request. Once every check the pull request runs is green, merge with `gh pr merge <N> --rebase --delete-branch`. Rebase keeps every Conventional Commit, and release-plz builds the changelog from them. The `plan` job of `release.yml` also runs on pull requests but is not a merge condition.
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
3. **Update the Managed Browser.** Run `forager browser install`. When the new version pins the same browser version, it downloads nothing; otherwise it downloads, verifies and installs the new one. The old installation directory is no longer used and is not deleted automatically.
4. **Run the live end-to-end test.** Run `forager smoke --live --opt-in` with the real configuration. `--opt-in` also runs the cases of platform routes that the order leaves out but whose prerequisites this machine meets, so routes enabled by hand are covered at release. Give the tool running the command a timeout longer than the smoke time limit, whose default `forager smoke --help` shows. Report the status of every case.

   A `skipped` case whose message starts with `not started:` has one of three kinds of source (the messages are constants in `src/ops/smoke.rs`):

   - **The time limit ran out** before the case began. Rerun with a larger `--timeout`.
   - **The site blocked the session**: an earlier case on the same site met a login wall, a verification page or a block, so the run stopped all access to that site. Do not rerun; follow the stop rules in [`browser-routes.md`](browser-routes.md). Another run only triggers the block again.
   - **An upstream case produced no input** for this one, such as a search that returned nothing to fetch. Investigate the upstream case; a larger time limit does not help.

   None of these is a provider failure in the case itself.

   The completion record lists every `unconfigured` case with the reason its `message` gives (the order leaves the route out when `--opt-in` is absent, the browser is not installed, the Browser Profile is not logged in, keys are missing, or a non-platform case lacks credentials). `unconfigured` is not a pass and is not let through by default. For a prerequisite this machine could meet, such as installing the browser or a login the user does in person, either meet it and rerun those cases or open an issue to track it. Keep the case unconfigured, with its reason in the record, only when this machine genuinely cannot meet the prerequisite, such as a key for a paid service it has no account for.
5. **Clean build artifacts.** Run `cargo clean -p forager`. forager's own test binaries and incremental compilation directories keep accumulating; this command removes only forager's own artifacts and keeps the compiled third-party dependencies.
