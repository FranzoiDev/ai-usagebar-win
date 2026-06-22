using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using AiUsageBar.Models;

namespace AiUsageBar.Services;

/// <summary>Turns vendor reports into what the UI shows: the tray wants the worst
/// severity (icon color) + a compact tooltip; the popup and settings windows
/// want structured view-models.</summary>
public static class Renderer
{
    public sealed record Rendered(Severity Severity, string Tooltip);

    /// <summary>Tray icon color + hover tooltip. Lists only vendors with an
    /// identified key/credential (see <see cref="ShouldShow"/>).</summary>
    public static Rendered Render(IReadOnlyList<VendorReport> reports, Config cfg, VendorId primary, DateTimeOffset now)
    {
        int? worst = null;
        foreach (var r in reports)
        {
            if (r.State.Kind != VendorStateKind.Ok || r.State.Snapshot is null) continue;
            var p = r.State.Snapshot.WorstPct();
            if (p is { } v) worst = worst is null ? v : Math.Max(worst.Value, v);
        }
        var severity = worst is { } w ? SeverityRules.ForPct(w) : Severity.Low;

        var tipLines = Ordered(reports, primary)
            .Where(r => ShouldShow(r.State, cfg, r.Id))
            .Select(r => TooltipLine(r, now))
            .ToList();

        var tooltip = tipLines.Count == 0
            ? "ai-usagebar — no models configured"
            : string.Join("\n", tipLines);

        return new Rendered(severity, tooltip);
    }

    private static IEnumerable<VendorReport> Ordered(IReadOnlyList<VendorReport> reports, VendorId primary)
        => reports
            .OrderBy(r => r.Id != primary)
            .ThenBy(r => Array.IndexOf(VendorIdExtensions.All, r.Id));

    /// <summary>Whether a vendor should surface in the popup/tooltip: it has an
    /// identified key/credential. Unconfigured and login-needed vendors are
    /// hidden (they belong in the settings window instead).</summary>
    private static bool ShouldShow(VendorState state, Config cfg, VendorId id) => state.Kind switch
    {
        VendorStateKind.Ok => true,
        VendorStateKind.Error => cfg.IsConfigured(id),
        _ => false,
    };

    private static string TooltipLine(VendorReport r, DateTimeOffset now)
    {
        var tag = r.Id.Short();
        switch (r.State.Kind)
        {
            case VendorStateKind.NeedsLogin: return $"{tag}: login needed";
            case VendorStateKind.Error: return $"{tag}: unavailable";
        }
        return r.State.Snapshot switch
        {
            AnthropicSnapshot s => $"{tag} {s.Session.UtilizationPct}% · {Format.Reset(s.Session.ResetsAt, now)}",
            OpenAiSnapshot s => $"{tag} {s.Session.UtilizationPct}% · {Format.Reset(s.Session.ResetsAt, now)}",
            ZaiSnapshot s => $"{tag} {(s.Session?.UtilizationPct ?? 0)}%",
            OpenRouterSnapshot s => $"{tag} {Money(s.Balance())}",
            DeepseekSnapshot s => $"{tag} {CurrencySym(s.Currency)}{Trim(s.Balance)}",
            _ => $"{tag}: unavailable",
        };
    }

    // -- Popup ---------------------------------------------------------------

    public static PopupModel PopupModel(IReadOnlyList<VendorReport> reports, Config cfg, VendorId primary, DateTimeOffset now)
    {
        var model = new PopupModel();
        foreach (var r in Ordered(reports, primary))
        {
            if (!ShouldShow(r.State, cfg, r.Id)) continue;
            switch (r.State.Kind)
            {
                case VendorStateKind.Ok:
                    model.Vendors.Add(OkCard(r.Id, r.State.Snapshot!, now));
                    break;
                case VendorStateKind.Error:
                    // Configured-but-erroring vendors show so problems are visible.
                    model.Vendors.Add(new VendorCard
                    {
                        Id = r.Id.Slug(),
                        Name = r.Id.Display(),
                        Status = "error",
                        Message = r.State.Message,
                    });
                    break;
            }
        }
        return model;
    }

    private static VendorCard OkCard(VendorId id, VendorSnapshot snap, DateTimeOffset now)
    {
        var bars = new List<Bar>();
        var facts = new List<Fact>();
        string? plan = null;

        switch (snap)
        {
            case AnthropicSnapshot s:
                plan = s.Plan;
                bars.Add(MakeBar("Session (5h)", s.Session, now));
                bars.Add(MakeBar("Weekly", s.Weekly, now));
                if (s.Sonnet is { } sw) bars.Add(MakeBar("Sonnet (weekly)", sw, now));
                if (s.Extra is { } e)
                    facts.Add(new Fact { Label = "Extra usage", Value = $"{e.Fmt()} ({e.Percent}%)" });
                break;

            case OpenAiSnapshot s:
                plan = s.Plan;
                bars.Add(MakeBar("Session (5h)", s.Session, now));
                bars.Add(MakeBar("Weekly", s.Weekly, now));
                if (s.CodeReview is { } cw) bars.Add(MakeBar("Code review", cw, now));
                if (s.Credits is { } c) facts.Add(new Fact { Label = "Credits", Value = c.Balance });
                break;

            case ZaiSnapshot s:
                plan = s.Plan;
                if (s.Session is { } ses) bars.Add(MakeBar("Session (5h)", ses, now));
                if (s.Weekly is { } wk) bars.Add(MakeBar("Weekly", wk, now));
                if (s.Mcp is { } mcp) bars.Add(MakeBar("MCP (monthly)", mcp, now));
                break;

            case OpenRouterSnapshot s:
                plan = s.Label;
                bars.Add(new Bar
                {
                    Label = "Credits used",
                    Pct = s.ConsumedPct(),
                    Level = SeverityRules.Level(s.ConsumedPct()),
                });
                facts.Add(new Fact { Label = "Balance", Value = Money(s.Balance()) });
                facts.Add(new Fact { Label = "Spent today", Value = Money(s.UsageDaily) });
                if (s.Limit is { } lim) facts.Add(new Fact { Label = "Key limit", Value = Money(lim) });
                break;

            case DeepseekSnapshot s:
                facts.Add(new Fact { Label = "Balance", Value = $"{CurrencySym(s.Currency)}{Trim(s.Balance)}" });
                facts.Add(new Fact { Label = "Status", Value = s.IsAvailable ? "available" : "unavailable" });
                break;
        }

        return new VendorCard
        {
            Id = id.Slug(),
            Name = id.Display(),
            Plan = plan,
            Status = "ok",
            Bars = bars,
            Facts = facts,
        };
    }

    private static Bar MakeBar(string label, UsageWindow w, DateTimeOffset now) => new()
    {
        Label = label,
        Pct = w.UtilizationPct,
        Reset = w.ResetsAt is null ? null : Format.Reset(w.ResetsAt, now),
        Level = SeverityRules.Level(w.UtilizationPct),
    };

    // -- Settings ------------------------------------------------------------

    public static SettingsModel SettingsModel(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        var model = new SettingsModel
        {
            PollSeconds = Math.Max(cfg.PollSeconds ?? 60, 15),
            Primary = cfg.Primary().Slug(),
        };
        foreach (var id in VendorIdExtensions.All)
            model.Vendors.Add(VendorSetting(id, cfg, reports));
        return model;
    }

    private static VendorSetting VendorSetting(VendorId id, Config cfg, IReadOnlyList<VendorReport> reports)
    {
        var status = reports.FirstOrDefault(r => r.Id == id) is { } r ? StateLabel(r.State) : null;

        string kind;
        string? env = null, key = null, tier = null, hint = null;
        switch (id)
        {
            case VendorId.Anthropic:
                kind = "oauth";
                hint = "Reads ~/.claude/.credentials.json — sign in with the `claude` CLI.";
                break;
            case VendorId.Openai:
                kind = "oauth";
                hint = "Reads ~/.codex/auth.json — sign in with `codex login`.";
                break;
            case VendorId.Zai:
                kind = "apikey";
                env = cfg.Zai.ApiKeyEnv;
                key = cfg.Zai.ApiKey;
                tier = cfg.Zai.PlanTier;
                break;
            case VendorId.Openrouter:
                kind = "apikey";
                env = cfg.Openrouter.ApiKeyEnv;
                key = cfg.Openrouter.ApiKey;
                break;
            default: // Deepseek
                kind = "apikey";
                env = cfg.Deepseek.ApiKeyEnv;
                key = cfg.Deepseek.ApiKey;
                break;
        }

        return new VendorSetting
        {
            Id = id.Slug(),
            Name = id.Display(),
            Enabled = cfg.IsEnabled(id),
            Configured = cfg.IsConfigured(id),
            Kind = kind,
            ApiKeyEnv = env,
            ApiKey = key,
            PlanTier = tier,
            Hint = hint,
            Status = status,
        };
    }

    private static string StateLabel(VendorState state) => state.Kind switch
    {
        VendorStateKind.Ok => "Connected",
        VendorStateKind.NeedsLogin => $"Login needed — {state.Message}",
        _ => $"Error — {state.Message}",
    };

    // -- Shared helpers ------------------------------------------------------

    private static string Money(double v) => $"${v.ToString("F2", CultureInfo.InvariantCulture)}";
    private static string Trim(double v) => v.ToString("F2", CultureInfo.InvariantCulture);

    private static string CurrencySym(string cur) => cur switch
    {
        "USD" => "$",
        "CNY" => "¥",
        _ => "",
    };
}
