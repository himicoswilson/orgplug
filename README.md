# orgplug

A lightweight Rust CLI for preparing **Organization Plugins** used by Claude Cowork admin deployments.

- Docs reference: https://claude.com/docs/cowork/3p/extensions#organization-plugins-admin
- Goal: build a clean `org-plugins/` directory from upstream plugin sources, then sync it to an admin-managed destination.
- Default managed workdir: `~/.orgplug/workdir/orgplug`.

---

## Installation

### macOS

```bash
curl -fsSL https://raw.githubusercontent.com/himicoswilson/orgplug/main/scripts/install.sh | bash
```

### Windows (PowerShell)

```powershell
iwr https://raw.githubusercontent.com/himicoswilson/orgplug/main/scripts/install.ps1 -UseBasicParsing | iex
```

Optional:

- Install a specific released version:

```bash
export ORG_PLUG_VERSION=v0.1.0
curl -fsSL https://raw.githubusercontent.com/himicoswilson/orgplug/main/scripts/install.sh | bash
```

---

## Usage

Run from anywhere after installation (the CLI uses `~/.orgplug/workdir/orgplug` by default):

```bash
orgplug build
orgplug sync --platform macos
```

### Commands

- `doctor` — Validate repository/submodule/config structure.
- `update` — Update configured submodules safely (targeted by `.gitmodules`).
- `build` — Build `dist/org-plugins`.
- `sync` — Build then sync plugins into a target directory.

---

## Configuration

Default user config path:

```text
~/.orgplug/config.yaml
```

### Config model

```yaml
version: 1

rules:
  repos:
    plugins/anthropics-skills:
      skills:
        deny: []

    plugins/knowledge-work-plugins:
      plugins:
        deny: []

  plugins: {}
```

### Rule semantics

- `rules.repos.<repo>.skills.deny`: exclude specific skills from packaging.
- `rules.repos.<repo>.plugins.deny`: exclude specific repo-level plugins from packaging.

---

## Contribution

Contributions are welcome.

1. Create a feature branch.
2. Keep changes focused and minimal.
3. Run local checks:

```bash
rustc scripts/org_plugins.rs -o /tmp/orgplug
/tmp/orgplug doctor
/tmp/orgplug build
/tmp/orgplug sync --platform macos --dest /tmp/orgplug-test
```

4. Open a PR with:
   - problem statement
   - change summary
   - verification results
