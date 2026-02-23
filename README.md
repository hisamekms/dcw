[English](README.md) | [日本語](README.ja.md)

# dcw – Devcontainer Wrapper

> **Alpha**: This project is in alpha. APIs and command interfaces may introduce breaking changes without notice.

A Rust CLI tool that wraps the `devcontainer` CLI and extends it with:

- **Dynamic port forwarding** — socat-based Docker sidecar containers that publish ports from the devcontainer to the host
- **Automatic port watching** — detects new listening ports inside the container and forwards them automatically
- **Config merging** — deep-merges `devcontainer.local.json` on top of `devcontainer.json`
- **Lifecycle management** — `up` / `exec` / `down` for the full devcontainer lifecycle

## Typical Usage Pattern

```sh
$ dcw up                          # Start the devcontainer (watch enabled by default)

# Inside the container, a service starts listening on a port
$ dcw exec -- python -m http.server 8080 &
# => dcw detects port 8080 and automatically forwards it to the host

$ curl localhost:8080             # Access from the host
# => 200 OK

$ dcw down                        # Stop everything (watcher, sidecars, container)
```

## Install

**Quick install** (Linux x86_64/aarch64, macOS Apple Silicon):

```sh
curl -fsSL https://raw.githubusercontent.com/hisamekms/dcw/main/install.sh | bash
```

**Override install directory:**

```sh
DCW_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/hisamekms/dcw/main/install.sh | bash
```

**Pin a specific version:**

```sh
VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/hisamekms/dcw/main/install.sh | bash
```

## Build from source

```sh
cargo build --release
```

The binary is produced at `target/release/dcw`.

## Usage

### `dcw up`

Start the devcontainer.

```sh
# Basic start (auto-forward and watch are enabled by default)
dcw up

# Rebuild from scratch
dcw up --rebuild

# Disable automatic port forwarding from devcontainer.json
dcw up --auto-forward=false

# Disable automatic port watching
dcw up --watch=false

# Pass extra arguments to devcontainer CLI
dcw up -- --config .devcontainer/custom.json
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--rebuild` | bool | `false` | Remove existing container and rebuild |
| `--auto-forward` | bool | `true` | Forward ports defined in `forwardPorts` after start |
| `--watch` | bool | `true` | Watch for new listening ports and auto-forward them |

Extra arguments after `--` are passed through to `devcontainer up`.

### `dcw down`

Stop the devcontainer. This performs cleanup in order:

1. Stop the port watcher (if running)
2. Remove all port-forwarding sidecar containers
3. Stop the devcontainer

```sh
dcw down
```

### `dcw exec`

Execute a command inside the devcontainer. If a merged config exists (from `devcontainer.local.json`), it is automatically applied.

```sh
dcw exec -- ls -la
dcw exec -- bash
```

All arguments after `--` are passed through to `devcontainer exec`.

### `dcw port`

Manage port forwards.

#### `dcw port add`

```sh
# Forward host port 8080 to container port 8080 (detached)
dcw port add -d 8080 8080

# Forward with different host/container ports
dcw port add -d 3000 8080
```

| Argument | Description |
|----------|-------------|
| `<host_port>` | Port on the host |
| `<container_port>` | Port in the container |

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d`, `--detach` | bool | `false` | Run in background |

#### `dcw port remove` (alias: `rm`)

```sh
# Remove a specific forward
dcw port remove 8080

# Remove all forwards
dcw port rm --all
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--all` | bool | `false` | Remove all port forwards |

#### `dcw port list` (alias: `ls`)

```sh
dcw port list
dcw port ls
```

#### `dcw port watch`

Watch for new listening ports inside the container and forward them automatically.

```sh
# Start watching with defaults
dcw port watch

# Custom interval and exclude specific ports
dcw port watch -i 5 --min-port 3000 -e 5432 -e 6379
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i`, `--interval` | seconds | `2` | Polling interval |
| `--min-port` | u16 | `1024` | Minimum port number to forward |
| `-e`, `--exclude` | u16 (repeatable) | — | Ports to exclude from auto-forwarding |

### `dcw update`

Update dcw to the latest version.

```sh
# Update to latest
dcw update

# Install a specific version
dcw update --version v0.2.0

# Force reinstall
dcw update --force
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--version` | string | latest | Install a specific version |
| `--force` | bool | `false` | Update even if already on the latest version |

## `forwardPorts` in devcontainer.json

When `--auto-forward` is enabled (the default), `dcw up` reads `forwardPorts` from `.devcontainer/devcontainer.json`. If `.devcontainer/devcontainer.local.json` exists, it is deep-merged on top before reading ports. Supported formats:

```jsonc
{
  "forwardPorts": [
    3000,                    // number
    "8080",                  // string
    "localhost:9090",        // host:port string
    { "port": 5432 }         // object
  ]
}
```

### Config merge behavior

`devcontainer.local.json` is deep-merged into `devcontainer.json`:

- **Objects** are merged recursively (keys from local override base)
- **Arrays and scalars** are replaced entirely (not appended)

The merged result is written to the XDG runtime directory as `devcontainer.merged.json` and used by `dcw exec` automatically.

## How it works

### Sidecar port forwarding

Port forwarding uses Docker sidecar containers running `alpine/socat`. Each forwarded port gets its own sidecar named `pf-<workspace>-c<port>` that:

1. Joins the devcontainer's Docker network
2. Listens on the host port via `-p 127.0.0.1:<port>:<port>`
3. Forwards traffic to the devcontainer via socat

> **Note**: If the devcontainer is connected to multiple Docker networks, the first network found is used for sidecar communication.

Sidecars are idempotent — running `dcw port add` for an existing port replaces the previous sidecar.

### Automatic port watching

`dcw port watch` (and `dcw up --watch`) polls `/proc/net/tcp` and `/proc/net/tcp6` inside the container to detect LISTEN sockets. When a new listening port is found (above `--min-port` and not in `--exclude`), a sidecar is created automatically. When a port stops listening, its sidecar is removed.

The watcher PID is stored in the XDG runtime directory so that `dcw down` can stop it during cleanup.

### Config file merging

If `.devcontainer/devcontainer.local.json` exists, `dcw up` deep-merges it on top of `devcontainer.json` and writes the result to the XDG runtime directory (`$XDG_RUNTIME_DIR/dcw/<workspace>/devcontainer.merged.json`). This merged config is then passed to `devcontainer up` and `devcontainer exec` via the `--config` flag.

## Requirements

- [devcontainer CLI](https://github.com/devcontainers/cli) (`npm install -g @devcontainers/cli`)
- Docker
