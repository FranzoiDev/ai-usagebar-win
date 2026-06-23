using System;
using System.IO;
using AiUsageBar.Models;
using Tomlyn;

namespace AiUsageBar.Services;

/// <summary>
/// Config at <c>%APPDATA%\ai-usagebar\config.toml</c>.
///
/// Mirrors the Linux crate's layout so an existing file stays compatible.
/// Missing file = defaults. API keys resolve env-var-first, then inline config.
/// OAuth-credential paths default to the Windows user profile.
///
/// Tomlyn maps PascalCase properties to snake_case keys by default, which lines
/// up with the schema (PollSeconds -&gt; poll_seconds, ApiKeyEnv -&gt; api_key_env).
/// </summary>
public sealed class Config
{
    // NOTE: bare values must serialize before the [table] sections — TOML
    // requires them to precede tables at the same level. Keep these two first.
    public long? PollSeconds { get; set; }

    /// <summary>Opt-in: let the app refresh the Claude/Codex OAuth tokens (and
    /// write them back to the CLI credential files) when they near expiry.
    /// Off/absent = strictly read-only, the historical default. See
    /// <see cref="RefreshEnabled"/>.</summary>
    public bool? RefreshTokens { get; set; }

    public UiConfig Ui { get; set; } = new();
    public AnthropicConfig Anthropic { get; set; } = new();
    public OpenAiConfig Openai { get; set; } = new();
    public ZaiConfig Zai { get; set; } = new();
    public OpenRouterConfig Openrouter { get; set; } = new();
    public DeepseekConfig Deepseek { get; set; } = new();

    private static readonly TomlModelOptions TomlOptions = new()
    {
        IgnoreMissingProperties = true,
    };

    public static Config Load()
    {
        var path = DefaultPath();
        return path is null ? new Config() : LoadFrom(path);
    }

    public static Config LoadFrom(string path)
    {
        try
        {
            var text = File.ReadAllText(path);
            return Toml.ToModel<Config>(text, options: TomlOptions);
        }
        catch
        {
            return new Config();
        }
    }

    public bool IsEnabled(VendorId id) => id switch
    {
        VendorId.Anthropic => Anthropic.Enabled,
        VendorId.Openai => Openai.Enabled,
        VendorId.Zai => Zai.Enabled,
        VendorId.Openrouter => Openrouter.Enabled,
        VendorId.Deepseek => Deepseek.Enabled,
        _ => false,
    };

    public IReadOnlyList<VendorId> EnabledVendors()
    {
        var list = new List<VendorId>();
        foreach (var id in VendorIdExtensions.All)
            if (IsEnabled(id)) list.Add(id);
        return list;
    }

    /// <summary>True when a usable credential/API key is present — drives whether
    /// a vendor shows in the popup (configured) vs. only in settings.</summary>
    public bool IsConfigured(VendorId id) => id switch
    {
        VendorId.Anthropic => FileExists(AnthropicCredsPath()),
        VendorId.Openai => FileExists(OpenAiAuthPath()),
        VendorId.Zai => ResolveApiKey(Zai.ApiKeyEnv, Zai.ApiKey) is not null,
        VendorId.Openrouter => ResolveApiKey(Openrouter.ApiKeyEnv, Openrouter.ApiKey) is not null,
        VendorId.Deepseek => ResolveApiKey(Deepseek.ApiKeyEnv, Deepseek.ApiKey) is not null,
        _ => false,
    };

    public TimeSpan PollInterval() => TimeSpan.FromSeconds(Math.Max(PollSeconds ?? 60, 15));

    /// <summary>True when the user opted into OAuth token refresh. A method (not a
    /// property) so Tomlyn doesn't try to serialize it as a config key.</summary>
    public bool RefreshEnabled() => RefreshTokens == true;

    /// <summary>Normalize a config built from the settings form: drop blank inline
    /// keys (so we never persist <c>api_key = ""</c>) and floor the poll interval.</summary>
    public Config Sanitized()
    {
        Zai.ApiKey = BlankToNull(Zai.ApiKey);
        Openrouter.ApiKey = BlankToNull(Openrouter.ApiKey);
        Deepseek.ApiKey = BlankToNull(Deepseek.ApiKey);
        Zai.PlanTier = BlankToNull(Zai.PlanTier);
        if (PollSeconds is { } p) PollSeconds = Math.Max(p, 15);
        return this;
    }

    /// <summary>Persist to <c>%APPDATA%\ai-usagebar\config.toml</c>, creating dirs.</summary>
    public void Save()
    {
        var path = DefaultPath() ?? throw new InvalidOperationException("could not resolve config directory");
        var dir = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
        File.WriteAllText(path, Toml.FromModel(this, TomlOptions));
    }

    // -- path / key resolution ------------------------------------------------

    /// <summary>Resolve an API key: env var wins, then inline config, else null.</summary>
    public static string? ResolveApiKey(string? envVarName, string? inline)
    {
        if (!string.IsNullOrEmpty(envVarName))
        {
            var v = Environment.GetEnvironmentVariable(envVarName);
            if (!string.IsNullOrEmpty(v)) return v;
        }
        return string.IsNullOrEmpty(inline) ? null : inline;
    }

    public static string? DefaultPath()
    {
        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        return string.IsNullOrEmpty(appData) ? null : Path.Combine(appData, "ai-usagebar", "config.toml");
    }

    public static string? HomeDir()
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        return string.IsNullOrEmpty(home) ? null : home;
    }

    /// <summary>Default Anthropic creds: <c>%USERPROFILE%\.claude\.credentials.json</c>.</summary>
    public string? AnthropicCredsPath()
    {
        if (!string.IsNullOrEmpty(Anthropic.CredentialsPath)) return Anthropic.CredentialsPath;
        var home = HomeDir();
        return home is null ? null : Path.Combine(home, ".claude", ".credentials.json");
    }

    /// <summary>Default OpenAI Codex auth: <c>%USERPROFILE%\.codex\auth.json</c>.</summary>
    public string? OpenAiAuthPath()
    {
        if (!string.IsNullOrEmpty(Openai.CodexAuthPath)) return Openai.CodexAuthPath;
        var home = HomeDir();
        return home is null ? null : Path.Combine(home, ".codex", "auth.json");
    }

    public VendorId Primary() => VendorIdExtensions.FromSlug(Ui.Primary);

    private static bool FileExists(string? path) => path is not null && File.Exists(path);

    private static string? BlankToNull(string? v) => string.IsNullOrWhiteSpace(v) ? null : v;
}

public sealed class UiConfig
{
    /// <summary>Which vendor leads the tray tooltip. Slug; null defaults to anthropic.</summary>
    public string? Primary { get; set; }
}

public sealed class AnthropicConfig
{
    public bool Enabled { get; set; } = true;
    public string? CredentialsPath { get; set; }
}

public sealed class OpenAiConfig
{
    public bool Enabled { get; set; } = true;
    public string? CodexAuthPath { get; set; }
}

public sealed class ZaiConfig
{
    public bool Enabled { get; set; } = true;
    public string ApiKeyEnv { get; set; } = "ZAI_API_KEY";
    public string? ApiKey { get; set; }
    public string? PlanTier { get; set; }
}

public sealed class OpenRouterConfig
{
    public bool Enabled { get; set; } = true;
    public string ApiKeyEnv { get; set; } = "OPENROUTER_API_KEY";
    public string? ApiKey { get; set; }
}

public sealed class DeepseekConfig
{
    public bool Enabled { get; set; } = false;
    public string ApiKeyEnv { get; set; } = "DEEPSEEK_API_KEY";
    public string? ApiKey { get; set; }
}
