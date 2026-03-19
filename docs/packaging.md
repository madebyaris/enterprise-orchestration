# Packaging

## Desktop build

The desktop shell is packaged through Tauri.

### Build command

```bash
pnpm --filter desktop build
```

This command:

1. builds the shared React control UI,
2. compiles the Tauri desktop shell,
3. produces Linux bundles under `target/debug/bundle/`.

## Current Linux outputs

The repository currently produces:

- `.deb`
- `.rpm`
- `.AppImage`

## Rust toolchain

The repository pins Rust through `rust-toolchain.toml`.

## Linux system dependencies

The Tauri desktop shell requires GTK/WebKit development packages during builds:

```bash
sudo apt-get update
sudo apt-get install -y \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

## Smoke testing

Useful verification commands:

```bash
cargo test
pnpm --filter control-ui build
pnpm --filter desktop build
```

The desktop runtime can also be smoke-tested under `xvfb-run` to confirm:

- the Tauri app launches,
- the embedded control server responds on `/health`,
- the root UI is served,
- remote pairing tokens authorize non-local API requests.
