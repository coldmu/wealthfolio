# UPSTREAM.md — Wealthfolio Fork Origin & Update Policy

## Fork identity

- **Upstream:** <https://github.com/wealthfolio/wealthfolio.git>
- **Private fork (origin):** <https://github.com/coldmu/wealthfolio.git>
- **Pinned release:** `v3.7.0` (latest upstream release tag at fork time; no v3.8/tag exists upstream)
- **Working branch:** `namu-spike` (pushed; fork-internal PR: coldmu/wealthfolio#1)
- **Local checkout:** `c:\Users\SDS\proj\rebal\wealthfolio-namu`

`origin` was repointed to the private fork on first push (2026-09-07);
`upstream` keeps pointing at the public repository for future merges:

```bash
git remote -v            # origin = private fork, upstream = wealthfolio/wealthfolio
```

## Recording the upstream version

Record the exact upstream commit this branch is based on before/after any
upstream merge, and again before shipping a release:

```bash
git describe --tags --always upstream/main
git describe --tags --always HEAD
```

Expected at fork time: `v3.7.0` on `namu-spike` (checked out from tag `v3.7.0`).

## License

- Upstream is distributed under **AGPL-3.0**.
- The `LICENSE` file and copyright notices must be preserved in every
  distribution of this fork.
- If this fork is offered over a network, comply with AGPL-3.0 obligations
  (offer the corresponding source). Not legal advice — review obligations
  before any public deployment.

## Update policy

- **Merge before custom change:** pull upstream into this branch, resolve
  conflicts, and record the new `git describe` before re-applying Namu-specific
  changes. The custom code lives in a small number of Namu modules so merges
  stay contained.
- **Do not** rewrite pushed history.
- **Do not** change upstream's `cash_flow_only` rebalancing engine in this
  phase (see spec: sell/hybrid stays disabled in the UI).

## Adaptations vs. the spike plan (documented deviations)

1. `crates/connect/src/brokers/` in the plan is actually
   `crates/connect/src/broker/` in v3.7.0 — Namu adapter lives under
   `crates/connect/src/broker/namu/`.
2. `apps/server/src/bin/` did not exist — created for the verification binary.
3. No Docker on the dev machine — unmodified web mode verified by running the
   Axum server from source; the Docker route remains available via
   `compose.namu.yml` (requires Docker + `.env.web`).
4. Latest release is `v3.7.0` (plan said "3.8.x"; no such tag exists upstream).