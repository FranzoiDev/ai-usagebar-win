# ai-usagebar-win

Windows system-tray app that shows AI plan usage for Anthropic (Claude),
OpenAI (Codex), Z.AI (GLM), OpenRouter, and DeepSeek.

Single `.exe`, no installer, no runtime. Written in Rust with
[`tray-icon`](https://crates.io/crates/tray-icon) and
[`tao`](https://crates.io/crates/tao) over the Win32 `Shell_NotifyIcon` API.
The popup and settings windows are 100% native: raw Win32 controls (real
progress bars, owner-drawn buttons) via
[`windows-sys`](https://crates.io/crates/windows-sys), with DWM dark mode and
rounded corners — no web engine. The data layer is a Windows port of the Linux
[`ai-usagebar`](https://github.com/akitaonrails/ai-usagebar) Waybar widget.

The app is read-only. It reads the access tokens the `claude` / `codex` CLIs
already wrote to disk and never refreshes or rewrites them, so it cannot log
you out. An expired token shows a "re-login" hint instead of being refreshed.

## UI

- **Hover** the tray icon for a one-line-per-provider tooltip.
- **Click** the tray icon for a popup with a card and progress bars per
  provider. Only providers with an identified key/credential are shown.
- **Settings** (button in the popup) opens a regular OS window to enable
  providers and manage API keys — including providers that aren't configured
  yet. Changes are written to `config.toml` and applied without a restart.
- **Quit** (button in the popup) exits the whole process.

The icon color tracks worst-case usage: green <50%, yellow >=50%, orange >=75%,
red >=90%.

## Screenshots

Popup (progress bars):

<!-- screenshots/popup.png -->
![popup](screenshots/popup.png)

Settings window:

<!-- screenshots/settings.png -->
![settings](screenshots/settings.png)

## Install

Download `ai-usagebar-win.exe` from
[Releases](https://github.com/FranzoiDev/ai-usagebar-win/releases) and run it.

The binary is unsigned, so SmartScreen may warn on first run: "More info" ->
"Run anyway". To start it with Windows, put a shortcut to the `.exe` in
`shell:startup` (`Win+R` -> `shell:startup`).

## Providers

| Provider | Source | Setup |
|---|---|---|
| Anthropic | `%USERPROFILE%\.claude\.credentials.json` | run `claude` once |
| OpenAI | `%USERPROFILE%\.codex\auth.json` | run `codex login` once |
| Z.AI | `ZAI_API_KEY` or `[zai] api_key` | set the key |
| OpenRouter | `OPENROUTER_API_KEY` or `[openrouter] api_key` | set the key |
| DeepSeek | `DEEPSEEK_API_KEY` or `[deepseek] api_key` | set the key, enable in config |

Set API keys via environment variable:

```powershell
setx ZAI_API_KEY "your-key"
setx OPENROUTER_API_KEY "your-key"
```

`setx` only affects new terminals. A provider that isn't configured shows
"login needed" and the others keep working.

## Config

Optional. Copy `config.example.toml` to `%APPDATA%\ai-usagebar\config.toml`.

- `poll_seconds`: refresh interval, default 60, minimum 15.
- `[ui] primary`: provider shown first in the tooltip/popup.
- per-provider `enabled` and inline `api_key`.

Same format as the Linux `ai-usagebar`, so an existing config works. The
Settings window edits this same file, so hand-edits and the UI stay in sync.

## Build

Requires Rust ([rustup.rs](https://rustup.rs), MSVC toolchain).

```powershell
cargo build --release
.\target\release\ai-usagebar-win.exe
```

Release builds have no console window. Use `cargo run` during development for
log output, and `cargo test` to run the suite.

## Layout

| File | Purpose |
|---|---|
| `src/usage.rs` | snapshot model, severity tiers, countdown formatting |
| `src/creds.rs` | read-only Claude/Codex credential readers (no refresh) |
| `src/config.rs` | config loading, API-key and path resolution |
| `src/vendors/` | one module per provider: endpoint, types, parse |
| `src/render.rs` | snapshot -> tooltip + popup/settings view-models, icon severity |
| `src/winui_win.rs` | native Win32 popup + settings windows (Windows only) |
| `src/winui_stub.rs` | no-op UI shims so the crate builds/tests off-Windows |
| `src/tray.rs` | RGBA tray icon generated in code |
| `src/main.rs` | tao event loop, tray icon, background poll thread |

A background thread polls each provider on an interval and sends results to the
UI thread, which owns the tray icon and the native windows. The Win32 window
procedures handle clicks directly (refresh / open settings / save / quit).

## Endpoints

Undocumented, reverse-engineered, may change.

| Provider | Endpoint |
|---|---|
| Anthropic | `GET api.anthropic.com/api/oauth/usage` (header `anthropic-beta: oauth-2025-04-20`) |
| OpenAI | `GET chatgpt.com/backend-api/wham/usage` |
| Z.AI | `GET api.z.ai/api/monitor/usage/quota/limit` (key in `Authorization`, no `Bearer`) |
| OpenRouter | `GET openrouter.ai/api/v1/credits` and `/key` |
| DeepSeek | `GET api.deepseek.com/user/balance` |

## License

MIT. Data layer reverse-engineered from
[akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar).
