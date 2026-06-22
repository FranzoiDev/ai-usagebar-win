using System;
using System.Globalization;
using System.Text.Json;
using AiUsageBar.Models;

namespace AiUsageBar.Services.Vendors;

/// <summary>Anthropic Claude — <c>GET /api/oauth/usage</c> with the CLI's access
/// token. Read-only: we never refresh the token (see <see cref="Creds"/>).</summary>
internal static class AnthropicVendor
{
    private const string UsageUrl = "https://api.anthropic.com/api/oauth/usage";
    private const string BetaHeader = "oauth-2025-04-20";

    public static async Task<VendorState> FetchAsync(Config cfg, DateTimeOffset now)
    {
        var path = cfg.AnthropicCredsPath();
        if (path is null) return VendorState.Error("could not resolve home directory");

        var cred = Creds.ReadAnthropic(path, now.ToUnixTimeSeconds());
        switch (cred.Kind)
        {
            case CredKind.Expired:
                return VendorState.NeedsLogin("token expired — run `claude` to re-login");
            case CredKind.Missing:
                return VendorState.NeedsLogin("not logged in — run `claude`");
            case CredKind.Malformed:
                return VendorState.Error($"bad credentials file: {cred.Error}");
        }
        var creds = cred.Value!;

        var fetch = await VendorHttp.GetJsonAsync(UsageUrl, req =>
        {
            req.Headers.TryAddWithoutValidation("Authorization", $"Bearer {creds.AccessToken}");
            req.Headers.TryAddWithoutValidation("anthropic-beta", BetaHeader);
            req.Headers.TryAddWithoutValidation("User-Agent", VendorHttp.DefaultUserAgent);
        }, loginHintOn401: "session invalid — run `claude` to re-login").ConfigureAwait(false);

        if (fetch.Failure is { } f) return f;
        using var doc = fetch.Doc!;
        var root = doc.RootElement;

        var session = ToWindow(root.Obj("five_hour"));
        var weekly = ToWindow(root.Obj("seven_day"));
        var sonnetEl = root.Obj("seven_day_sonnet");
        var sonnet = sonnetEl is null ? null : ToWindow(sonnetEl);

        ExtraUsage? extra = null;
        if (root.Obj("extra_usage") is { } ex && ex.BoolOr("is_enabled"))
            extra = new ExtraUsage(ex.LongOr("monthly_limit"), ex.LongOr("used_credits"));

        return VendorState.Ok(new AnthropicSnapshot(creds.PlanLabel, session, weekly, sonnet, extra));
    }

    private static UsageWindow ToWindow(JsonElement? w)
    {
        if (w is not { } win) return new UsageWindow(0, null);
        var pct = VendorHttp.RoundClamp(win.DblOr("utilization"), 0, int.MaxValue);
        DateTimeOffset? reset = null;
        if (win.StrOrNull("resets_at") is { } s
            && DateTimeOffset.TryParse(s, CultureInfo.InvariantCulture,
                DateTimeStyles.RoundtripKind, out var dto))
            reset = dto.ToUniversalTime();
        return new UsageWindow(pct, reset);
    }
}
