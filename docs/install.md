# Install And Setup

PGSandbox is distributed as a native Rust binary. It needs a reachable Postgres admin connection that can create databases and roles.

## Homebrew

```bash
brew install LVTD-LLC/tap/pgsandbox-mcp
pgsandbox-mcp setup --client codex --admin-url postgres://postgres:postgres@localhost:5432/postgres
```

This uses the [LVTD-LLC/homebrew-tap](https://github.com/LVTD-LLC/homebrew-tap) repository, which Homebrew addresses as `LVTD-LLC/tap`.

## GitHub Install Script

For users who do not use Homebrew:

```bash
curl -fsSL https://raw.githubusercontent.com/LVTD-LLC/pgsandbox-mcp/main/scripts/install.sh | sh
pgsandbox-mcp setup --client codex --admin-url postgres://postgres:postgres@localhost:5432/postgres
```

The installer fetches the latest GitHub release for the current OS and CPU,
installs `pgsandbox-mcp` to `~/.local/bin`, and verifies checksums when the
release includes `pgsandbox-mcp-<version>-checksums.txt`.

Pin a version or install somewhere else with environment variables:

```bash
curl -fsSL https://raw.githubusercontent.com/LVTD-LLC/pgsandbox-mcp/main/scripts/install.sh | PGSANDBOX_VERSION=0.1.0 sh
curl -fsSL https://raw.githubusercontent.com/LVTD-LLC/pgsandbox-mcp/main/scripts/install.sh | PGSANDBOX_INSTALL_DIR=/usr/local/bin sh
```

## From Source

```bash
cargo install --path .
pgsandbox-mcp setup --client codex --admin-url postgres://postgres:postgres@localhost:5432/postgres
```

From GitHub without cloning first:

```bash
cargo install --git https://github.com/LVTD-LLC/pgsandbox-mcp --tag v0.1.0
pgsandbox-mcp setup --client codex --admin-url postgres://postgres:postgres@localhost:5432/postgres
```

## Supported Clients

```bash
pgsandbox-mcp setup --client codex --admin-url "$PGSANDBOX_ADMIN_DATABASE_URL"
pgsandbox-mcp setup --client cursor --scope project --admin-url "$PGSANDBOX_ADMIN_DATABASE_URL"
pgsandbox-mcp setup --client vscode --scope project --admin-url "$PGSANDBOX_ADMIN_DATABASE_URL"
pgsandbox-mcp setup --client claude-desktop --admin-url "$PGSANDBOX_ADMIN_DATABASE_URL"
pgsandbox-mcp setup --client all --admin-url "$PGSANDBOX_ADMIN_DATABASE_URL"
```

## Verify

```bash
pgsandbox-mcp doctor
pgsandbox-mcp smoke-test
```

Then restart your MCP client and ask it to create a disposable Postgres sandbox.
