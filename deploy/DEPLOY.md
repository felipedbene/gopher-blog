# Deploy — gopher-blog on the RackNerd VPS (`:7071`)

The blog runs as a **single immutable container** on the same VPS as gopher-cta:
geomyidae + the baked phlog tree (see [`../Dockerfile`](../Dockerfile)). CI
publishes a multi-arch image to `ghcr.io/felipedbene/gopher-blog:latest`; the
**existing cta Watchtower** (label-enabled, same Docker daemon) recreates this
container whenever a new digest lands. The container swap is the atomic publish.

cta (`:70`) is untouched. The two holes are stitched only by the menu cross-links
already shipped (cta root → `:7071`, blog root → `:70`).

## One-time setup on the VPS

```sh
# 1. Place the compose file (e.g. alongside the cta stack).
mkdir -p ~/gopher-blog/deploy
# copy deploy/docker-compose.yml there, or clone this repo.

# 2. Pull + start (ghcr package is public — no login needed).
cd ~/gopher-blog
docker compose -f deploy/docker-compose.yml up -d

# 3. Open the port in the firewall (whichever this VPS uses):
sudo ufw allow 7071/tcp                         # if ufw
# or, raw iptables:
sudo iptables -A INPUT -p tcp --dport 7071 -j ACCEPT && sudo netfilter-persistent save

# 4. (Already done for cta, but ensure) the shared access-log dir is writable
#    by geomyidae's `nobody` (uid 65534):
sudo mkdir -p /var/log/gopher && sudo chown 65534:65534 /var/log/gopher
```

That's it. The cta Watchtower (`--label-enable`, 5-min poll) now auto-updates
`gopher-blog` on every CI publish — no further manual steps.

## Verify

```sh
# from the VPS
docker compose -f deploy/docker-compose.yml logs --tail=20
printf '\r\n' | nc 127.0.0.1 7071 | head            # root menu (.gph)
printf '/posts\r\n' | nc 127.0.0.1 7071 | head      # posts index

# from anywhere
lynx gopher://gopher.debene.dev:7071/
# and confirm the hub link resolves: cta root (:70) -> "Phlog -- the blog" -> here
```

## Updating content

The image bakes `debene-dev@main` at build time. To republish after a content
change, trigger a rebuild:

- **Daily:** CI already rebuilds on a schedule (06:17 UTC) and re-tags `:latest`,
  so content changes go live within a day with zero action.
- **On demand:** `gh workflow run CI` (or push any commit to `gopher-blog`).
- **Later fio:** a `repository_dispatch` from a `debene-dev` push → instant
  rebuild (not wired yet; the daily schedule covers v1).

## Rollback

`ghcr.io/felipedbene/gopher-blog` keeps `:sha-<commit>` tags. To pin a previous
build, set `image:` to that tag and `docker compose ... up -d` (and temporarily
remove the watchtower label so it isn't re-pulled to `:latest`).
