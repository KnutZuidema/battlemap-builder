# DUNGEON FORGE — root task runner.
#
# Curator is deployed from the image published to GHCR — the same registry and tag
# scheme the cluster pulls (ghcr.io/knutzuidema/curator:<sha>, see
# .github/workflows/curator-cd.yml). The nodes pull the image; nothing runs locally.
#
#   just curator-login     # one-time GHCR auth (gh CLI token, write:packages)
#   just curator-deploy    # publish this commit (linux/arm64) and roll the cluster onto it
#   just curator-publish   # publish only: build + push, no rollout
#   just curator-rollout   # roll the cluster onto an already published tag
#   just port-forward      # forwarded access to the running service on localhost:3002
#
# The cluster nodes are arm64 (Hetzner cax11), so linux/arm64 is what gets published;
# the Dockerfile cross-compiles that leg natively on an amd64 machine (no qemu).
# `CURATOR_IMAGE_TAG` selects the tag (default: this commit, the scheme CD deploys).

set shell := ["bash", "-uc"]

# Published image repository (must match the CD workflow's REGISTRY/IMAGE_NAME)
registry_image := env_var_or_default("CURATOR_IMAGE", "ghcr.io/knutzuidema/curator")

# Platforms published: the cluster is arm64 (Hetzner cax11) and CI publishes arm64 only
platforms := env_var_or_default("CURATOR_PLATFORMS", "linux/arm64")

# Tag published and deployed (default: this commit, the scheme CD deploys)
image_tag := env_var_or_default("CURATOR_IMAGE_TAG", `git rev-parse HEAD`)

# Cluster manifests and justfile used by the rollout, port-forward and cluster recipes.
# Relative to the repository root, which is where just runs every recipe (the checkout
# path contains a space, so these are always interpolated quoted).
deploy_dir := "deploy/curator"

# List available tasks
default:
    @just --list

# Log in to GHCR using the GitHub CLI token (the curator package is private)
curator-login:
    #!/usr/bin/env bash
    set -euo pipefail
    user=$(gh api user --jq .login)
    gh auth token | docker login ghcr.io --username "$user" --password-stdin

# Publish the cluster image for this commit: build it and push it to GHCR
curator-publish tag=image_tag:
    #!/usr/bin/env bash
    set -euo pipefail
    image="{{registry_image}}:{{tag}}"
    docker buildx build \
        --file curator/Dockerfile \
        --platform {{platforms}} \
        --tag "$image" \
        --push \
        .
    echo "published $image"
    published=$(docker buildx imagetools inspect "$image" | grep -oE 'linux/[a-z0-9]+' | sort -u || true)
    printf '%s\n' "$published" | grep -v '^$' || true
    for want in $(printf '%s' "{{platforms}}" | tr ',' ' '); do
        if [ -n "$published" ] && ! printf '%s\n' "$published" | grep -qx "$want"; then
            echo "published index is missing $want; check the active builder with 'docker buildx ls'" >&2
            exit 1
        fi
    done

# Roll the cluster onto an already published tag and wait for the rollout
curator-rollout tag=image_tag:
    #!/usr/bin/env bash
    set -euo pipefail
    image="{{registry_image}}:{{tag}}"
    published=$(docker buildx imagetools inspect "$image" 2>/dev/null | grep -oE 'linux/[a-z0-9]+' | sort -u || true)
    if [ -z "$published" ]; then
        echo "$image could not be read back; is that tag published and are you logged in ('just curator-login')?" >&2
        exit 1
    fi
    echo "$image ships: $(printf '%s ' $published)"
    if ! printf '%s\n' "$published" | grep -qx "linux/arm64"; then
        echo "$image has no linux/arm64 leg; the cax11 nodes cannot run it" >&2
        exit 1
    fi
    kubectl set image -f "{{deploy_dir}}/deployment.yaml" curator="$image" --local -o yaml | kubectl apply -f -
    kubectl rollout status deployment/curator -n curator --timeout=180s

# Deploy this commit: publish the image, then roll the cluster onto that exact tag
curator-deploy tag=image_tag: (curator-publish tag) (curator-rollout tag)

# Port-forward the cluster Curator service to localhost:3002
port-forward:
    just --justfile "{{deploy_dir}}/justfile" port-forward

# Run any deploy/curator/justfile recipe, e.g. `just cluster status`, `just cluster tail-logs`
cluster *args:
    just --justfile "{{deploy_dir}}/justfile" {{args}}
