/**
 * Minimal DOM helpers.
 */

export function escapeHtml(str) {
    if (!str) return "";
    return String(str)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#039;");
}

/** Sanitize a Curated Map ID into a safe `.zip` archive filename matching the backend. */
export function battlemapArchiveFilename(mapId) {
    const safe = String(mapId || "")
        .replace(/[^A-Za-z0-9._-]/g, "")
        .replace(/^\.+|\.+$/g, "");
    return `${safe || "battlemap"}.zip`;
}

/** Trigger a browser file download for `blob` under `filename` and revoke the object URL. */
export function triggerBlobDownload(blob, filename) {
    const urlApi = typeof window !== "undefined" && window.URL ? window.URL : globalThis.URL;
    const downloadUrl = urlApi.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = downloadUrl;
    a.download = filename;
    const container = document.body || document.documentElement;
    container?.appendChild(a);
    if (typeof a.click === "function") {
        a.click();
    }
    if (typeof a.remove === "function") {
        a.remove();
    } else {
        a.parentElement?.removeChild(a);
    }
    urlApi.revokeObjectURL(downloadUrl);
}
