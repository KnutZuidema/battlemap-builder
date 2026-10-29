/**
 * Grid geometry constants and lattice math shared by the triage canvas and the
 * fullscreen stage.
 */

export const VTT_GRID_SIZE = 64; // 64px represents standard 5ft D&D VTT square
export const MIN_GRID_SIZE = 16;
export const MAX_GRID_SIZE = 300;
export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 5.0;

/**
 * Calculates the full-grid lattice dimensions, section counts, and eligibility.
 */
export function calculateLatticeInfo(width, height, cellSize, offsetX, offsetY) {
    const size = Math.max(16, cellSize || 64);
    const ox = ((Math.round(offsetX || 0) % size) + size) % size;
    const oy = ((Math.round(offsetY || 0) % size) + size) % size;
    const cols = Math.floor(Math.max(0, (width || 0) - ox) / size);
    const rows = Math.floor(Math.max(0, (height || 0) - oy) / size);
    const isEligible = cols >= 16 && rows >= 16;
    const kx = isEligible ? Math.ceil(cols / 16) : null;
    const ky = isEligible ? Math.ceil(rows / 16) : null;
    return {
        size,
        ox,
        oy,
        cols,
        rows,
        isEligible,
        kx,
        ky,
        fullW: cols * size,
        fullH: rows * size,
    };
}

/**
 * The HUD grid readout: cell size, offset (when set) and visibility.
 */
export function formatGridStatus(view) {
    const size = view.gridSize || VTT_GRID_SIZE;
    const ox = Math.round(view.gridOffset?.x ?? 0);
    const oy = Math.round(view.gridOffset?.y ?? 0);
    const visibility = view.gridVisible === false ? "OFF" : "ON";
    if (ox !== 0 || oy !== 0) {
        return `Grid: ${size}px (${ox > 0 ? `+${ox}` : ox}, ${oy > 0 ? `+${oy}` : oy}) [${visibility}]`;
    }
    return `Grid: ${size}px [${visibility}]`;
}

/**
 * The lattice badge: whole cells, and their 16×16 section split when eligible.
 */
export function formatLatticeStatus(lattice) {
    if (lattice.isEligible) {
        return `${lattice.cols} × ${lattice.rows} cells (${lattice.kx} × ${lattice.ky} sections)`;
    }
    return `${lattice.cols} × ${lattice.rows} cells (< 16×16 cells)`;
}

/**
 * The Grid Configuration panel's two lattice readouts, split across the cells and
 * sections spans both panels render.
 */
export function formatLatticePanel(lattice) {
    return {
        cells: `${lattice.cols} × ${lattice.rows} cells`,
        sections: lattice.isEligible ? `${lattice.kx} × ${lattice.ky} sections` : "< 16×16 cells",
    };
}
