/**
 * Keyboard help overlay open/close.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { stopPanLoop, stopZoomLoop } from "../stage/canvas.js";

export function toggleHelpModal() {
    stopPanLoop();
    stopZoomLoop();
    state.helpModalOpen = !state.helpModalOpen;
    if (state.helpModalOpen) {
        dom.helpModal.classList.remove("hidden");
    } else {
        dom.helpModal.classList.add("hidden");
        dom.canvas.focus();
    }
}

export function closeHelpModal() {
    stopPanLoop();
    stopZoomLoop();
    state.helpModalOpen = false;
    dom.helpModal.classList.add("hidden");
    dom.canvas.focus();
}
