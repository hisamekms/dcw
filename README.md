# dcw – Devcontainer CLI Helper

A Rust CLI tool that wraps `devcontainer up` and adds dynamic TCP port forwarding via Docker sidecar containers. This enables `forwardPorts` support without VS Code by launching socat-based sidecars that publish ports from the devcontainer to the host.

## Install

**Quick install** (Linux x86_64 and aarch64):

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

### Start a devcontainer

```sh
# Basic start
dcw up

# Rebuild from scratch
dcw up --rebuild

# Start and auto-forward ports defined in devcontainer.json
dcw up --auto-forward

# Pass extra arguments to devcontainer CLI
dcw up -- --config .devcontainer/custom.json
```

### Manage port forwards

```sh
# Forward host port 8080 to container port 8080 (detached)
dcw port add -d 8080 8080

# Forward with different host/container ports
dcw port add -d 3000 8080

# List active forwards
dcw port list

# Remove a specific forward
dcw port remove 8080

# Remove all forwards
dcw port remove --all
```

### `forwardPorts` in devcontainer.json

`dcw up --auto-forward` reads `forwardPorts` from `.devcontainer/devcontainer.json` (or `.devcontainer/devcontainer.local.json` as an override). Supported formats:

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

## How it works

Port forwarding is implemented using Docker sidecar containers running `alpine/socat`. Each forwarded port gets its own sidecar named `pf-<workspace>-c<port>` that:

1. Joins the devcontainer's Docker network
2. Listens on the host port via `-p 127.0.0.1:<port>:<port>`
3. Forwards traffic to the devcontainer via socat

Sidecars are idempotent — running `dcw port add` for an existing port replaces the previous sidecar.

## Requirements

- [devcontainer CLI](https://github.com/devcontainers/cli) (`npm install -g @devcontainers/cli`)
- Docker
