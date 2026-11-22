# supplement-organizer

Tracks the supplement bottles you own, the doses you take, and which bottles are on course to expire before you finish them, so that as few bottles as possible expire.

- **Today** shows what's due, with one tap to log each product's planned dose.
- **Bottles** are taken in earliest-expiry order. The app opens the next bottle automatically and projects every bottle forward at your actual (or planned) rate.
- **At risk** lists every bottle that would still hold units at its expiry date, plus a scorecard of how many have expired.

Rust (axum) server-rendered with Askama + htmx, Postgres via sqlx. Installable to a phone's home screen as a PWA.

Docs: [`CONTEXT.md`](CONTEXT.md) (vocabulary) · [`docs/design.md`](docs/design.md) (rules and screens) · [`docs/adr/`](docs/adr/) (decisions).

## Quick start

```sh
cp .env.example .env   # set TZ to your time zone
just db                # local Postgres 17 in Docker
just run               # build image + run on http://localhost:3000
just up / down / logs  # detached container
```

Native development (needs rustup and `cargo install sqlx-cli --no-default-features --features postgres,rustls`):

```sh
just migrate           # apply migrations to $DATABASE_URL (needed before the first build)
just dev               # cargo run
just test              # unit tests (domain logic: replay, rates, projections, scorecard)
```

No Rust installed? `just toolbox <command>` runs any command in a Rust container wired to the local database, e.g. `just toolbox cargo test` or `just toolbox sqlx migrate run`.

### Changing SQL

Queries are checked at compile time with `sqlx::query!`. The Docker build has no database, so it uses the metadata committed in `.sqlx/`. **After adding or changing a query or migration, run `just prepare`** (or `just toolbox cargo sqlx prepare`) and commit `.sqlx/`.

## Configuration

| Variable | Default | |
|---|---|---|
| `DATABASE_URL` | (required) | Postgres connection string |
| `TZ` | `UTC` | IANA time zone deciding when "today" rolls over |
| `PORT` | `3000` | |
| `RUST_LOG` | `info` | |

Migrations run automatically at startup.

## Deploying to Dokploy

> **The app has no login.** Put it behind Tailscale or Dokploy (Traefik) basic auth before exposing it. See [ADR 0003](docs/adr/0003-no-in-app-auth.md).

1. In Dokploy, create a **Postgres** database service. Turn on its scheduled backups.
2. Create an Application with source **Docker**, image `$IMAGE:latest` (add registry credentials if the image is private). Set container port `3000`, and set `DATABASE_URL` (the Postgres service's internal connection URL) and `TZ` in its environment.
3. Fill in `IMAGE` and either `DOKPLOY_WEBHOOK_URL` or `DOKPLOY_URL` / `DOKPLOY_API_KEY` / `DOKPLOY_APP_ID` in `.env`.
4. `docker login <registry>`, then `just deploy`. That builds for `linux/amd64`, pushes `:<sha>` + `:latest`, and triggers the redeploy.

Alternatively, point a Dokploy application at this Git repo with build type **Dockerfile** and it will build the image itself.

`GET /health` returns `{"status":"ok"}` and is used by the container's `HEALTHCHECK`.
