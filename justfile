set dotenv-load := true

app      := "supplement-organizer"
image    := env("IMAGE", app)
tag      := `git rev-parse --short HEAD 2>/dev/null || echo latest`
port     := env("PORT", "3000")
platform := env("DEPLOY_PLATFORM", "linux/amd64")

# Local Postgres (see `just db`). Containers on the `dev` network reach it by name.
network      := app + "-dev"
db_container := app + "-db"
db_name      := "supplement_organizer"
db_url_net   := "postgres://postgres:postgres@" + db_container + ":5432/" + db_name

# List available recipes
default:
    @just --list

# ---- Local (native; needs rustup + sqlx-cli) ----

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

# Apply migrations to $DATABASE_URL (the app also runs them at startup)
migrate:
    sqlx migrate run

# Regenerate .sqlx/ query metadata after changing any SQL (commit the result)
prepare: migrate
    cargo sqlx prepare

# ---- Toolbox (no local Rust needed) ----

# Run a command in a Rust container wired to the local DB, e.g. `just toolbox cargo test`
[positional-arguments]
toolbox *args:
    #!/usr/bin/env bash
    set -euo pipefail
    docker run --rm -i $([ -t 0 ] && echo -t) --network {{network}} \
        -v "$PWD":/app -w /app \
        -v {{app}}-cargo:/usr/local/cargo/registry \
        -v {{app}}-target:/app/target \
        -v {{app}}-tools:/opt/tools \
        -e PATH=/opt/tools/bin:/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        -e DATABASE_URL={{db_url_net}} \
        rust:1-slim-trixie bash -c '
            command -v sqlx >/dev/null || cargo install --quiet --root /opt/tools sqlx-cli \
                --no-default-features --features postgres,rustls
            # The slim image ships without rustfmt and clippy.
            case " $* " in *fmt*|*clippy*) rustup component add rustfmt clippy >/dev/null 2>&1 ;; esac
            exec "$@"' -- "$@"

# ---- Database (local) ----

# Start a local Postgres 17 (data persists in a Docker volume)
db:
    @docker network create {{network}} >/dev/null 2>&1 || true
    @docker start {{db_container}} >/dev/null 2>&1 || docker run -d --name {{db_container}} \
        --network {{network}} -p 5432:5432 \
        -e POSTGRES_USER=postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB={{db_name}} \
        -v {{app}}-pgdata:/var/lib/postgresql/data \
        postgres:17 >/dev/null
    @echo "postgres running on localhost:5432 (db {{db_name}})"

# Stop the local Postgres (data is kept)
db-down:
    -docker stop {{db_container}}

# Open psql against the local Postgres
psql:
    docker exec -it {{db_container}} psql -U postgres {{db_name}}

# ---- Docker (local) ----

# Build the Docker image for the local machine
build:
    docker build -t {{app}}:latest .

# Build and run the container in the foreground on $PORT (against `just db`)
run: build db
    docker run --rm -it --name {{app}} \
        --network {{network}} \
        -p {{port}}:{{port}} \
        {{ if path_exists(".env") == "true" { "--env-file .env" } else { "" } }} \
        -e PORT={{port}} \
        -e DATABASE_URL={{db_url_net}} \
        {{app}}:latest

# Build and run the container detached (against `just db`)
up: build db
    -docker rm -f {{app}} >/dev/null 2>&1
    docker run -d --name {{app}} --restart unless-stopped \
        --network {{network}} \
        -p {{port}}:{{port}} \
        {{ if path_exists(".env") == "true" { "--env-file .env" } else { "" } }} \
        -e PORT={{port}} \
        -e DATABASE_URL={{db_url_net}} \
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
