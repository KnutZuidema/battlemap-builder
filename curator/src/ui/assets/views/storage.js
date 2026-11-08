        setStorageStatsPhase("error", {
            error: error && error.message ? error.message : String(error),
        });
        // The request module already raised the pill and toasted; clearing the markup's placeholder
        // figures keeps anything on screen from claiming a footprint that was never read.
        renderStorageStatsFallback();
        return null;
    }
}
