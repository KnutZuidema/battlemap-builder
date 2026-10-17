# Direct raster output contract with no silent fallbacks

Map generation and Forge operations require the configured vision/image model to return raster image bytes directly rather than intermediate SVG markup or procedural geometry. When a provider key is missing or an upstream model call fails to produce an image, the system surfaces the failure and telemetry immediately rather than substituting mock imagery, switching models silently, or returning the unedited Battlemap.
