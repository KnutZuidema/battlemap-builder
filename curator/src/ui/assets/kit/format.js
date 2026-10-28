/**
 * Display formatting for layer roles, byte sizes, readiness ages and durations.
 */

export function formatRoleName(role, elevation) {
    const r = (role || "unassigned").toLowerCase().replace(/[-_]/g, "");
    if (r === "ground") return "Ground (0)";
    if (r === "upperfloor" || r === "upper") {
        const elev = typeof elevation === "number" ? elevation : 1;
        return `Upper (${elev >= 0 ? `+${elev}` : elev})`;
    }
    if (r === "basement" || r === "lower" || r === "lowerfloor" || r === "cellar") {
        const elev = typeof elevation === "number" ? elevation : -1;
        return `Basement (${elev >= 0 ? `+${elev}` : elev})`;
    }
    if (r === "roof") return "Roof";
    if (r === "variant") return "Variant";
    if (r === "exclude" || r === "excluded") return "Exclude";
    return "Unassigned";
}

export function formatBytes(bytes) {
    if (!bytes || bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    const val = bytes / Math.pow(k, i);
    return (val >= 10 || i === 0 ? val.toFixed(0) : val.toFixed(1)) + " " + sizes[i];
}

export function formatReadinessAge(iso) {
    if (!iso) return "never";
    const age = Date.now() - new Date(iso).getTime();
    if (!Number.isFinite(age) || age < 60000) return "just now";
    const minutes = Math.floor(age / 60000);
    if (minutes < 60) return `${minutes} min ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours} h ago`;
    return `${Math.floor(hours / 24)} d ago`;
}

export function formatDuration(seconds) {
    if (!seconds || seconds <= 0) return "0s";
    const sec = Math.round(seconds);
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = sec % 60;
    if (h > 0) return `${h}h ${m}m ${s}s`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
}
