# Curator frontend stays bundler-free, with a measured framework gate

The Curator web UI is embedded at compile time by `rust-embed`, so the Curator container builds Rust only and serves whatever sits in `curator/src/ui/assets/`. Today that is one ~9.7k-line `app.js` closure loaded by a plain `<script>`, meant to be split into native ES modules served verbatim: one `<script type="module">`, no bundler, no node stage in `curator/Dockerfile`, no extra CI job. Replacing the DOM cache and the scattered state-to-DOM writes uses a local tagged-template helper returning `DocumentFragment`s rather than a framework; escalation to a vendored Preact module is permitted only if converting the library grid drops fewer than 25% of its authored lines.

## Considered Options

- **Native ES modules vs Vite build.** Vite matches `client/` conventions and gives minification and content hashing, but the UI is served from a private ClusterIP with no caching pressure, and a bundler reintroduces a node build stage into a Rust-only image. React + Vite was additionally rejected because JSX plus hooks do not reduce authored lines relative to the helper, while adding a second ecosystem to maintain.
- **Local render helper vs vendored Preact/htm.** Preact + htm needs no build step (~10 KB embedded module) and gives a real component model, at the cost of owning vendored code inside the crate. The helper is kept as the first attempt because most of the estimated line savings come from deleting the element cache and duplicated status writes, not from the framework itself.
- **Deleting the source-text UI tests.** `curator/src/ui/mod.rs` asserted that asset files contain substrings. Those tests pin identifiers rather than behaviour, so they are to be replaced by `node --test` over pure kit modules (`just test-ui`, run in the Curator CI test job) plus browser verification for views.

## Consequences

- Embedded assets are neither minified nor cache-busted: a first load fetches roughly twenty module files. Revisit only if that measurably hurts.
- UI unit tests will live in `curator/src/ui/tests/`, outside the embedded folder, so test code never ships in the binary.
- The static handler's own contract (SPA fallback, `/api/*` returning JSON 404) is asserted in Rust, and that assertion survives the removal of the source-text tests.
- Any future proposal to add a bundler or a framework to this frontend must clear the gate above rather than argue from taste.
