# Ungridded battlemap generation with runtime grid overlay

Image generation models are instructed to render strictly ungridded Battlemap artwork calibrated to 5-foot tabletop human proportions, while the interactive canvas renders the tactical Grid Square overlay at runtime. Generating ungridded raster art prevents models from baking misaligned, uneven, or perspective-warped grid lines into floor textures and allows the grid overlay to be toggled or rescaled cleanly.
