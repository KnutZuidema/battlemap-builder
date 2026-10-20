# FLUX 3 Image Klein LoRA training stack

`LoRA Adapter` training is standardized on `FLUX 3 Image Klein Base` (`black-forest-labs/flux-3-image-klein-base`, 5.6B, FLUX Kommunity License), fine-tuned with Ostris `ai-toolkit` (`arch: flux3`) in a `Training Run` executed on Modal's L40S 48GB tier ($1.95/hr) from a new top-level `training/` package. The baseline recipe is the vendor's own for this checkpoint: rank 32 / alpha 32, AdamW (0.9, 0.95) at lr 1e-4 with 200 warmup steps then linear decay, batch size 1, bfloat16, 4,000 steps, a checkpoint every 250. A 1,500-step calibration run (~$3.30) validates dataset packaging, Modal plumbing and the serving path before the full run (~$8.78). The published adapter is the checkpoint chosen by sample inspection, with its step number recorded in the adapter manifest — small datasets overfit well before 4,000 steps. Base and distilled checkpoints share an architecture, so an adapter trained on Base serves unmodified on the distilled model at 16 steps, CFG 1, LoRA scale 0.75.

Training consumes the existing `Training Dataset` of Ground-layer `Map Section`s at their native 1024×1024 with no resampling and no expansion to Upper Floor, Basement or Roof layers, published to Cloudflare R2 and fetched by URL. Every caption is content-only: `{Trigger Phrase}, {theme}, {details}, {Physical Description}`, with the `Trigger Phrase` set to the rare token `dungeonforge battlemap` (`TRIGGER_PHRASE` in `curator/src/domain/caption.rs`) and the style clause `flat 2D plan view, perpendicular orthographic camera, poché cut walls, strictly gridless` removed from the caption head. The orthographic conventions must bind to the token rather than to common English words the base model already understands, which is why the vendor's rule is to describe everything visible except the concept being taught; style terms stay stripped from `Physical Description` text.

`LoRA Adapter` inference is served by WaveSpeedAI (`wavespeed-ai/flux-3-klein-image-distilled/text-to-image-lora` at $0.015 per 1024×1024 `Battlemap`, and `…/edit-lora` at $0.021), which takes adapters as `loras: [{ path, scale }]` pointing at the R2 URL. Dungeon Forge authors the full-scene prompt itself — `Trigger Phrase` plus `Physical Description` — and sends it verbatim with host-side prompt rewriting disabled: this model requires full scene captions, and an upsampler would paraphrase the `Trigger Phrase` away.

## Considered options

- `FLUX.2 [dev]` and `FLUX.2 [klein] 9B Base` — the only families fal.ai serves `loras` for, but both are FLUX Non-Commercial, and `[dev]` needs multi-GPU training (56B total parameters with its standalone Mistral-3 encoder).
- `Qwen-Image-2.1` and `Ideogram 4.0 Instant` — research-only and non-commercial weights respectively, and neither is supported by `ai-toolkit`.
- Prodigy optimizer with rank 32 / alpha 16 and a dataset-scaled step count — no published support for this checkpoint; the vendor recipe supersedes it.
- RunPod Serverless as the training runner — cheaper per GPU-hour, but every script change needs a rebuilt and pushed Docker image.
- Modal self-hosted inference — the only route to masked inpainting with the adapter, at 15–25s cold boots or $60/day to stay warm.

## Consequences

- fal.ai is not the inference host: neither it nor the Black Forest Labs API exposes a `loras` parameter for FLUX 3 Klein (the `-finetuned` beta covers FLUX.2 [klein] only). At $0.015 per `Battlemap` against the current $0.070 Gemini 3.1 Flash Image baseline, one `Training Run` amortizes after ~160 generated `Battlemap`s.
- No hosted FLUX 3 Klein endpoint performs masked inpainting with a `LoRA Adapter` — WaveSpeedAI exposes bounding boxes only and Replicate's LoRA endpoint has no mask input — so canvas `Forge` stays on Reference Image conditioning until that is revisited.
- License posture is non-commercial until monetization. The FLUX Kommunity License grants free commercial use to Qualifying Users under $5M revenue, while Black Forest Labs' own model page states that product use and self-hosted deployment at scale need a commercial license; charging money requires a written license check first.
