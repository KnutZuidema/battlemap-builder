# Flat curated dataset storage with multi-tag metadata

Curated Maps are stored in a flat directory hierarchy (`data/curated/<id>/`) with dynamic multi-label Tags persisted in `manifest.json` rather than nested category directories (`data/curated/<category>/<id>/`). This allows a Curated Map to carry multiple Tags simultaneously and enables tag edits without moving files on disk.
