using System.Text.Json;
using AiUsageBar.Models;

namespace AiUsageBar.Services.Vendors;

/// <summary>OpenRouter — combines <c>/api/v1/credits</c> and <c>/api/v1/key</c>.</summary>
internal static class OpenRouterVendor
{
    private const string CreditsUrl = "https://openrouter.ai/api/v1/credits";
    private const string KeyUrl = "https://openrouter.ai/api/v1/key";

    public static async Task<VendorState> FetchAsync(Config cfg)
    {
        var key = Config.ResolveApiKey(cfg.Openrouter.ApiKeyEnv, cfg.Openrouter.ApiKey);
        if (key is null)
            return VendorState.NeedsLogin($"no API key — set {cfg.Openrouter.ApiKeyEnv} or [openrouter] api_key");

        var creditsFetch = await Get(CreditsUrl, key).ConfigureAwait(false);
        if (creditsFetch.Failure is { } cf) return cf;
        using var creditsDoc = creditsFetch.Doc!;
        var credits = creditsDoc.RootElement.Obj("data");

        var keyFetch = await Get(KeyUrl, key).ConfigureAwait(false);
        if (keyFetch.Failure is { } kf) return kf;
        using var keyDoc = keyFetch.Doc!;
        var keyData = keyDoc.RootElement.Obj("data");

        var label = keyData?.StrOrNull("label");
        var displayLabel = string.IsNullOrEmpty(label) ? "OpenRouter" : $"OpenRouter — {label}";

        var snap = new OpenRouterSnapshot(
            Label: displayLabel,
            TotalCredits: credits?.DblOr("total_credits") ?? 0,
            TotalUsage: credits?.DblOr("total_usage") ?? 0,
            UsageDaily: keyData?.DblOr("usage_daily") ?? 0,
            UsageWeekly: keyData?.DblOr("usage_weekly") ?? 0,
            UsageMonthly: keyData?.DblOr("usage_monthly") ?? 0,
            IsFreeTier: keyData?.BoolOr("is_free_tier") ?? false,
            Limit: NullableDbl(keyData, "limit"),
            LimitRemaining: NullableDbl(keyData, "limit_remaining"));

        return VendorState.Ok(snap);
    }

    private static Task<VendorHttp.Fetch> Get(string url, string key)
        => VendorHttp.GetJsonAsync(url, req =>
        {
            req.Headers.TryAddWithoutValidation("Authorization", $"Bearer {key}");
            req.Headers.TryAddWithoutValidation("User-Agent", VendorHttp.DefaultUserAgent);
        }, loginHintOn401: "API key rejected (401)");

    private static double? NullableDbl(JsonElement? obj, string name)
    {
        if (obj is not { } o || !o.TryProp(name, out var v)) return null;
        if (v.ValueKind == JsonValueKind.Number && v.TryGetDouble(out var d)) return d;
        return null;
    }
}
