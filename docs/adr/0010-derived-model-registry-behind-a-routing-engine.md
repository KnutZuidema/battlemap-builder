---
status: accepted
---

# Derived Model Registry behind a provider-routing ModelEngine

Model identity becomes a `ModelRegistry` computed at read time — the OpenRouter spec constants plus a scan of `data/adapters/*/adapter.json` — read through `GET /api/models` and shared by the Benchmark Drawer, the benchmark suite and cost stats, replacing `ALL_BENCHMARK_MODELS`, `BenchmarkModelSpec::find` and the model checklist hardcoded in the drawer markup. One registry entry is one (`LoRA Adapter`, inference engine) pair, its descriptor is a superset of today's `BenchmarkModelSpec` plus a provider kind (`openrouter` or `wavespeed`), and a `RoutingModelEngine` dispatches by provider kind either to the existing OpenRouter adapter or to a WaveSpeed adapter that passes `loras: [{path, scale}]`.

## Decision

- **The registry is derived, never persisted.** Publishing an adapter makes its entries appear and retiring it hides them, so the filesystem stays the only adapter catalog and there is no second copy to drift.
- **Entry ids carry the engine.** `{run_id}-s{step}` for the text-to-image engine and `{run_id}-s{step}-edit` for the edit engine, because the two have different prices ($0.015 vs $0.021) and different reference-conditioning capability; one id spanning both would make `by_model` cost stats ambiguous and force a per-row engine picker in the Benchmark Drawer.
- **The descriptor keeps today's fields and adds what adapters need.** Provider kind, API kind, `has_vision`, `supports_reference_image`, plus pricing that is either token-based (`input_per_million`, `output_per_million`, `image_output`) or flat per image.
- **Capability flags are gates, not labels.** The reference-conditioning toggle in the Benchmark Drawer is refused for an entry that cannot accept an image, so an impossible combination cannot be silently ignored and misrecorded as a result.
- **Retired entries stay resolvable for history, never selectable.**
- **The static list dies with the registry.** `ALL_BENCHMARK_MODELS` and `BenchmarkModelSpec::find` stop being the source of model identity, and the drawer fetches its checklist instead of shipping it in markup.

## Considered options

- **A persisted registry document that adapters are registered into** — the filesystem already holds the adapters and a registration write can disagree with it.
- **Encoding the adapter inside the model-id string** (`wavespeed-ai/flux-3-klein#<adapter>`) — provider, price and capabilities still need a table, so a registry of some shape returns anyway.
- **Keeping the static list and adding a parallel adapter list** — two identities for one concept, with every consumer branching on which list it iterated.
- **One entry per adapter with an `engines[]` list** — two prices under one id, and the pre-flight estimate becomes a range.
- **Flattening the descriptor to provider + flat per-image price** — the two OpenRouter models are token-billed, and `has_vision` is a correctness gate: dropping it re-enables reference conditioning on a model that cannot accept an image.

## Consequences

- Reference conditioning for an adapter exists only through the edit engine, so "adapter against baseline, with and without reference conditioning" compares an edit call against text-to-image baselines; a benchmark verdict must name the engine it scores.
- Adapter cost amortizes per entry, so a `Training Run`'s break-even depends on which engine serves production.
- Tests that assert a four-model catalog (`curator/tests/openrouter_benchmark_tests.rs`) and the drawer's hardcoded checklist (`curator/src/ui/assets/index.html`) change with the registry, and the benchmark suite's default model list becomes the registry's selectable OpenRouter entries.
