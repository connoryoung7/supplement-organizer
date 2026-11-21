set dotenv-load := true

app      := "supplement-organizer"
image    := env("IMAGE", app)
tag      := `git rev-parse --short HEAD 2>/dev/null || echo latest`
port     := env("PORT", "3000")
platform := env("DEPLOY_PLATFORM", "linux/amd64")

# List available recipes
default:
    @just --list

# ---- Local (native) ----

# Run natively with cargo (hot path for development)
dev:
    cargo run

# Run tests
test:
    cargo test

# Format + lint
check:
    cargo fmt --check
    cargo clippy -- -D warnings

# ---- Docker (local) ----

# Build the Docker image for the local machine
build:
    docker build -t {{app}}:latest .

# Build and run the container in the foreground on $PORT
run: build
    docker run --rm -it --name {{app}} \
        -p {{port}}:{{port}} \
        -e PORT={{port}} \
        {{ if path_exists(".env") == "true" { "--env-file .env" } else { "" } }} \
        {{app}}:latest

# Build and run the container detached
up: build
    -docker rm -f {{app}} >/dev/null 2>&1
    docker run -d --name {{app}} --restart unless-stopped \
        -p {{port}}:{{port}} \
        -e PORT={{port}} \
        {{ if path_exists(".env") == "true" { "--env-file .env" } else { "" } }} \
        {{app}}:latest
    @echo "running on http://localhost:{{port}}"

# Stop the detached container
down:
    -docker rm -f {{app}}

# Tail container logs
logs:
    docker logs -f {{app}}

# Hit the health endpoint of the running container
health:
    curl -fsS http://localhost:{{port}}/health && echo

# ---- Deploy (Dokploy) ----

# Build for the server platform and push :<git-sha> and :latest to $IMAGE
push:
    docker buildx build --platform {{platform}} \
        -t {{image}}:{{tag}} -t {{image}}:latest \
        --push .

# Push the image, then trigger a Dokploy redeploy
deploy: push trigger

# Trigger a Dokploy redeploy (webhook if DOKPLOY_WEBHOOK_URL is set, otherwise the API)
trigger:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -n "${DOKPLOY_WEBHOOK_URL:-}" ]]; then
        echo "→ triggering Dokploy webhook"
        curl -fsS -X POST "$DOKPLOY_WEBHOOK_URL"
    else
        : "${DOKPLOY_URL:?set DOKPLOY_URL or DOKPLOY_WEBHOOK_URL in .env}"
        : "${DOKPLOY_API_KEY:?set DOKPLOY_API_KEY in .env}"
        : "${DOKPLOY_APP_ID:?set DOKPLOY_APP_ID in .env}"
        echo "→ triggering Dokploy deploy for application $DOKPLOY_APP_ID"
        curl -fsS -X POST "${DOKPLOY_URL%/}/api/application.deploy" \
            -H "Content-Type: application/json" \
            -H "x-api-key: $DOKPLOY_API_KEY" \
            -d "{\"applicationId\": \"$DOKPLOY_APP_ID\"}"
    fi
    echo
    echo "✓ deploy triggered"

# Remove local image and build artifacts
clean:
    -docker rmi {{app}}:latest
    -cargo clean
