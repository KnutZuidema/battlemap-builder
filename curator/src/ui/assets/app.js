// Curator SPA bootstrap
console.log("Dungeon Forge Battlemap Curator UI initialized.");

async function checkStatus() {
    const statusBar = document.getElementById("status-bar");
    try {
        const res = await fetch("/api/status");
        if (!res.ok) {
            throw new Error(`HTTP ${res.status}`);
        }
        const data = await res.json();
        statusBar.textContent = `Status: ${data.status} | Staging: ${data.staging_count} | Curated: ${data.curated_count}`;
    } catch (err) {
        statusBar.textContent = `Status check failed: ${err.message}`;
    }
}

window.addEventListener("DOMContentLoaded", () => {
    checkStatus();
});
