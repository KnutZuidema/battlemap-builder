# ⚔️ DUNGEON FORGE — AI TTRPG Battlemap Builder

**DUNGEON FORGE** is an interactive, full-stack web application designed for Tabletop RPG players and Game Masters (D&D 5e, Pathfinder, Cyberpunk RED) to generate, edit, and incrementally expand high-resolution 2D battlemaps using a **selectable model catalog** — Google Gemini through the Gen AI SDK, or any of the Gemini / OpenAI / GLM models available on OpenRouter.

---

## 🌟 Key Features

1. **🎨 Interactive Sketching Canvas**:
   - Draw rough room layouts, wall lines, doors, and water features using Pencil, Wall, Room Rect, and Eraser tools.
   - The sketch layer is a local planning aid on the canvas.

2. ** Selectable Output Format & Map Models**:
   - Two independent choices in the AI Gen tab: the **output format** (`SVG`, preferred, ahead of `Image`) and then the **model** that runs it. Switching format re-points the selection at that format's default model.
   - SVG is the cheap default path: `Gemini 3.6 Flash` (Google SDK) plus OpenRouter text models from GLM, DeepSeek and Kimi that draw the map as SVG geometry. `Image` adds the painted raster models (`Gemini 3.1 Flash Image`, `Gemini 3 Pro Image`).
   - Each entry shows its provider, whether it can see images, its measured behaviour and its list price.
   - Preset themes: *Dungeon Hallways, Wilderness Forest, Medieval Tavern, Sci-Fi Starship, Overgrown Temple*.
   - Rich prompt formatting tailored for 2D orthographic top-down battlemaps with zero perspective distortion and crisp grid alignment.

3. **🖌️ Detail Inpainting & Incremental Edits**:
   - Highlight any specific spot on the map with a mask brush.
   - Provide a targeted prompt (*"Add a glowing magical altar surrounded by skull candles"*) to update only the masked region while retaining map continuity.
   - Text-only models cannot see the canvas, so the Edit tab blocks the request and explains why instead of shipping a doomed call.

4. **🧭 Seamless Canvas Outpainting & Expansion**:
   - Expand your battlemap in any direction (**North, South, East, West, or All**) to extend corridors, add secret rooms, or widen forests.
   - Uses `sharp` image buffer processing to calculate overlapping boundary masks so the new terrain seamlessly stitches texture, lighting, and grid lines.

5. **📐 Customizable Grid & TTRPG Tools**:
   - Square & Hexagon grid overlays with adjustable cell size (32px to 128px), line opacity, grid color, and snap-to-grid.
   - **Playable Tokens**: Spawn draggable player & monster tokens (Fighter, Wizard, Rogue, Dragon, Treasure Chest).
   - **Distance Ruler Tool**: Click and drag to measure distances in 5ft grid increments for spell ranges & movement speeds.

6. **💾 Export & Save**:
   - Download High-Res PNG battlemaps for VTT software (Foundry VTT, Roll20, Owlbear Rodeo).
   - Save & Load full project JSON states.

7. **🚨 Loud Failure Reporting**:
   - The selected model is the only map source — there is no procedural stand-in. When a request fails, the app raises a rose banner naming the provider, the model, the cause (`NO_API_KEY`, billing, key restrictions, no SVG / no image, network), plus the per-model attempt trail, and logs the same telemetry to the Debug Logs inspector.

---

## 🚀 Quick Start

### Prerequisites
- Node.js (v18+) & npm

### Running the App
Run both the Express backend (`http://localhost:3001`) and Vite frontend (`http://localhost:3000`) simultaneously:

```bash
npm run dev
```

Open `http://localhost:3000` in your web browser.

---

## 🔑 API Configuration

Provider keys are configured server-side in `server/.env` (there is no client-side key input):

```env
PORT=3001
GEMINI_API_KEY=your_gemini_api_key_here
OPENROUTER_API_KEY=your_openrouter_api_key_here
```

- `GEMINI_API_KEY` (Google AI Studio) drives the `Gemini 3.6 Flash` entry through the Gen AI SDK. It can also live in the process environment instead of the file.
- `OPENROUTER_API_KEY` drives every OpenRouter entry in the selector. It can also live in the process environment instead of the file.
- `/api/health` reports which providers are configured (`providers.gemini`, `providers.openrouter`) and `/api/models` reports per-model `keyConfigured`, so the selector marks models whose key is missing. A request against a model without a key fails loudly (`NO_API_KEY`, HTTP 503) rather than falling back to procedural imagery.

---

## 🧠 Output Format & Model Selection

`server/services/modelCatalog.js` is the single source of truth, and `GET /api/models` ships it to the client as `{ formats, models }`. The two choices in the AI Gen tab are independent:

| Choice | What it decides |
|:---|:---|
| **Output format** — `svg` (listed first, so it is the default) or `image` | Filters which models may run. `svg` models emit SVG XML that `sharp` rasterises at the requested canvas size; `image` models render raster pixels themselves and the bytes are resized to the canvas |
| **Model** — the engines inside the chosen format | Which engine generates the map, and whether it can also run masked edits and expansion (it must be able to see images) |

Every request carries both `format` and `model`. A model of the other format is refused with `MODEL_FORMAT_MISMATCH` (HTTP 400) rather than silently reinterpreted, and a request naming neither runs the preferred format's default model. Switching format in the UI re-points the model selection at that format's default.

**SVG is the cheap path**: measured SVG maps cost $0.0007–$0.013 each, the image models $0.067–$0.139 — and the cheap OpenRouter text models (GLM, DeepSeek, Kimi) are the only ones that can generate for a tenth of a cent.

Probe results for a 1024×1024 *"flooded stone dungeon room…"* request (2026-09-24, this machine; list prices and per-call costs as reported by OpenRouter):

| Model | Format | Generate | Mask edit | Expand | Output character |
|:---|:---|:---|:---|:---|:---|
| `gemini-3.6-flash` | svg | not measurable at probe time — Google answered `UNAVAILABLE: this model is currently experiencing high demand` for it and its `gemini-3.5-flash` fallback | supported (vision) | supported (vision) | Vector-style top-down map |
| `z-ai/glm-5.3-flash` | svg | 133 s, $0.0007 | attempted and **failed**: blew the 32k SVG completion ceiling after 275 s, came back with no SVG | uses the canvas (vision) | Walled room, water ripples, circular altar |
| `deepseek/deepseek-v3.2` | svg | 68 s, $0.0007 | refused (HTTP 400, text-only) | blind (prompt only) | Flat grid schematic — tiles, doors, candles, altar |
| `z-ai/glm-4.7-flash` | svg | 161 s, $0.0014 | refused (HTTP 400, text-only) | blind: 89 s, $0.0015 → eastward strip of unrelated dark rock | Brick floor, torch sconces, glowing altar |
| `deepseek/deepseek-v4-flash` | svg | 88 s, $0.0032 | refused (HTTP 400, text-only) | blind (prompt only) | Detailed pillared hall, ring of torches, rune-lit altar (10.5k output tokens) |
| `z-ai/glm-4.5-air` | svg | 122 s, $0.0070 | refused (HTTP 400, text-only) | blind (prompt only) | Plank floor, four pillars, plain altar platform |
| `moonshotai/kimi-k2.5` | svg | 109 s, $0.0074 (one earlier attempt hung upstream past 20 min) | vision-capable; masked edits not probed | uses the canvas (vision) | Dark tiled room, cyan-lit altar, water ripples |
| `z-ai/glm-5.2` | svg | 107 s, $0.013 | refused (HTTP 400, text-only) | blind: 72 s, $0.048 → almost empty extension | Flat schematic room — geometric walls, torch glows, altar glyph |
| `google/gemini-3.1-flash-image` | image | 12–49 s, $0.067 | 14 s, $0.069 | 13 s, $0.068 → 1536×1024 | Painted battlemap: tiled floor, water reflections, torch lighting |
| `google/gemini-3-pro-image` | image | 23 s, $0.139 | same contract as Flash Image | same contract as Flash Image | Painted battlemap, higher fidelity |

Candidates probed and rejected from the catalog:

| Candidate | Why it is not selectable |
|:---|:---|
| `openai/gpt-5-image`, `openai/gpt-5-image-mini` | OpenRouter answers 404 `0 endpoints out of 1 requested are available matching your guardrail restrictions and data policy — ZDR violation (account settings)`. Re-enable the endpoints under https://openrouter.ai/settings/privacy and re-add the entries to `modelCatalog.js` to use them |

Mask containment was measured on the real-model edit: **0 pixels changed outside the mask bounding box**, 48 399 changed inside it. Latency on OpenRouter varies a lot per call (11 s – 49 s observed for the same image model and prompt), so the client timer reports the true round trip.

---

## 🏗️ Architecture

```
Battlemap Builder Monorepo
├── server/                    # Node.js + Express Backend
│   ├── index.js               # REST endpoints (/api/generate, /api/inpaint, /api/outpaint, /api/models, /api/health)
│   └── services/
│       ├── battlemapService.js  # Orchestrator: format + model resolution, prompt contract choice, compositing, error shape
│       ├── modelCatalog.js      # Output formats + selectable models and capabilities (contract, vision, provider key)
│       ├── prompts.js           # SYSTEM_PROMPTS (svg contract) + IMAGE_PROMPTS (image contract)
│       ├── failure.js           # Shared error summary + actionable failure sentences
│       ├── imageProcessor.js    # sharp mask composition & outpaint buffer slicing
│       └── providers/
│           ├── geminiProvider.js     # @google/genai SVG generation
│           ├── openrouterProvider.js # OpenRouter chat completions (svg + image contracts)
│           └── svgContract.js        # shared SVG attempt loop: extract <svg>, rasterise, record attempts
└── client/                    # Vite + React Frontend
    ├── index.html             # Document metadata
    └── src/
        ├── App.jsx            # State management, format + model selection, undo/redo history, failure banner
        ├── components/
        │   ├── Navbar.jsx         # Header navigation & map export menu
        │   ├── Sidebar.jsx        # Tabbed control panels (Sketch, AI Gen + format/model selectors, Edit, Expand, Grid, Tokens)
        │   ├── BattlemapCanvas.jsx# Konva 2D Stage with multi-layer rendering & pan/zoom
        │   └── DebugModal.jsx     # Prompt & telemetry inspector (incl. failed requests and per-call cost)
        └── services/
            └── api.js         # Fetch client for Express endpoints
```

---

## 📜 License
MIT
