using System;

namespace AiUsageBar.Models;

/// <summary>A single usage window: percent used (0..=100) + when it resets.</summary>
public sealed record UsageWindow(int UtilizationPct, DateTimeOffset? ResetsAt);

/// <summary>"Extra usage" pay-as-you-go block. Money in integer cents.</summary>
public readonly record struct ExtraUsage(long LimitCents, long SpentCents)
{
    public int Percent => LimitCents <= 0 ? 0 : (int)(SpentCents * 100 / LimitCents);

    public string Fmt() => $"{FmtCents(SpentCents)} / {FmtCents(LimitCents)}";

    private static string FmtCents(long c)
    {
        var sign = c < 0 ? "-" : "";
        var abs = Math.Abs(c);
        return $"{sign}${abs / 100}.{abs % 100:D2}";
    }
}

/// <summary>Discriminated base for the per-vendor snapshots. Each provider keeps
/// its own shape because they expose genuinely different data.</summary>
public abstract record VendorSnapshot
{
    /// <summary>Worst-case utilization across the snapshot's windows, used to
    /// drive the tray icon color. <c>null</c> for balance-only vendors that
    /// don't express a meaningful "percent of plan used".</summary>
    public abstract int? WorstPct();
}

/// <summary>Anthropic Claude — three rolling windows + optional credits.</summary>
public sealed record AnthropicSnapshot(
    string Plan,
    UsageWindow Session,
    UsageWindow Weekly,
    UsageWindow? Sonnet,
    ExtraUsage? Extra) : VendorSnapshot
{
    public override int? WorstPct()
    {
        var m = Math.Max(Session.UtilizationPct, Weekly.UtilizationPct);
        if (Sonnet is not null) m = Math.Max(m, Sonnet.UtilizationPct);
        return m;
    }
}

public sealed record OpenAiCredits(string Balance, bool HasCredits, bool Unlimited);

/// <summary>OpenAI Codex OAuth — two windows + optional code-review bucket.</summary>
public sealed record OpenAiSnapshot(
    string Plan,
    UsageWindow Session,
    UsageWindow Weekly,
    UsageWindow? CodeReview,
    OpenAiCredits? Credits) : VendorSnapshot
{
    public override int? WorstPct() => Math.Max(Session.UtilizationPct, Weekly.UtilizationPct);
}

/// <summary>Z.AI / BigModel — session/weekly token buckets + monthly MCP ceiling.</summary>
public sealed record ZaiSnapshot(
    string Plan,
    UsageWindow? Session,
    UsageWindow? Weekly,
    UsageWindow? Mcp) : VendorSnapshot
{
    public override int? WorstPct()
    {
        int? max = null;
        foreach (var w in new[] { Session, Weekly, Mcp })
        {
            if (w is null) continue;
            max = max is null ? w.UtilizationPct : Math.Max(max.Value, w.UtilizationPct);
        }
        return max;
    }
}

/// <summary>OpenRouter — credit balance + daily/weekly/monthly spend.</summary>
public sealed record OpenRouterSnapshot(
    string Label,
    double TotalCredits,
    double TotalUsage,
    double UsageDaily,
    double UsageWeekly,
    double UsageMonthly,
    bool IsFreeTier,
    double? Limit,
    double? LimitRemaining) : VendorSnapshot
{
    public double Balance() => Math.Max(0.0, TotalCredits - TotalUsage);

    public int ConsumedPct()
    {
        if (TotalCredits <= 0.0) return 0;
        return (int)Math.Clamp(Math.Round(TotalUsage / TotalCredits * 100.0), 0.0, 100.0);
    }

    public override int? WorstPct() => ConsumedPct();
}

/// <summary>DeepSeek — credit balance from <c>/user/balance</c>.</summary>
public sealed record DeepseekSnapshot(
    bool IsAvailable,
    double Balance,
    double Granted,
    double ToppedUp,
    string Currency) : VendorSnapshot
{
    public override int? WorstPct() => null;
}

/// <summary>Severity tiers (mirror the Linux widget's thresholds).</summary>
public enum Severity { Low, Mid, High, Critical }

public static class SeverityRules
{
    public static Severity ForPct(int pct) => pct switch
    {
        >= 90 => Severity.Critical,
        >= 75 => Severity.High,
        >= 50 => Severity.Mid,
        _ => Severity.Low,
    };

    public static string Level(int pct) => ForPct(pct) switch
    {
        Severity.Mid => "mid",
        Severity.High => "high",
        Severity.Critical => "critical",
        _ => "low",
    };
}

public static class Format
{
    /// <summary>Compact human countdown: "2h 13m", "3d 4h", "now", or "—".</summary>
    public static string Reset(DateTimeOffset? reset, DateTimeOffset now)
    {
        if (reset is not { } r) return "—";
        var secs = (long)(r - now).TotalSeconds;
        if (secs <= 0) return "now";
        var days = secs / 86_400;
        var hours = secs % 86_400 / 3_600;
        var mins = secs % 3_600 / 60;
        if (days > 0) return $"{days}d {hours}h";
        if (hours > 0) return $"{hours}h {mins}m";
        return $"{mins}m";
    }
}
