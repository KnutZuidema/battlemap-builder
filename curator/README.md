# Curator — Battlemap Ingestion, AI Benchmarking & Curation Service

**Curator** is an internal, high-performance Rust web service (Axum + Tokio + Embedded Web UI) for tabletop RPG battlemap ingestion, automated triage, AI model benchmarking, and dataset curation.

---

## 1. Architecture & Service Overview

Curator operates as a standalone service running on port `3002`:

- **Backend**: Built with Rust, Axum, Tokio, and Serde, featuring zero-copy asset handling and atomic filesystem persistence.
- **Frontend**: A zero-dependency embedded single-page application (`rust_embed`) with a high-performance 2D HTML5 canvas viewport, 64px VTT grid overlay, multi-layer filmstrip deck, and an interactive decision modal.
- **Ingestion Sources**:
  - Automated Reddit harvesting from `/r/battlemaps` with OAuth2 authentication and multi-image gallery bundle assembly.
  - Benchmark generation runs evaluating image models via OpenRouter (e.g. Gemini, Seedream, Muse).
  - Manual staging ingestion via the Ingestion Drawer.
- **Multi-Layer Bundle Support**: Staged maps support multi-level floorplans (Ground, Upper Floor, Basement, Roof, Variant, Exclude) with floor elevation tagging, split/regroup operations, and onion-skin comparison.

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
    └── Step 2: Custom Tags & Autocomplete
          ├── [Type text + Tab]     ──► Accept highlighted autocomplete pill
          └── [Enter on empty input]──► Finalize curation (requires >= 1 accepted theme)
    │
    ▼
POST /api/curate
    ├── Success: atomic move to data/curated/<id>/, update queue/counters, load next
    └── Error: danger toast, candidate preserved in staging
```

### 2.1. Upfront Layer Validation Gate

Before any curation decision modal opens, Curator validates the candidate's layer configuration:

1. **At least 1 layer included**: If a candidate has layers and all layers have role `exclude`, curation is blocked immediately with a danger toast:
   > *"Cannot curate: at least 1 layer must be included"*
2. **No unassigned layers**: If any included layer has role `unassigned` (or missing role), curation is blocked with a danger toast:
   > *"Cannot curate: layer X is Unassigned. Please assign a role."*
   The unassigned layer card is automatically scrolled into view and focused in the filmstrip deck, and role labeling mode (`1-6`) is activated.
3. **Modal opening**: Only when all layer validations pass does `#curate-decision-modal` open.

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

### 2.3. Keyboard Event Routing & Trapping

While `#curate-decision-modal` is active:
- Background candidate navigation (`ArrowLeft` / `ArrowRight`, `j` / `k`) is strictly disabled to prevent accidental queue shifting.
- Global triage shortcuts (`1-6` role assignment, `g` grid toggle, `f` fit screen) are suppressed.
- `Escape` dismisses and closes the modal cleanly.

### 2.4. Submission & State Handling

Upon completion, `submitDecisionCuration()` dispatches `POST /api/curate`:
- **Payload**: `{ id: candidate.id, themes: acceptedThemes, layers: candidate.layers }`.
- **On Success**:
  - Removes the candidate from staging queue and local memory image cache.
  - Updates staging and curated count indicators via `/api/status`.
  - Loads the next candidate in sequence.
  - Closes the decision modal.
- **On Error**:
  - Displays a danger notification toast.
  - Preserves the candidate in the staging queue without data loss.

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
│       ├── manifest.json
│       ├── 0_ground.png
│       ├── 1_upper_1.png
│       ├── <candidate_id>.png       # Primary display image
│       └── <candidate_id>.txt       # Companion LoRA training caption
└── rejections.jsonl
```

### Key Storage Guarantees

- **Flat `<id>/` Bundles**: Stored directly at `data/curated/<candidate_id>/manifest.json`, eliminating redundant nested theme directories (`data/curated/<theme>/<id>`) and enabling clean multi-theme labeling.
- **Deterministic Layer Filenames**: Layers are saved with standardized names: `<index>_<role>[_<elevation>].<ext>` (e.g. `0_ground.png`, `1_upper_1.png`, `2_basement_-1.png`).
- **Atomic Operations**: Files are written and moved atomically (`atomic_write`, `atomic_move`), ensuring crash-safe state updates.
- **LoRA Companion Assets**: Automatically generates paired `.png` and `.txt` caption files formatted with prompt triggers and architectural descriptors for model fine-tuning.
- **Backward Compatibility**: API routes serve curated images by both direct ID (`/api/image/curated/<id>`) and legacy theme routes (`/api/image/curated/<theme>/<id>`).

---

## 5. Development & Testing Commands

All commands are run from within the repository root or the `curator/` directory:

```bash
# Run unit and integration tests
cargo test --manifest-path curator/Cargo.toml

# Run the Curator service locally
cargo run --manifest-path curator/Cargo.toml

# Run specific API integration tests
cargo test --manifest-path curator/Cargo.toml --test api_integration_tests
```
