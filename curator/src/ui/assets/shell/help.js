/**
 * Keyboard help overlay open/close.
 */

import { dom } from "../dom.js";
import { state } from "../store.js";
import { stopStageMotion } from "../stage/canvas.js";

export function toggleHelpModal() {
    stopStageMotion();
    state.helpModalOpen = !state.helpModalOpen;
    if (state.helpModalOpen) {
        dom.helpModal.classList.remove("hidden");
    } else {
        dom.helpModal.classList.add("hidden");
        dom.canvas.focus();
    }
}

export function closeHelpModal() {
    stopStageMotion();
    state.helpModalOpen = false;
    dom.helpModal.classList.add("hidden");
    dom.canvas.focus();
}
