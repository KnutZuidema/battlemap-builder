# Derived thumbnails for curated library images

The dataset library rendered every curated card from its full-resolution Ground layer: PNGs of 1–11 MB and up to 7560×8960 pixels, read whole by the curator pod on every request with no proxy or CDN in front of it. The grid now requests a derived image instead — long edge 768 px, JPEG quality 82, aspect preserved — from a rebuildable `thumbs/curated/<id>/` namespace through `GET /api/thumb/curated/<id>[/<layer_index>]`, generated on first request behind a single-decode limiter and eagerly after curation. Originals remain authoritative for fullscreen inspection, reference conditioning and dataset export.

## Considered Options

- **Serve originals with client virtualization vs. derived artifacts**: fewer requests still transferred tens of megabytes per viewport, and the server still read and buffered each multi-megabyte blob per request.
- **On-the-fly resize per request vs. persisted derivatives**: resizing on every request repeats the decode — a 7560×8960 layer is ≈271 MB of RGBA against a 1 GiB pod limit — and cannot be cached by the browser; persisted artifacts make the cost a one-time, bounded event.
- **Generate at ingestion vs. lazy ensure with curation precompute**: harvested candidates are mostly never reviewed, so generation is limited to layers the library displays, with curation precomputing the thumbs of newly curated maps.
- **Square center crop vs. aspect-preserved**: cards letterbox the whole map (`object-fit: contain`), so cropping would change what curators see.
- **Lossy WebP vs. JPEG**: the in-tree encoder writes lossless WebP only; a lossy WebP dependency would add a C build to the arm64 image for a modest byte saving.
- **Fall back to the original on failure vs. fail the request**: returning a multi-megabyte original as a "thumbnail" would silently mask a broken pipeline and recreate the transfer problem.

## Consequences

- Thumbnails are derived and safe to delete; they live outside the authoritative bundle and can be rebuilt, but `GET /api/storage/stats` does not count the `thumbs/` tree yet.
- The first request per layer after a cold cache pays a serialized decode of the source PNG; the library's first-ever visit fills in progressively.
- Layer edits invalidate a map's thumbnails. The client busts the URL of the map it just edited; other clients may see the previous thumbnail for up to the one-hour cache lifetime.
- Generation failures surface as an explicit `502` image error, not a degraded substitute.
