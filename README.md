# ai-usagebar-win

Windows system-tray app that shows AI plan usage for Anthropic (Claude),
OpenAI (Codex), Z.AI (GLM), OpenRouter, and DeepSeek.

Built with **C# and WinUI 3** (Windows App SDK) on .NET 8. The tray icon uses
[`H.NotifyIcon`](https://github.com/HavenDV/H.NotifyIcon); config is TOML via
[`Tomlyn`](https://github.com/xoofx/Tomlyn) and stays compatible with the Linux
[`ai-usagebar`](https://github.com/akitaonrails/ai-usagebar) Waybar widget. The
popup and settings windows are native XAML.

> **History:** this started as a Rust + raw-Win32 app. Maintaining the
> hand-rolled Win32 UI in Rust turned out to be too much work, so I gave up on
> that stack and migrated the project to a more modern, productive one — C# +
> WinUI 3. The data layer (vendor endpoints, credential parsing, severity model)
> is a faithful port of the original Rust code; see the git history for the
> Rust version.

The app is read-only. It reads the access tokens the `claude` / `codex` CLIs
already wrote to disk and never refreshes or rewrites them, so it cannot log you
out. An expired token shows a "re-login" hint instead of being refreshed.

## UI

- **Hover** the tray icon for a one-line-per-provider tooltip.
- **Click** the tray icon for a popup with a card and progress bars per
  provider. Only providers with an identified key/credential are shown.
- **Settings** (button in the popup) opens a window to enable providers and
  manage API keys — including providers that aren't configured yet. Changes are
  written to `config.toml` and applied without a restart.
- **Quit** (button in the popup) exits the whole process.

The icon color tracks worst-case usage: green <50%, yellow >=50%, orange >=75%,
red >=90%.

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

Requires:

- **.NET 8 SDK**
- **Windows 10 2004 (19041) or later** — WinUI 3 is Windows-only.
- **Visual Studio 2022** with the *.NET Desktop Development* and *Windows App
  SDK / WinUI* components (or the `Microsoft.WindowsAppSDK` NuGet alone for CLI
  builds).

```powershell
# from the repo root
dotnet restore AiUsageBar.sln
dotnet build  AiUsageBar.sln -c Release -p:Platform=x64
dotnet test   AiUsageBar.Tests/AiUsageBar.Tests.csproj -c Release -p:Platform=x64

# run
dotnet run --project AiUsageBar/AiUsageBar.csproj -p:Platform=x64
```

Or open `AiUsageBar.sln` in Visual Studio, set the platform to **x64**, and
press F5.

## Deploy

WinUI 3 cannot ship as a single static `.exe` like the old Rust build — it
depends on the Windows App SDK runtime. This project is configured **unpackaged
and self-contained** (`WindowsPackageType=None`, `WindowsAppSDKSelfContained=true`),
so the published output is a *folder* that runs without any separate runtime
install:

```powershell
dotnet publish AiUsageBar/AiUsageBar.csproj -c Release -p:Platform=x64 -r win-x64 --self-contained
# -> AiUsageBar/bin/x64/Release/net8.0-windows10.0.19041.0/win-x64/publish/
# run ai-usagebar-win.exe from that folder
```

To start it with Windows, put a shortcut to `ai-usagebar-win.exe` in
`shell:startup` (`Win+R` -> `shell:startup`).

## Layout

| Path | Purpose |
|---|---|
| `Models/Usage.cs` | snapshot model, severity tiers, countdown formatting |
| `Models/VendorId.cs` | provider ids + display/slug helpers |
| `Models/VendorReport.cs` | per-vendor poll result (Ok / NeedsLogin / Error) |
| `Models/ViewModels.cs` | popup + settings view-models bound by XAML |
| `Services/Config.cs` | TOML config load/save, API-key and path resolution |
| `Services/Creds.cs` | read-only Claude/Codex credential readers (no refresh) |
| `Services/Vendors/` | one file per provider: endpoint, parse, snapshot |
| `Services/Renderer.cs` | reports -> tooltip + popup/settings view-models |
| `Services/TrayIconFactory.cs` | severity-tinted tray icon drawn in code |
| `Services/TrayService.cs` | H.NotifyIcon wrapper |
| `Services/Poller.cs` | background polling loop with on-demand refresh |
| `Views/PopupWindow.xaml` | frameless popup anchored near the tray |
| `Views/SettingsWindow.xaml` | provider enable + API-key form |
| `App.xaml.cs` | tray-first app wiring (no main window) |

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
