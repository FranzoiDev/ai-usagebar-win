using System;
using System.Linq;
using System.Text.Json;
using AiUsageBar.Models;

namespace AiUsageBar.Services.Vendors;

/// <summary>Z.AI / BigModel — <c>GET /api/monitor/usage/quota/limit</c>.
/// Auth quirk: API key in <c>Authorization</c> WITHOUT the <c>Bearer </c> prefix.</summary>
internal static class ZaiVendor
{
    private const string QuotaUrl = "https://api.z.ai/api/monitor/usage/quota/limit";

    public static async Task<VendorState> FetchAsync(Config cfg)
    {
        var key = Config.ResolveApiKey(cfg.Zai.ApiKeyEnv, cfg.Zai.ApiKey);
        if (key is null)
            return VendorState.NeedsLogin($"no API key — set {cfg.Zai.ApiKeyEnv} or [zai] api_key");

        var fetch = await VendorHttp.GetJsonAsync(QuotaUrl, req =>
        {
            req.Headers.TryAddWithoutValidation("Authorization", key); // NO "Bearer " prefix.
            req.Headers.TryAddWithoutValidation("Accept-Language", "en-US,en");
            req.Headers.TryAddWithoutValidation("Content-Type", "application/json");
            req.Headers.TryAddWithoutValidation("User-Agent", VendorHttp.DefaultUserAgent);
        }, loginHintOn401: "API key rejected (401)").ConfigureAwait(false);

        if (fetch.Failure is { } f) return f;
        using var doc = fetch.Doc!;
        var data = doc.RootElement.Obj("data");

        var limits = new List<JsonElement>();
        if (data?.Obj("limits") is { ValueKind: JsonValueKind.Array } arr)
            limits.AddRange(arr.EnumerateArray());

        var tokens = limits.Where(l => l.StrOr("type", "") == "TOKENS_LIMIT").ToList();
        var session = tokens.Count > 0 ? ToWindow(tokens[0]) : null;
        var weekly = tokens.Count > 1 ? ToWindow(tokens[1]) : null;
        var mcpEl = limits.Where(l => l.StrOr("type", "") == "TIME_LIMIT").ToList();
        var mcp = mcpEl.Count > 0 ? ToWindow(mcpEl[0]) : null;

        var level = data?.StrOrNull("level");
        if (string.IsNullOrEmpty(level)) level = cfg.Zai.PlanTier ?? "unknown";

        var plan = $"GLM Coding {VendorHttp.Capitalize(level)}";
        return VendorState.Ok(new ZaiSnapshot(plan, session, weekly, mcp));
    }

    private static UsageWindow ToWindow(JsonElement l)
    {
        DateTimeOffset? reset = null;
        if (l.LongOrNull("nextResetTime") is { } ms)
            reset = DateTimeOffset.FromUnixTimeMilliseconds(ms);
        return new UsageWindow(VendorHttp.RoundClamp(l.DblOr("percentage")), reset);
    }
}
