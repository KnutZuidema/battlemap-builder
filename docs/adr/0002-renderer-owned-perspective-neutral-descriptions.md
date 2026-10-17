# Renderer-owned camera and perspective-neutral descriptions

Prompt generators (`wikiPrompter`) and dataset vision describers (`curator`) emit strictly perspective-neutral Physical Descriptions focused on tangible architecture, materials, hazards, and lighting, forbidding camera or projection terms ("top-down", "orthographic", "2D plan view", "poché", "gridless"). Camera projection, Poché Wall slicing, and gridless directives are owned exclusively by the renderer's system prompt so Physical Descriptions remain portable across rendering models and training pipelines.
