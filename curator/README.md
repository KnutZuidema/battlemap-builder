# Curator — Battlemap Ingestion, AI Benchmarking & Curation Service

**Curator** is an internal, high-performance Rust web service (Axum + Tokio + Embedded Web UI) for tabletop RPG battlemap ingestion, automated triage, AI model benchmarking, and dataset curation.

---

## 1. Architecture & Service Overview

Curator operates as a standalone service running on port `3002`:

- **Backend**: Built with Rust, Axum, Tokio, and Serde, featuring zero-copy asset handling and atomic filesystem persistence.
- **Frontend**: A zero-dependency embedded single-page application (`rust_embed`) with a high-performance 2D HTML5 canvas viewport, 64px VTT grid overlay, multi-layer filmstrip deck, and an interactive decision modal.
- **Ingestion Sources**:
  - Automated Reddit harvesting from `/r/battlemaps` with OAuth2 authentication and multi-image gallery bundle assembly.
  - Benchmark generation runs across the OpenRouter baselines (e.g. Gemini, Seedream, Muse) and published WaveSpeed LoRA adapters, driven by the derived model registry (`GET /api/models`).
  - Manual staging ingestion via the Ingestion Drawer.
- **Multi-Layer Bundle Support**: Staged maps support multi-level floorplans (Ground, Upper Floor, Basement, Roof, Variant, Exclude) with floor elevation tagging and split/regroup operations.

### Derived Model Registry (`GET /api/models`)

Model identity is computed at read time and never persisted (ADR-0010). The endpoint returns the OpenRouter specification entries plus, for every published and non-retired `LoRA Adapter` whose checkpoint artifact exists in storage, two entries:

- `{adapter.id}` — the text-to-image engine (`$0.015/image`, `supports_reference_image: false`)
- `{adapter.id}-edit` — the edit engine (`$0.021/image`, `supports_reference_image: true`)

Each descriptor carries `id`, `display_name`, `provider`, `api_type`, `has_vision`, `supports_reference_image`, `pricing` and `estimated_cost_per_image`; adapter entries additionally carry `adapter_id`, `run_id`, `step`, `lora_path` and `lora_scale`. Publishing or retiring an adapter changes the list on the next request, with no restart.

The Benchmark Drawer renders its checklist and pre-flight cost projection from this endpoint, and the benchmark runner resolves `{run_id}-s{step}[-edit]` through the registry, so a run uses that adapter's exact weights URL and scale and records cost under that exact id. Requesting reference conditioning for an entry with `supports_reference_image: false` is refused with `422 MODEL_NOT_REFERENCE_CAPABLE` (the drawer surfaces the blocked combination as an inline warning); retired adapters are absent and refused for new runs, but still resolve for recorded results and cost entries.

---

## 2. Step-by-Step Curation Workflow

Curation is an intentional, quality-gated process transforming raw staged maps into production training and reference assets.

```
Staging Queue (Canvas & Filmstrip Deck)
    │
    ▼ [Press Enter or click "Curate"]
Upfront Layer Validation Gate
    ├── All layers excluded? ──► [Danger Toast] ──► Block curation
    ├── Any layer unassigned? ──► [Danger Toast] ──► Focus layer in deck & enter role mode
    │
    ▼ (Validation Passed)
Curate Decision Modal (#curate-decision-modal)
    │
    ├── Step 1: Vision Suggestions (OpenRouter Vision)
    │     ├── [ArrowRight / Accept] ──► Accept theme tag
    │     ├── [ArrowLeft / Reject]  ──► Skip theme tag
    │     └── [Backspace / Undo]    ──► Step back to previous card
    │
    ├── Step 2: Custom Tags & Autocomplete
    │     ├── [Type text + Tab]     ──► Accept highlighted autocomplete pill
    │     └── [Enter on empty input]──► Proceed to description review
    │
    └── Step 3: Vision Description Generation (Gemini 3.5 Flash Lite)
          ├── Online: POST /api/staging/:id/describe ──► Rich 40-90 word physical description
          └── Vision unavailable: request fails explicitly (ADR-0005) ──► no synthesized fallback
    │
    ▼
POST /api/curate
    ├── Success: atomic move to data/curated/<id>/, persist description & metadata, load next
    └── Error: danger toast, candidate preserved in staging
```

### 2.1. Upfront Layer Validation Gate

Before any curation decision modal opens, Curator validates the candidate's layer configuration:

1. **At least 1 layer included**: If a candidate has layers and all layers have role `exclude`, curation is blocked immediately with a danger toast:
   > *"Cannot curate: at least 1 layer must be included"*
2. **No unassigned layers**: If any included layer has role `unassigned` (or missing role), curation is blocked with a danger toast:
   > *"Cannot curate: layer X is Unassigned. Please assign a role."*
   The unassigned layer card is automatically scrolled into view and focused in the filmstrip deck, and role labeling mode (`1-6`) is activated.
3. **16×16 Grid Cell curation gate**: The Ground layer must span at least 16×16 full Grid Cells under the candidate's persisted Grid Configuration. Otherwise curation is blocked with a danger toast:
   > *"Map has only N×M full Grid Cells; at least 16×16 required for curation"*
4. **Modal opening**: Only when all layer validations pass does `#curate-decision-modal` open.

### 2.2. Interactive Curate Decision Modal

The decision modal presents a streamlined two-step flow:

- **Step 1: AI Vision Theme Suggestions**:
  - Automatically fetches suggestions from `POST /api/staging/:id/suggest-themes`.
  - A multimodal vision model (e.g. `google/gemini-3.5-flash-lite` via OpenRouter) evaluates visual map features and proposes 3–5 contextual theme tags with rationale notes.
  - **Review Cards**: Reviewers step through cards with rapid keyboard shortcuts:
    - `ArrowRight` (or click Accept): Accepts the theme into `acceptedTags`.
    - `ArrowLeft` (or click Reject): Skips the suggestion.
    - `Backspace` / `ArrowUp` (or click Undo): Reverses the previous card choice.
  - **Offline Resilience**: If OpenRouter is unreachable, API keys are missing, or no suggestions are returned, the modal transitions directly to the custom step with an offline indicator badge.
- **Step 2: Custom Themes & Autocomplete**:
  - Input field with dynamic taxonomy autocomplete pills.
  - Pressing `Tab` adds the highlighted autocomplete pill.
  - Pressing `Enter` adds custom text.
  - Pressing `Enter` when the input is empty completes curation if at least 1 theme tag has been accepted.
  - **Zero-Tag Guard**: Attempting to finish with 0 accepted themes triggers a shake animation and validation error.

### 2.3. AI Vision Description Generation Workflow (`POST /api/staging/:id/describe`)

Curator features an automated vision description workflow powered by Google Gemini 3.5 Flash Lite (`google/gemini-3.5-flash-lite` via OpenRouter) to produce rich, evocative physical descriptions for every curated battlemap:

1. **Endpoint**: `POST /api/staging/:id/describe` (also accessible via `GET` for headless inspection).
   - **Request Payload**:
     ```json
     {
       "themes": ["dungeon", "crypt"],
       "tags": ["stone", "sarcophagus", "torches"]
     }
     ```
   - **Response**:
     ```json
     {
       "id": "candidate_123",
       "description": "An ancient subterranean crypt carved from damp bedrock features crumbling stone sarcophagi resting against masonry walls..."
     }
     ```
2. **Model Selection & Configuration**:
   - Default model: `google/gemini-3.5-flash-lite`.
   - Configurable via the `CURATOR_DESCRIPTION_MODEL` environment variable (falling back to `CURATOR_VISION_MODEL` or the default).
3. **Prompt Structure (DungeonForge Cartographer Prompts)**:
   - **System Instruction (`SYSTEM_INSTRUCTION_GENERATE_DESCRIPTION`)**: Formulated specifically for tabletop RPG cartography. Directs the vision model to analyze the image and describe tangible physical features:
     - Room architecture, masonry, stonework, woodwork, pillars, doors, and furnishings.
     - Environmental hazards, terrain obstacles, water, elevation differences, and tactical cover.
     - Visible lighting sources (braziers, torches, glowing moss, luminescence) and atmospheric textures.
   - **Strict Negative Constraints**:
     - **NO camera angles or perspective directives**: Strictly forbids terms like `"top-down"`, `"2D flat view"`, `"orthographic"`, or `"poché"` — camera and perspective are strictly owned by the renderer.
     - **NO art style meta-prompts**: Strictly forbids phrases like `"digital art"`, `"hand-painted"`, `"photorealistic"`.
     - **NO game mechanics**: Strictly forbids statistics, grid square counts, or rule system references.
   - **Output Format**: Dense, evocative 40–90 word description in plain text only (no markdown, no bullets, no commentary).
   - **User Prompt (`build_description_user_prompt`)**: Combines the candidate `title` (when present) and its `tags` with strict instructions to guide the model's focus onto the identified architectural features.
4. **Failure Propagation & Section Fallback (ADR-0005)**:
   - **Map-level physical description is strict**: there is no deterministic caption fallback. `POST /api/staging/:id/describe` and `POST /api/curated/:id/describe` propagate vision failures as HTTP errors: `503 Service Unavailable` when the vision engine is unconfigured (missing/invalid `OPENROUTER_API_KEY` or disabled) and `502 Bad Gateway` when the upstream provider fails or returns an empty description.
   - During curation (`POST /api/curate`), an omitted map-level description is generated through the same strict path; if vision is unavailable, curation fails with the propagated error instead of persisting a synthesized description.
   - **Per-section descriptions are best-effort**: for multi-section maps, each 16×16 section is described independently and falls back to the map-level `Physical Description` when its vision call fails or its crop is unavailable, so a single failed section never blocks curation or regridding. Single-section maps reuse the map-level description verbatim. Captions are never synthesized here; the `Trigger Phrase` is applied once, at export time.

### 2.4. Keyboard Event Routing & Trapping

While `#curate-decision-modal` is active:
- Background candidate navigation (`ArrowLeft` / `ArrowRight`, `j` / `k`) is strictly disabled to prevent accidental queue shifting.
- Global triage shortcuts (`1-6` role assignment, `g` grid toggle, `f` fit screen) are suppressed.
- `Escape` dismisses and closes the modal cleanly.

### 2.5. Submission & State Handling

Upon completion, `submitDecisionCuration()` dispatches `POST /api/curate` **without awaiting the response**, so the triage loop never stalls on vision description generation:
- **Payload**: `{ id: candidate.id, tags: acceptedTags, description: generatedDescription, layers: candidate.layers }` (tags-only; the legacy `themes` field is still accepted for compatibility but is never sent by the UI).
- **Optimistic advance**: the decision modal closes and the candidate is dropped from the local queue and the in-memory image cache *before* the request is sent; the next candidate loads immediately and the id is recorded in `state.pendingCurationIds`, so a staging refetch (harvest completion, manual refresh) cannot resurrect an in-flight candidate.
- **On Success** (2xx response received later): success toast `Candidate #<id> curated`, and staging/curated counts are refreshed via `/api/status`.
- **On Error** (non-2xx response or network failure): `restoreUncuratedCandidate()` puts the candidate back into the local queue at the position it was curated from, and a danger toast reports `Curate failed for #<id>: <server error>`.
  - Error bodies are `{ "error": "<message>" }`; the toast surfaces that message instead of a bare status code.
  - No data is lost: nothing was committed, and the candidate is still listed in staging.
- **Server-side caveat**: `POST /api/curate` still generates descriptions to completion before responding — the strict map-level path (ADR-0005) and the atomic commit are unchanged. The UI merely stops waiting for it, so refreshing or closing the page mid-flight aborts the request and leaves the candidate in staging.
---

## 3. Dynamic Themes & Star Rating Removal

- **Dynamic Multi-Label Themes**:
  - Replaces legacy single fixed theme enums with flexible multi-label dynamic themes (e.g. `["dungeon", "crypt", "boss-arena"]`).
  - Themes represent architectural archetypes, biome settings, and environmental characteristics.
  - Supports flexible querying and dataset export across overlapping archetypes.
- **Removal of Star Ratings**:
  - Legacy 1–5 star ratings have been completely removed from storage models, API contracts, and the UI.
  - Quality evaluation in Curator is binary: candidates either meet production standards and are curated, or are rejected with structured rejection reasons (`rejections.jsonl`).

---

## 4. Flat Curated Storage Layout

Curator organizes all curated assets in a deterministic, flat storage hierarchy under `data/curated/<candidate_id>/`:

```
data/
├── staging/
│   └── <candidate_id>/
│       ├── manifest.json
│       ├── 0.png
│       └── 1.png
├── curated/
│   └── <candidate_id>/
│       ├── manifest.json            # Contains tags, dimensions, grid, sections, and description (legacy "themes" is read-only compatibility)
│       ├── 0_ground.png             # Deterministically named layer file
│       └── 1_upper_1.png
└── rejections.jsonl
```

### Key Storage Guarantees

- **Flat `<id>/` Bundles**: Stored directly at `data/curated/<candidate_id>/manifest.json`, eliminating redundant nested theme directories (`data/curated/<theme>/<id>`) and enabling clean multi-theme labeling.
- **Description Persistence**:
  - **Manifest**: The `description` string is persisted directly in `data/curated/<id>/manifest.json`.
  - **Primary Layer Caption**: The primary layer (Ground role) `caption` field matches and retains the curated description.
  - **Dynamic LoRA Dataset Export**: LoRA dataset export dynamically crops and Lanczos3-resamples each 16×16 MapSection on demand from the Ground layer into `{theme}/{id}_c{col}_r{row}_lora1024.png` (exact 1024×1024, zero padding bars) and `{theme}/{id}_c{col}_r{row}_lora1024.txt` companion captions: the `Trigger Phrase` (`TRIGGER_PHRASE`) followed by that section's `Physical Description` — its own cached one, otherwise the map-level one. The Trigger Phrase is applied exactly once, at caption generation, and never stored inside a `Physical Description`.
  - **Perspective-Neutral Section Descriptions**: a Map Section's `description` is generated per crop by vision; when that call fails or the crop is unavailable, the section reuses the map-level `Physical Description` instead of a synthesized caption, so a `Physical Description` never carries camera, perspective, art-style or grid directives (ADR-0002, ADR-0007).
  - **Backward Compatibility & Legacy Deserialization**: Legacy `manifest.json` files lacking the `"description"` field deserialize cleanly (with `description: None`). `GET /api/curated` is a pure read: it returns persisted manifests as-is and never synthesizes or rewrites captions. The only handlers registered on `/api/curated/{id}` are `PATCH` and `DELETE`; updates happen exclusively through `PATCH /api/curated/{id}`.
- **Reference Provider Prompt Keyword Scoring & Section Cropping (`ReferenceProvider`)**:
  - Curated map sections and physical descriptions are directly leveraged in the benchmark and generation pipeline for few-shot orthographic perspective conditioning.
  - `ReferenceProvider::score_curated_section_for_prompt` scores keyword overlap between generation prompts and `(CuratedMap, MapSection)` pairs:
    - **Tag match**: +2 points per matching tag (legacy themes are stored as tags, so they score identically).
    - **Section description match**: +1 point per matching word in `section.description`.
  - The highest scoring `(CuratedMap, MapSection)` pair is selected, and its exact `16 * cell_size_px` Grid Cell box is cropped from the Ground layer and Lanczos3-resampled.
  - Theme-only resolution selects the most recently curated map and its center-most 16×16 section.
  - Sections are never rescaled: when the exact box does not fit the loaded image the section is **unresolvable**, the reference conditioning path falls back to the static reference, and the LoRA ZIP export logs a warning and skips that section instead of center-cropping or magnifying it.
- **Deterministic Layer Filenames**: Layers are saved with standardized names: `<index>_<role>[_<elevation>].<ext>` (e.g. `0_ground.png`, `1_upper_1.png`, `2_basement_-1.png`).
- **Atomic Operations**: Manifests and blobs are written atomically (`atomic_write` in the filesystem catalog and blob adapters), ensuring crash-safe state updates.
- **Theme-Free Image Routes**: Curated images are served by id (`/api/image/curated/<id>` and `/api/image/curated/<id>/<layer_index>`); theme remains a listing/export filter (`/api/curated?theme=...`), never part of a storage path.

---

## 5. Configuration & Environment Variables

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `OPENROUTER_API_KEY` | *(None)* | OpenRouter authentication key for vision models and image benchmarking |
| `CURATOR_DESCRIPTION_MODEL` | `google/gemini-3.5-flash-lite` | Model ID for AI vision battlemap description generation (`POST /api/staging/:id/describe`) |
| `CURATOR_VISION_MODEL` | `google/gemini-3.5-flash-lite` | Fallback vision model ID for theme suggestions and descriptions |
| `PORT` | `3002` | Curator Axum HTTP server port |

---

## 6. Development & Testing Commands

Run local checks narrowly; the full curator suite, clippy and the container build run in PR CI
([`.github/workflows/curator-ci.yml`](../.github/workflows/curator-ci.yml)). Keep the local machine usable: run only
the test filter or suite covering the change. Commands run from the repository root.

```bash
# Preferred locally: focused tests matching a name
just test-curator cost_ledger

# Run one integration suite
just test-curator-suite api_integration_tests

# Full suite + clippy (CI parity; heavy, reproduce CI failures only)
just check-curator

# Run the Curator service locally
cargo run --manifest-path curator/Cargo.toml
```

---

## 7. LoRA Dataset Viewer (`tools/lora-viewer.html`)

Single-file, offline browser app for reviewing exported LoRA dataset ZIPs. Open `curator/tools/lora-viewer.html`
directly in Chrome 103+/Firefox 113+/Safari 16.4+ (`file://` works) and drop one or more `lora-dataset.zip`
archives — or pick them via **Load ZIP…**. Archives are parsed in the browser (central directory + raw DEFLATE
via `DecompressionStream`); nothing is uploaded and no network access is used.

- **Samples**: every `<theme>/<id>_c<col>_r<row>_lora1024.png` is paired with its `.txt` companion caption.
- **Flipping**: `←`/`→` (or `j`/`k`) step one sample, `↑`/`↓`/`PageUp`/`PageDown` step ten, `Home`/`End` jump to
  the ends, `Space` toggles a 1.4 s slideshow, and the filmstrip below the stage jumps directly.
- **Viewport**: same controls and speeds as the curator viewport (`curator/src/ui/assets/shell/hotkeys.js`): `W`/`A`/`S`/`D`
  pan continuously at 700 px/s (diagonals normalized), `[`/`]` zoom continuously at 2×/s, `-`/`+` step 1.2×,
  `F`/`0` fit, `1` shows actual pixels, wheel zooms at the cursor, drag pans, double-click toggles fit/1:1.
  Zoom is clamped to 10%–500% and survives sample navigation.
- **Map overview (`M` or the Overview button)**: reconstructs the whole map the current section came from and marks
  every section box with its `c{col} r{row}` cell origin. The export stores 16×16-cell crops at a uniform scale
  (1024 px per section) and `GridConfig::section_origins` places the last origin at `cells − 16`, so the full
  lattice is `max(col) + 16` × `max(row) + 16` cells: drawing each crop at `(col, row) × 64 px` stitches the map
  back together (overlapping sections simply overdraw, lattice holes skipped by the export render dashed).
  The current section is highlighted and clicking any box selects that section — the detail panel, filmstrip and HUD
  follow — without leaving the overview (the search is cleared first if it hid the section). `Enter` or a
  double-click opens the selected crop, `M`/`Escape` leave the overview. Built canvases are cached per map (LRU 3).
- **Filtering**: theme dropdown (built from the archive's theme folders) plus a search box matching map id, theme,
  section coordinates, and full caption text.
- **Inspection**: `G` overlays the 64 px cell lattice (16×16 cells on a section crop, the whole map in overview;
  line and marker borders/labels are divided by the zoom so they stay 1 screen pixel thin), and `c` copies the
  current caption.
- **Loading**: image bytes stay in the archive and are inflated on demand (LRU-capped object URLs, neighbours
  prefetched); captions are read eagerly at load so search covers the whole dataset.

## 8. Training Dataset Readiness Audit (`GET /api/dataset/readiness`)

`lora-dataset.zip` only ships samples a `Training Run` can consume, so the curated store is audited
against that contract before a dataset leaves the machine. The audit decodes each curated map's Ground
layer, plans its samples through the exact same policy the ZIP export uses, and reports every defect
with the operator-facing detail needed to fix it.

```
GET /api/dataset/readiness            # whole curated store
GET /api/dataset/readiness?theme=cave # one theme, i.e. exactly the ZIP folder scope
```

```json
{
  "ready": false,
  "maps_total": 3, "maps_ready": 1,
  "sections_total": 6, "sections_ready": 4,
  "maps": [
    { "id": "contaminated_legacy", "theme": "cave", "sections": 1, "ready_sections": 0,
      "issues": [{ "code": "physical_description_not_neutral",
                   "detail": "Map Section (0, 0) description holds camera, perspective, art-style or grid directives: \"…\"" }] }
  ]
}
```

An empty store is **not** a ready dataset (`ready: false`): there is nothing to train on.

### Issue codes and remediation

| Code | Meaning | Fix |
| :--- | :--- | :--- |
| `ground_layer_missing` | No Ground layer image could be read from the bundle | Restore `0_ground.png` in `data/curated/<id>/` |
| `ground_image_unreadable` | The Ground layer bytes do not decode as an image | Re-ingest the map; an export would skip it entirely |
| `grid_below_minimum` | The persisted Grid Configuration yields no full 16×16 Grid Cell window | Recalibrate the grid (`PATCH /api/curated/{id}` with a `grid` that fits the Ground layer) |
| `section_unresolvable` | A Map Section's exact `16 * cell_size_px` box does not fit the Ground layer | Regrid, or re-curate the map; the export skips that section |
| `physical_description_missing` | The section (and the map) resolve to no `Physical Description`, so its caption would carry only the Trigger Phrase, themes and tags | `POST /api/curated/{id}/describe` with `{"apply": true}` regenerates and persists the description plus every Map Section description |
| `physical_description_not_neutral` | The stored description holds camera, perspective, art-style or grid directives instead of physical prose | `POST /api/curated/{id}/describe` with `{"apply": true}` regenerates it under the strict neutral prompt and recomputes the section descriptions per crop |

The embedded UI owns the audit in the **Frozen Dataset Snapshots** drawer, directly above the shelf: a
single readiness block for the active theme scope, summarized **by issue kind** rather than per map. It
shows the ready/total counts, the scope, the age of the stored audit, and one row per issue code with the
number of affected maps and the one-click fix. There is no separate readiness button or modal.

- **Stored audits**: a report is cached per theme scope in `localStorage`
  (`curator_dataset_readiness`), so opening the library, switching views, or reloading the page renders
  the last audit without re-decoding every layer. Only an explicit **Re-audit**, a scope with no cached
  report, or a curated write (curate, delete, description apply, grid save, inspector save) triggers a
  new request. A curated write also bumps an audit generation, so a request that began before the write
  is neither joined nor cached: the panel drops to *Not audited* and a fresh audit runs behind the stale
  one. A scope switch is handled the same way, so the newly selected scope is audited even while the
  previous scope's request is still running.
- **Freeze refusals** (`DATASET_NOT_READY`) carry the full audit of the theme that was frozen, so the
  refusal seeds that scope's cache and renders the block (highlighted, scrolled into view) plus a danger
  toast; the results stay visible after the toast disappears. If the operator switched themes while the
  freeze ran, the report is still stored under the frozen scope and only the toast fires for the active
  one. A successful freeze returns only snapshot metadata, so it leaves the stored audit untouched
  rather than inventing a clean report.
- **Remediation** per issue kind:
  - **Description codes** (`physical_description_missing`, `physical_description_not_neutral`) offer
    *Fix descriptions*: one `POST /api/curated/{id}/describe` with `{"apply": true}` per affected map,
    sequentially, with the button labelled `Regenerating n/total…`. The request regenerates the strict
    perspective-neutral `Physical Description`, persists it, and recomputes every Map Section
    description through the same policy the ZIP export reads. A map already regenerating reuses that
    in-flight request instead of being skipped, and the action always reports its outcome.
  - **Grid codes** (`grid_below_minimum`, `section_unresolvable`) offer *Edit grid*, which opens the
    fullscreen grid calibration editor for the first affected map (the toast names how many maps still
    need a grid fix).
  - **Ground layer codes** (`ground_layer_missing`, `ground_image_unreadable`) offer *Inspect bundle*,
    which opens the curated inspector drawer for the first affected map.

A successful action re-audits the active scope once, so resolved kinds leave the list and the counts
update without leaving the drawer; a description that is generated but not persisted, or that still fails
the audit, surfaces as a danger/warning toast. Failures propagate the same way (`503` when vision is
unconfigured, `502` on upstream failure).

Legacy bundles curated before ADR-0007 carry no `sections` and no `description`: their full-grid lattice
still exports (one 16×16 window per lattice) but their captions have no `Physical Description`, which the
audit reports as `physical_description_missing`.

### Bulk AI refinement of selected library maps

The library bulk bar (**AI Refine Selected**) refreshes every selected curated map in one action:
`POST /api/curated/{id}/refine` (POST only, no payload) returns the persisted `CuratedMap`.

The endpoint reuses the existing refinement machinery rather than a parallel pipeline:

1. **Suggestions**: the curated Ground layer is sent through the same cached vision theme-suggestion
   path as `POST /api/curated/{id}/suggest-themes`, so a suggestion set already in
   `theme_suggestions_cache` is reused and a fresh set is stored. A missing or failing vision adapter
   yields *empty* suggestions — it never fails the request on its own.
2. **Merge rules**: `merged tags = normalize_tags(existing tags ++ suggestions)` — case-insensitive
   de-duplication with first-seen wins, so the map's existing tags stay first and suggestions only add
   new ones (`clean_tags` trims, formats and caps the suggestion list at 5 first).
3. **Strict description**: the description engine is called exactly like the inspector/readiness
   remediation path (`describe` with the merged tags and no title). Vision failures propagate through
   the standard error mapping — `503` when unconfigured, `502` on upstream failure — and **nothing is
   persisted** when it fails.
4. **Sections**: Map Section descriptions are planned exactly like the `describe` apply branch, from
   the decoded Ground layer and `ground_dimensions`, with the merged tags and the new map description.
5. **Single write**: one `library.update_curated` stores `tags`, `description` and `sections` together,
   so a map is never left with partially refreshed metadata.

The UI loops the selected ids sequentially (never a batch endpoint), labels the button
`Refining i/n…` and disables it while running. Each success replaces the map in `state.curatedMaps`;
an open inspector on that map refreshes its draft tags, description and layers only when it has no
unsaved edits (a dirty draft is kept). After any success the library filters are re-applied and
`invalidateDatasetReadiness()` clears the cached audits, because tags, description and sections all
feed the readiness contract above. The final toast is a success only when every map succeeded;
otherwise it names the failed ids with the first error, and each failure is logged to the console.
