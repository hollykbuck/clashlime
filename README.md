```text
 ██████╗ ███╗   ███╗ █████╗ ███████╗██╗  ██╗
██╔═══██╗████╗ ████║██╔══██╗██╔════╝██║  ██║
██║   ██║██╔████╔██║███████║███████╗███████║
██║   ██║██║╚██╔╝██║██╔══██║╚════██║██╔══██║
╚██████╔╝██║ ╚═╝ ██║██║  ██║███████║██║  ██║
 ╚═════╝ ╚═╝     ╚═╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝
```

`omash` is forked from
[Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) and
reworked as a fast, native terminal dashboard for Mihomo, built for
[Omarchy](https://omarchy.org/). It carries the upstream Mihomo management
design into a Rust TUI without a browser runtime.

The TUI is only the control surface. Mihomo runs under a self-managed
user daemon (`omash --daemon`), so closing `omash` does not stop your proxy.

<p align="center">
  <img src="screenshots/1.jpg" alt="omash with a blue Omarchy theme" width="49%">
  <img src="screenshots/2.jpg" alt="omash with an orange Omarchy theme" width="49%">
</p>

## Features

- Imports local profiles and remote subscriptions, with per-profile update settings (auto-update switch, interval, timeout, fetch via proxy, auth token, User-Agent) and rename
- Supports Rule, Global, and Direct modes, proxy selection, and delay tests
- Manages active connections, Merge enhancements, backups, and logs
- Non-privileged: auto-discovers `mihomo` at `$OMASH_MIHOMO` → `~/.local/bin/mihomo` → `~/.local/share/omash/bin/mihomo` → `$PATH` → `/usr/bin/mihomo`
- Self-managed core via `omash --daemon` (socket IPC in `$XDG_RUNTIME_DIR`; autostart via `systemd --user` unit, enabled on first TUI run)
- Portable single binary: `omash --help` creates no files; first TUI run auto-creates config/data
- Own logs at `~/.local/share/omash/logs/omash-{tui,daemon}-YYYY-MM-DD.log` merged with `mihomo-YYYY-MM-DD.log` in the Logs tab
- Configurable DNS (`[dns] enable/listen/ipv6/nameserver/fallback/enhanced-mode/fake-ip-range`), hot-patched via `PATCH /configs`
- Static/dynamic config split: static `XDG_CONFIG_HOME/omash/config.toml` (defaults) + dynamic `XDG_DATA_HOME/omash/config.json` (TUI writes)
- GitHub Releases update check for Mihomo (`MetaCubeX/mihomo`) via `omash update check` and TUI Settings
- Updates `gsettings` and the UWSM/systemd environment for newly launched apps
- Follows the active Omarchy palette, with optional theme overrides
- Provides an optional Omarchy Shell widget for common controls
- Works in `herdr`/`tmux`-like multiplexers (alternate-screen aware)

## Install

System `mihomo`/`clash-geoip` is optional now. `omash` will run without them
and start the core once you import a profile; missing resources only warn.

```bash
# Optional on Omarchy/Arch (still supported):
omarchy pkg aur add mihomo clash-geoip

# Only needed when Cargo is not already installed:
omarchy install dev-env rust
source "$HOME/.cargo/env"
```

Install `omash`, then launch it once to finish setup:

```bash
curl -fsSL https://raw.githubusercontent.com/ourongxing/omash/main/scripts/install | bash
omash
```

The installer writes the binary and systemd user unit under your home directory; it
does not require `sudo`. Options:

```bash
# Non-privileged without system mihomo: auto-download core + GeoIP
curl -fsSL https://raw.githubusercontent.com/ourongxing/omash/main/scripts/install | bash -s -- --with-core
# Or allow missing core and fetch later from TUI
curl -fsSL https://raw.githubusercontent.com/ourongxing/omash/main/scripts/install | bash -s -- --allow-missing-core
```

On first launch, `omash` creates `~/.config/omash/config.toml` and
`~/.local/share/omash/{profiles,logs,backups,runtime.yaml}`, starts the
daemon, and enables login startup because `auto_start = true` by default.
Set `$OMASH_MIHOMO` or place a binary at `~/.local/bin/mihomo` to override
discovery; `Country.mmdb` may live at `~/.local/share/omash/Country.mmdb` or
`geo/Country.mmdb` without `/etc`.

### Install from source

After installing the dependencies above, build and install manually:

```bash
git clone https://github.com/ourongxing/omash.git
cd omash
cargo build --locked --release

install -Dm755 target/release/omash "$HOME/.local/bin/omash"
# systemd user unit (autostart is enabled on first TUI run)
systemd_user_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
install -Dm644 systemd/omash-supervisor.service \
  "$systemd_user_dir/omash-supervisor.service"
sed -i 's|^ExecStart=.*|ExecStart=%h/.local/bin/omash --daemon|' \
  "$systemd_user_dir/omash-supervisor.service"
systemctl --user daemon-reload || true

omash
```

No install needed for a quick try:

```bash
cargo run -- --help        # no files written
cargo run                  # auto-creates config and runs TUI
```

### Optional Shell widget

The installer does not add the Omarchy Shell widget. Install it from the public
repository:

```bash
omarchy plugin add https://github.com/ourongxing/omash.git --enable
```

Or install it from a source checkout while in the repository root:

```bash
mkdir -p ~/.config/omarchy/plugins
cp -r integrations/omarchy/ourongxing.omash ~/.config/omarchy/plugins/
omarchy plugin enable ourongxing.omash --section right
```

The widget appears immediately and supports mode changes, proxy selection, and
delay tests. It calls `omash bar` and does not access the Mihomo API secret.

### Update

From `0.1.3` or newer, rerun the installer to update in place:

```bash
curl -fsSL https://raw.githubusercontent.com/ourongxing/omash/main/scripts/install | bash
```

Versions before `0.1.3` used a system-wide `/usr` layout and must be uninstalled
first:

```bash
curl -fsSL https://raw.githubusercontent.com/ourongxing/omash/main/scripts/uninstall | bash
curl -fsSL https://raw.githubusercontent.com/ourongxing/omash/main/scripts/install | bash
```

Configuration, profiles, logs, and backups are preserved. The old-version
uninstaller removes the optional widget, so add it again after updating.

Check Mihomo core updates (GitHub Releases, cached 6h):

```bash
omash update check            # text
omash update check --json     # JSON
omash update check --force    # bypass cache
# Custom repo / token
OMASH_MIHOMO_REPO=owner/repo GITHUB_TOKEN=ghp_xxx omash update check
```

Or in TUI: open `Settings` → `u` check, `U` force, `o` open releases (`Mihomo update (GitHub)` panel).

## Controls

| Key | Action |
| --- | --- |
| `1`-`8` | Open a page (Dashboard/Proxies/Profiles/Conns/Rules/Logs/Settings/Help) |
| `Up` / `Down`, `j` / `k` | Move the selection |
| `Tab`, `Left` / `Right`, `h` / `l` | Switch between proxy groups and nodes |
| `Enter` | Run the selected action (activate profile, select node, toggle setting) |
| `r` | Refresh now |
| `s` (Dashboard) | Start / stop core |
| `m` | Routing mode menu (rule/global/direct) |
| `d` (Proxies) | Test node delay |
| `a` (Profiles) | Import profile (URL or local YAML) |
| `u` (Profiles) | Update selected profile |
| `e` (Profiles) | Update settings (auto-update, interval, timeout, proxy, auth, UA) and rename |
| `D` (Profiles) | Delete profile |
| `x` / `X` (Conns) | Close one / all connections |
| `b` (Settings) | Create backup |
| `R` (Settings) | Restore latest backup (confirm) |
| `u` (Settings) | Check Mihomo update (GitHub, cached) |
| `U` (Settings) | Force check Mihomo update |
| `o` (Settings) | Open releases page (`xdg-open`) |
| DNS in Settings | `6` toggle enable, `7` edit listen, `8` edit servers (Enter to save, hot-patched) |
| `?` | Toggle shortcut help |
| `q`, `Ctrl-C` | Exit the TUI without stopping Mihomo |

Press `?` for the complete shortcut list. Mouse input (click/double-click/wheel) is also supported.

## Remove

Remove the widget, user service, binary, and legacy system files with:

```bash
curl -fsSL https://raw.githubusercontent.com/ourongxing/omash/main/scripts/uninstall | bash
```

The uninstaller stops the daemon (systemd unit + `pkill`) and removes the binary, unit file, and any leftover autostart entry.
It preserves `~/.config/omash` and `~/.local/share/omash`, which
contain your configuration, profiles, logs, and backups. Remove those
directories manually if you also want to delete user data.

## Configuration

Static defaults live in `XDG_CONFIG_HOME/omash/config.toml` (`~/.config/omash/config.toml`);
TUI changes are written to dynamic `XDG_DATA_HOME/omash/config.json` (`~/.local/share/omash/config.json`) and override static on next load.
`--config <path>` loads a different static file (dynamic still at `XDG_DATA_HOME`).

```toml
controller = "http://127.0.0.1:9090"
secret = ""
refresh_ms = 1500
delay_test_url = "https://www.gstatic.com/generate_204"
auto_start = true
mixed_port = 7897
allow_lan = false
ipv6 = true
system_proxy = true
proxy_bypass = "localhost,127.0.0.1,::1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12"
log_level = "info"

[dns]
enable = false
listen = "0.0.0.0:1053"
ipv6 = false
nameserver = ["223.5.5.5", "119.29.29.29"]
fallback = ["tls://8.8.4.4"]
enhanced-mode = "fake-ip"
fake-ip-range = "198.18.0.1/16"
```

`OMASH_REFRESH_MS` and `--refresh-ms` override the configured refresh interval.
`OMASH_MIHOMO`/`OMASH_CORE_BIN` overrides `mihomo` discovery.
`OMASH_MIHOMO_REPO` overrides the GitHub repo for update checks (`owner/repo`).
`GITHUB_TOKEN`/`GH_TOKEN` avoids rate limits.

Runtime data is stored in `~/.local/share/omash/`:

```
~/.local/share/omash/
  profiles/            # imported profiles
  profiles.yaml        # index
  runtime.yaml         # validated, enhanced Mihomo config
  logs/mihomo-YYYY-MM-DD.log
  logs/omash-YYYY-MM-DD.log
  backups/             # manual backups
  config.json          # dynamic overrides
  update-check.json    # GitHub release cache (6h)
  supervisor-state.json
  supervisor.pid
```

### Theme override

The TUI follows the current Omarchy palette by default. To override it:

```bash
mkdir -p ~/.config/omash
cp themes/default.toml ~/.config/omash/theme.toml
```

Changes reload automatically. Remove the file to follow Omarchy again; available
fields are documented in [`themes/default.toml`](themes/default.toml).

## Development

```bash
cargo test
cargo build --locked --release
# isolated HOME test (no install)
TMP_HOME=$(mktemp -d) cargo run -- --help
# update check
cargo run -- update check --json --force
```

## License

As a fork of Clash Verge Rev, `omash` remains licensed under GPL-3.0-only. The
full, unmodified license is retained in [`LICENSE`](LICENSE).
