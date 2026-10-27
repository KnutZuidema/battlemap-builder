# AGENTS.md — DUNGEON FORGE

AI battlemap builder for tabletop RPGs: React/Vite client (3000) → Express server (3001) → OpenRouter image
models, plus a Rust `curator` service (3002) for ingestion, benchmarking and dataset curation, and a Python LoRA
training package.

## Navigation

| Need | Read |
| :--- | :--- |
| Client/server feature contracts: generation, region forging, telemetry, canvas, stack, environment | [`docs/features.md`](docs/features.md) |
| Curator curation workflow, storage layout, dataset readiness, runbooks | [`curator/README.md`](curator/README.md) |
| LoRA training package and run policies | [`training/README.md`](training/README.md) |
| Curator deployment and rollout | [`deploy/curator/README.md`](deploy/curator/README.md) |
| Domain vocabulary | [`GLOSSARY.md`](GLOSSARY.md) |
| Decisions and their rationale | [`docs/adr/`](docs/adr/) |
| Issue tracking, domain-doc conventions | [`docs/agents/issue-tracker.md`](docs/agents/issue-tracker.md), [`docs/agents/domain.md`](docs/agents/domain.md) |

## Commands

Run from the repository root.

| Task | Command |
| :--- | :--- |
| Dev server + client | `npm run dev` |
| Focused curator test | `cargo test --manifest-path curator/Cargo.toml <filter>` (or `--test <suite>` for one integration suite) |
| Focused server test | `node --test server/test/<file>.test.js` |
| Curator service | `cargo run --manifest-path curator/Cargo.toml` |
| Curator deploy | `just curator-deploy` (`just curator-publish`, `just curator-rollout <tag>`) |

`.githooks/pre-commit` only lints the client when staged changes touch it; it never compiles or runs a test suite.
`npm install` installs the hooks. Full verification is PR CI (see [Verification](#verification)).

### Curator build cache

Curator compiles share one target dir across worktrees (`~/.omp/targets/curator`, override with
`CURATOR_SHARED_TARGET_DIR`). `.githooks/post-checkout` writes an untracked `.cargo/config.toml` pointing there;
delete that file to opt a checkout out. The shared dir accrues per-crate artifacts, so prune
`debug/deps/{curator-,libcurator-}*` and `debug/deps/*_tests-*` when it grows.

## Invariants

- **No silent fallbacks.** A failing model or provider fails the request: `{ error, code, debug }` with
  400/502/503 and a visible banner. Never substitute procedural imagery, switch models, or return the unedited
  source as a success.
- **Prompts live in `server/services/prompts.js`.** Directives change there only; the client renders the telemetry
  the server sends and marks requests that never reached a model as "not captured".
- **Model catalog entries are verified facts.** `server/services/modelCatalog.js` records each model's provider and
  `vision` capability. `DEFAULT_MODEL_ID` must be vision-capable; region edits refuse anything else with
  `MODEL_NOT_VISION_CAPABLE` (400) before an upstream call.
- **Descriptions are perspective-neutral physical prose** (ADR-0002): no camera, art-style or grid directives, and
  the `Trigger Phrase` is applied exactly once, at caption generation — see `curator/README.md` §8.
- **Backend keys only.** The server reads `OPENROUTER_API_KEY` from `server/.env` or the environment; client-side
  key modals and `localStorage` overrides stay out of the codebase.

## Verification

PR CI is the authoritative full check; run it rather than rebuilding everything locally.

| Workflow | Covers |
| :--- | :--- |
| [`.github/workflows/ci.yml`](.github/workflows/ci.yml) | Client lint + production build, full server test suite |
| [`.github/workflows/curator-ci.yml`](.github/workflows/curator-ci.yml) | Curator test suite + clippy (when sources change), container build |

Locally, run only the narrow check for what you touched. Do **not** run `npm run build`, `npm test`,
`cargo test`, `cargo clippy`, `cargo run`, a Docker build, or any other whole-app compile/test unless the user
asks or you are reproducing a CI failure:

- Rust: `cargo test --manifest-path curator/Cargo.toml <filter>`, or `--test <suite>` for a single integration
  suite.
- Server: `node --test server/test/<file>.test.js`.
- Client: lint only (`npm run lint`, no compile); exercise the change in the dev server/browser.

Behavior still needs proof; tests alone are not enough:

- Live-API work: exercise both generation and region forging, and prove `compositeRegion` changes 0 pixels outside
  the selection box.
- Canvas work: drive it in a real browser — right-drag selection, handle resize, and no uncaught Konva draw errors.
