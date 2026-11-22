# No authentication in the app

The app has no login and no notion of users; it must be deployed behind Tailscale or Dokploy (Traefik) basic auth. It is a single-person tool, so network-level access control is enough and keeps every table free of ownership columns. Exposing it directly on a public URL gives anyone full read/write access. If it ever needs multiple people, add accounts and an `owner_id` then.
