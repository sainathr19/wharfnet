# Releasing

wharfnet is published to [crates.io](https://crates.io/crates/wharfnet). Releases
are driven by a **git tag**; pushing `vX.Y.Z` runs the
[`Release`](.github/workflows/release.yml) workflow, which verifies, publishes to
crates.io, and cuts a GitHub Release.

## Cutting a release

Versioning follows [SemVer](https://semver.org). All work lands on `main` through
a PR first (see the branch rules below), then the release is tagged on `main`.

1. **Bump the version** — on a branch off `dev`, edit `version` in `Cargo.toml`
   and move the `## [Unreleased]` section of `CHANGELOG.md` under a new
   `## [X.Y.Z]` heading.
2. **PR it into `main`** and merge once CI is green.
3. **Tag `main`:**
   ```sh
   git checkout main && git pull
   git tag vX.Y.Z          # must equal the Cargo.toml version
   git push origin vX.Y.Z
   ```
4. The `Release` workflow runs automatically:
   - **Verify** — tag matches `Cargo.toml`, `cargo test`, `cargo audit`, and
     `cargo publish --dry-run` all pass.
   - **Publish** — `cargo publish` to crates.io via Trusted Publishing (OIDC).
   - **GitHub Release** — created from the tag with generated notes.

The tag→version check means a mismatched tag fails fast, before anything is
published.

## Security model

Publishing is the highest-risk automation in the repo, so it is locked down:

- **Trusted Publishing (OIDC), no stored token.** The publish step exchanges a
  short-lived GitHub OIDC token for a crates.io token via
  [`rust-lang/crates-io-auth-action`](https://github.com/rust-lang/crates-io-auth-action).
  There is no long-lived `CARGO_REGISTRY_TOKEN` secret to leak or rotate.
- **Protected `release` environment.** The publish job runs in the `release`
  GitHub Environment — add required reviewers there to gate publishing behind a
  human approval.
- **Least-privilege permissions.** The workflow defaults to `contents: read`;
  only the publish job gets `id-token: write` and only the release job gets
  `contents: write`.
- **Dependency auditing.** `cargo audit` (RustSec) gates every release and runs
  weekly via [`audit.yml`](.github/workflows/audit.yml).
- **Reproducible deps.** Everything uses `--locked`, so the published artifact is
  built from the exact, reviewed `Cargo.lock`.
- **2FA** is enabled on the crates.io account that owns the crate.

### One-time setup

Trusted Publishing must be registered on crates.io before the workflow can
publish. On the crate's page → **Settings → Trusted Publishing → Add**, enter:

- Repository owner / name: `sainathr19` / `wharfnet`
- Workflow filename: `release.yml`
- Environment: `release`

**First publish / claiming the name:** if `wharfnet` has never been published,
run one manual publish to create the crate and claim the name, then rely on the
workflow for every release after:

```sh
cargo login          # paste a scoped, publish-only crates.io token
cargo publish --locked
# then delete the local token: cargo logout
```

### Hardening (optional)

- **Pin actions to commit SHAs** instead of tags (e.g.
  `actions/checkout@<sha>`) so a compromised action tag can't inject code.
- **Enable required reviewers** on the `release` environment.

## Branch rules

`main` is protected: no direct commits, PRs only, and `rustfmt` / `clippy` /
`cargo test` must pass. Do work on `dev`, PR to `main`, and merge — see the
project's git workflow.
