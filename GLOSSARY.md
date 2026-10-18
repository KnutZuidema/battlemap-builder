# Dungeon Forge

An AI-powered 2D tabletop RPG battlemap studio, interactive encounter canvas, and dataset curation workbench.

## Spatial & Rendering

**Battlemap**:
A top-down, ungridded 2D orthographic tactical map representing an encounter space at a scale of 1 Grid Cell = 5 tabletop feet.
_Avoid_: Map image, board, floorplan, scene

**Grid Cell**:
The fundamental spatial unit representing a 5×5 foot tabletop square, governing Selection snapping, Interactive Feature bounds, and the tactical grid overlay.
_Avoid_: Tile, grid square, square, block

**Grid Configuration**:
The spatial calibration of a Battlemap's tactical grid, defined by the pixel dimension of a single Grid Cell and the `(x, y)` pixel offset of the grid origin.
_Avoid_: Grid settings, grid calibration, grid params

**Poché Wall**:
An architectural wall rendered strictly as a flat horizontal cross-section sliced at eye level, showing solid interior fill and zero vertical side faces.
_Avoid_: Wall face, 3D wall, isometric wall

**Void**:
Unpainted space outside the current Battlemap bounds within a Context Window waiting to be filled with terrain.
_Avoid_: Padding, empty canvas, background

**Reference Image**:
A 16×16 Grid Cell Map Section extracted from a Curated Map and attached to a generation request to condition the model on orthographic plan-view perspective and 5-foot prop proportions.
_Avoid_: Style image, seed image, template

## Canvas & Forging

**Selection**:
A grid-snapped rectangular area on or adjacent to the Battlemap designated for forging or clearing.
_Avoid_: Target region, region selection, forge box, patch

**Context Window**:
A bounded sub-area of the canvas encompassing the Selection plus surrounding Battlemap terrain and any Void, providing spatial and style continuity during generation.
_Avoid_: Context frame, padded canvas, crop box

**Forge**:
The generative action of creating or modifying terrain within a Selection.
_Avoid_: Region forge, inpaint, render pass

**Reforge**:
A Forge operation whose Selection lies entirely within the existing Battlemap bounds, replacing or modifying existing terrain in place.
_Avoid_: In-place edit, region edit, overpaint

**Extend**:
A Forge operation whose Selection lies outside the existing Battlemap bounds in the Void, growing the Battlemap footprint. When extending North or West, existing terrain and Interactive Features are translated by the resulting origin offset.
_Avoid_: Region expand, outpaint, canvas expansion

**Bridge**:
A Forge operation whose Selection straddles both existing Battlemap terrain and the Void, modifying the boundary while extending the footprint. When extending North or West, existing terrain and Interactive Features are translated by the resulting origin offset.
_Avoid_: Hybrid forge, boundary edit, hybrid region

**Clear**:
The action of erasing all terrain within a Selection back to empty space.
_Avoid_: Delete region, wipe, cut

## Interactive Features & Play

**Edit Mode**:
The authoring state of the studio where the user generates terrain, modifies the canvas, and configures Interactive Features.
_Avoid_: Studio mode, build mode, author mode

**Play Mode**:
The tabletop encounter state of the studio where authoring tools are hidden and Interactive Features can be triggered between states.
_Avoid_: Run mode, game mode, preview mode

**Interactive Feature**:
A bounded architectural element on a Battlemap (`Doorway`, `Destructible Wall`, or `Hidden Floor Rug`) that toggles between two visual states during Play Mode.
_Avoid_: Map object, prop, hotspot, token

**Base State**:
The initial appearance of an Interactive Feature as depicted in the underlying Battlemap.
_Avoid_: State 1, default state, closed state

**Alternate State**:
The secondary appearance of an Interactive Feature after player interaction, such as a swung-open door, a breached wall, or a rug pulled back to reveal a trapdoor.
_Avoid_: State 2, alt state, open state, feature patch

## Campaign Wiki

**Region**:
A geographic territory in the campaign world that provides overarching lore and groups location Wiki Articles.
_Avoid_: Lore region, parent region, world area

**Wiki Article**:
A campaign lore document describing a specific location, adventure site, or setting used as source material for Battlemap generation.
_Avoid_: Lore file, markdown doc, location page

**Encounter Suggestion**:
A proposed Battlemap scenario derived from a Wiki Article, specifying an encounter title, tactical highlights, and a Physical Description.
_Avoid_: Wiki prompt, map concept, suggestion card

## Curation & Dataset

**Candidate**:
An uncurated Battlemap bundle in the staging queue awaiting human triage.
_Avoid_: Staging item, raw map, post, bundle

**Curated Map**:
A Battlemap that has passed triage with a persisted Grid Configuration, assigned Layer Roles, Tags, a Physical Description, and 16×16 Map Sections, stored in the curated dataset for reference conditioning and training.
_Avoid_: Approved map, dataset item, manifest

**Layer**:
An individual image within a Candidate or Curated Map representing a single vertical level or visual variation of the same encounter space.
_Avoid_: Slice, image file, sub-map, page

**Layer Role**:
The structural function assigned to a Layer: `Ground`, `Upper Floor`, `Basement`, `Roof`, `Variant`, or `Exclude` (with `Unassigned` prior to triage).
_Avoid_: Layer type, floor kind, category

**Elevation**:
The signed integer vertical offset of a Layer relative to the `Ground` layer (`0`), using positive integers for upper floors (`+1, +2`) and negative integers for basements (`-1, -2`).
_Avoid_: Z-index, floor number, level offset

**Tag**:
A descriptor attached to a Curated Map capturing its architectural setting, encounter archetype, environmental hazards, or notable props.
_Avoid_: Theme, category, genre, label, keyword

**Physical Description**:
A perspective-neutral prose description of a Battlemap's tangible architecture, materials, furnishings, hazards, and lighting, strictly free of camera angles or art-style directives.
_Avoid_: Caption, alt text, lore summary, meta-prompt

**Map Section**:
A 16×16 Grid Cell sub-region of a Curated Map aligned to its Grid Configuration and paired with its own Physical Description for reference conditioning and LoRA dataset export.
_Avoid_: Training crop, tile, slice, patch, chunk
