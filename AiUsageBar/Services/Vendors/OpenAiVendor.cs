using System;
using System.Globalization;
using System.Text.Json;
using AiUsageBar.Models;

namespace AiUsageBar.Services.Vendors;

/// <summary>OpenAI Codex — <c>GET chatgpt.com/backend-api/wham/usage</c> with the
/// Codex CLI's access token. Read-only by default; refreshes only when the user
/// opts in (see <see cref="Creds"/>).</summary>
internal static class OpenAiVendor
{
    private const string UsageUrl = "https://chatgpt.com/backend-api/wham/usage";

    public static async Task<VendorState> FetchAsync(Config cfg, DateTimeOffset now)
    {
        var path = cfg.OpenAiAuthPath();
        if (path is null) return VendorState.Error("could not resolve home directory");

        var cred = await Creds.ReadOpenAiAsync(
            path, now.ToUnixTimeSeconds(), cfg.RefreshEnabled(), VendorClient.Client).ConfigureAwait(false);
        switch (cred.Kind)
        {
            case CredKind.Expired:
                return VendorState.NeedsLogin("token expired — run `codex login`");
            case CredKind.Missing:
                return VendorState.NeedsLogin("not logged in — run `codex login`");
            case CredKind.Malformed:
                return VendorState.Error($"bad auth file: {cred.Error}");
        }
        var creds = cred.Value!;

        var fetch = await VendorHttp.GetJsonAsync(UsageUrl, req =>
        {
            req.Headers.TryAddWithoutValidation("Authorization", $"Bearer {creds.AccessToken}");
            req.Headers.TryAddWithoutValidation("User-Agent", "codex-cli");
            if (creds.AccountId is { } aid)
                req.Headers.TryAddWithoutValidation("ChatGPT-Account-Id", aid);
        }, loginHintOn401: "session invalid — run `codex login`").ConfigureAwait(false);

        if (fetch.Failure is { } f) return f;
        using var doc = fetch.Doc!;
        var root = doc.RootElement;

        var planType = root.StrOrNull("plan_type") ?? creds.PlanHint ?? "Unknown";
        var plan = $"ChatGPT {VendorHttp.Capitalize(planType)}";

        var rl = root.Obj("rate_limit");
        var session = WindowOrDefault(rl?.Obj("primary_window"));
        var weekly = WindowOrDefault(rl?.Obj("secondary_window"));

        var codeReviewEl = root.Obj("code_review_rate_limit")?.Obj("primary_window");
        var codeReview = codeReviewEl is null ? null : ToWindow(codeReviewEl.Value);

        OpenAiCredits? credits = null;
        if (root.Obj("credits") is { } c)
            credits = new OpenAiCredits(Money(c, "balance"), c.BoolOr("has_credits"), c.BoolOr("unlimited"));

        return VendorState.Ok(new OpenAiSnapshot(plan, session, weekly, codeReview, credits));
    }

    private static UsageWindow WindowOrDefault(JsonElement? w)
        => w is { } win ? ToWindow(win) : new UsageWindow(0, null);

    private static UsageWindow ToWindow(JsonElement w)
    {
        DateTimeOffset? reset = null;
        if (w.LongOrNull("reset_at") is { } secs)
            reset = DateTimeOffset.FromUnixTimeSeconds(secs);
        else if (w.LongOrNull("reset_after_seconds") is { } after)
            reset = DateTimeOffset.UtcNow.AddSeconds(after);

        var pct = (int)Math.Clamp(w.LongOr("used_percent"), 0, 100);
        return new UsageWindow(pct, reset);
    }

    /// <summary>balance may be a string or a number; mirror the Rust de_money.</summary>
    private static string Money(JsonElement obj, string name)
    {
        if (!obj.TryProp(name, out var v)) return "$0.00";
        return v.ValueKind switch
        {
            JsonValueKind.String => v.GetString() ?? "$0.00",
            JsonValueKind.Number => $"${(v.TryGetDouble(out var d) ? d : 0.0).ToString("F2", CultureInfo.InvariantCulture)}",
            _ => "$0.00",
        };
    }
}
