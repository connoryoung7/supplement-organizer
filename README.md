# supplement-organizer

Rust (axum) API. Endpoints: `GET /` (name/version), `GET /health`.

## Quick start

```sh
cp .env.example .env   # fill in values
just run               # build image + run on http://localhost:3000
just up / down / logs  # detached container
just dev               # native `cargo run` (needs rustup)
```

## Deploying to Dokploy

1. In Dokploy, create an Application with source **Docker**, image `$IMAGE:latest`
   (add registry credentials if the image is private). Set container port `3000`.
2. Fill in `IMAGE` and either `DOKPLOY_WEBHOOK_URL` or `DOKPLOY_URL` / `DOKPLOY_API_KEY` / `DOKPLOY_APP_ID` in `.env`.
3. `docker login <registry>`, then `just deploy` — builds for `linux/amd64`, pushes `:<sha>` + `:latest`, and triggers the redeploy.

Alternatively, point a Dokploy application at this Git repo with build type **Dockerfile** and it will build the image itself.
