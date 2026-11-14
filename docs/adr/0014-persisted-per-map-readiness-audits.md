# Persisted per-map readiness audits

The dataset readiness audit decoded every curated map's Ground layer on every call: `GET /api/dataset/readiness` re-read the whole library, and every snapshot freeze audited the entire scope before packaging, so freezing an unchanged library paid the same full decode pass again. The audit now runs **once per curated map**: the result is persisted in the catalog (`readiness/<id>.json` on the filesystem backend, the `map_readiness` table in PostgreSQL) together with a fingerprint of the audited map, and both the readiness endpoint and the freeze reuse it while that fingerprint still matches. Only maps that are new or edited are audited.

## Considered Options

- **Re-audit on every request vs. persist per map**: the audit decodes full-resolution Ground layers and classifies every Map Section; at 184 maps that is the dominant cost of both the readiness panel and every freeze, and it repeats unchanged work.
- **One persisted report per theme scope vs. one record per curated map**: scopes overlap (a map belongs to exactly one theme folder, but `all` covers every theme), so a per-scope cache would duplicate records and re-audit a map when only its scope changed. Per-map records are independent of the query.
- **Invalidate by `curated_at` vs. by a content fingerprint**: `curated_at` is set when the bundle is filed, but curation edits (grid changes, descriptions, section descriptions, layer roles) mutate the manifest in place. A fingerprint over the serialized curated map invalidates on any audit-relevant change, including ones that never touch `curated_at`.
- **Persist in object storage vs. the catalog**: the record is catalog metadata keyed by map id, read for a whole scope at once; a blob per map would add a storage round trip per map on every readiness read. The catalog already stores derived documents (dataset snapshots) as indexed JSONB.
- **Keep the client's localStorage audit cache as the only cache**: it is per browser, per scope, and invisible to the freeze endpoint, which is why a freeze could never skip the audit.
- **Fall back to a stale audit when persistence fails vs. fail**: a stale readiness report would let a freeze package a map that no longer passes, so save failures surface as the request's error.

## Consequences

- Freezing an unchanged, already-audited library performs no Ground-layer decode during auditing; only the packaging pass (which must read the pixels anyway) touches layers.
- `GET /api/dataset/readiness?refresh=true` is the explicit re-audit; the panel's "Re-audit" button sends it. Without it, a report may be older than the store, which the response now exposes as `audited_at` (newest per-map audit).
- Editing a curated map changes its fingerprint and invalidates its record on the next audit; deleting the map deletes the record in every adapter.
- A report whose issue is a Ground-layer read or decode failure (`ground_layer_missing`, `ground_image_unreadable`) is returned but not persisted: a transient blob outage must not pin the map as unready. The next audit retries the layer, and `refresh=true` re-verifies deliberately.
- The filesystem backend gains a `readiness/` directory beside `curated/`; like thumbnails it is derived and safe to delete (it is rebuilt on the next audit), and the disk→PostgreSQL catalog migration does not import it.
