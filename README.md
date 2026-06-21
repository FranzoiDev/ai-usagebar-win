# ai-usagebar-win

A **100% native Windows** notification-area (system tray) app that shows how
much of your AI plans you've used — Anthropic **Claude**, OpenAI **Codex**,
**Z.AI (GLM)**, **OpenRouter**, and **DeepSeek** — right next to the clock,
Wi-Fi and volume icons.

It's a from-scratch Windows frontend that reverse-engineers the data layer of
the Linux [`ai-usagebar`](https://github.com/akitaonrails/ai-usagebar) Waybar
widget. No Electron, no .NET runtime — a single `~3 MB` `.exe` built on
[`tray-icon`](https://crates.io/crates/tray-icon) + [`tao`](https://crates.io/crates/tao)
talking directly to the Win32 `Shell_NotifyIcon` API.

## Read-only by design — it will not log you out

The one hard rule: **this app never refreshes your OAuth tokens and never
writes to your credential files.** The official `claude` / `codex` CLIs own
those tokens; refreshing them here would rotate the refresh-token out from under
the CLI and risk logging you out of your tools.

- It only **reads** the access token already on disk.
- If a token has already **expired**, the tray shows a *"run `claude` /
  `codex login` to re-login"* hint instead of refreshing.
- API-key vendors (Z.AI, OpenRouter, DeepSeek) never had this problem — keys
  don't expire.

## What you see

- **Tray icon** tinted by your worst-case usage: green → amber → orange → red
  (≥50% / ≥75% / ≥90%).
- **Hover tooltip** with a compact per-vendor summary, e.g.
  `cld 29% · 1h12m` / `gpt 4% · 3d` / `or $74.50`.
- **Right-click menu** with the full breakdown (session / weekly / Sonnet /
  code-review windows, extra-usage, credit balances), plus **Refresh now** and
  **Quit**.

## Authentication

| Vendor | Method | What to do |
|---|---|---|
| Anthropic | OAuth read from `%USERPROFILE%\.claude\.credentials.json` | Run `claude` once. |
| OpenAI | OAuth read from `%USERPROFILE%\.codex\auth.json` | Run `codex login` once. |
| Z.AI | API key (`ZAI_API_KEY` env or `[zai] api_key`) | Set either. |
| OpenRouter | API key (`OPENROUTER_API_KEY` env or `[openrouter] api_key`) | Set either. |
| DeepSeek | API key (`DEEPSEEK_API_KEY` env or `[deepseek] api_key`) | Set either; opt-in. |

If a vendor's credentials are absent the app simply shows "login needed" for
that vendor and keeps showing the others.

## Build & run

Needs a stable Rust toolchain. **Build on Windows** (or cross-compile to
`x86_64-pc-windows-msvc`):

```powershell
cargo build --release
.\target\release\ai-usagebar-win.exe
```

The release build sets `windows_subsystem = "windows"`, so there's no console
window — it just appears in the tray. To launch at login, drop a shortcut to
the `.exe` in `shell:startup`.

> The project also builds and `cargo test`s on macOS/Linux for development
> (tray-icon is cross-platform), but it is intended for Windows.

## Configuration

Optional. Copy [`config.example.toml`](config.example.toml) to
`%APPDATA%\ai-usagebar\config.toml`. It's wire-compatible with the Linux
`ai-usagebar` config, so an existing file works as-is. See the example for all
keys (poll interval, primary vendor, per-vendor enable + key sources).

## How it works

```
                ┌──────────────────────────── poll thread ───────────────┐
%USERPROFILE%\… │ read creds (RO) → GET usage endpoints (blocking reqwest)│
  env / config  │ → parse into VendorSnapshot                             │
                └───────────────┬─────────────────────────────────────────┘
                                │ EventLoopProxy::send_event(Update)
                ┌───────────────▼──────────── UI thread (tao) ────────────┐
                │ render → set tray icon color + tooltip + context menu    │
                └──────────────────────────────────────────────────────────┘
```

- `src/usage.rs` — canonical snapshot model + severity + countdown formatting.
- `src/creds.rs` — read-only Claude/Codex credential readers (expiry check, **no refresh**).
- `src/config.rs` — config + API-key/credential-path resolution.
- `src/vendors/` — one module per provider: endpoint + wire types + parse.
- `src/render.rs` — snapshot → tooltip + menu lines + icon severity.
- `src/tray.rs` — in-code RGBA icon generation (no asset files).
- `src/main.rs` — tao event loop + background poll thread.

Endpoints reverse-engineered (all undocumented, may drift):

| Vendor | Endpoint |
|---|---|
| Anthropic | `GET https://api.anthropic.com/api/oauth/usage` (`anthropic-beta: oauth-2025-04-20`) |
| OpenAI | `GET https://chatgpt.com/backend-api/wham/usage` |
| Z.AI | `GET https://api.z.ai/api/monitor/usage/quota/limit` (key in `Authorization`, **no** `Bearer`) |
| OpenRouter | `GET https://openrouter.ai/api/v1/credits` + `/key` |
| DeepSeek | `GET https://api.deepseek.com/user/balance` |

## Status / roadmap

MVP works: all five vendors, tray icon + tooltip + menu, read-only auth.

Possible next steps:
- Render the actual percentage *number* into the tray icon (font rasterization).
- "Open config" / "Re-login" menu shortcuts.
- Per-vendor icon cycling like the Linux scroll-to-cycle.
