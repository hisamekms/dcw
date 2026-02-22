# dc – Devcontainer CLI Helper

A Rust CLI tool that wraps `devcontainer up` and adds dynamic TCP port forwarding via Docker sidecar containers. This enables `forwardPorts` support without VS Code by launching socat-based sidecars that publish ports from the devcontainer to the host.

## Build

```sh
cargo build --release
```

The binary is produced at `target/release/dc`.

## Usage

### Start a devcontainer

```sh
# Basic start
dc up

# Rebuild from scratch
dc up --rebuild

# Start and auto-forward ports defined in devcontainer.json
dc up --auto-forward

# Pass extra arguments to devcontainer CLI
dc up -- --config .devcontainer/custom.json
```

### Manage port forwards

```sh
# Forward host port 8080 to container port 8080 (detached)
dc port add -d 8080 8080

# Forward with different host/container ports
dc port add -d 3000 8080

# List active forwards
dc port list

# Remove a specific forward
dc port remove 8080

# Remove all forwards
dc port remove --all
```

### `forwardPorts` in devcontainer.json

`dc up --auto-forward` reads `forwardPorts` from `.devcontainer/devcontainer.json` (or `.devcontainer/devcontainer.local.json` as an override). Supported formats:

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

Sidecars are idempotent — running `dc port add` for an existing port replaces the previous sidecar.

## Requirements

- [devcontainer CLI](https://github.com/devcontainers/cli) (`npm install -g @devcontainers/cli`)
- Docker
