using System.Globalization;
using System.Linq;
using System.Text.Json;
using AiUsageBar.Models;

namespace AiUsageBar.Services.Vendors;

/// <summary>DeepSeek — <c>GET /user/balance</c>.</summary>
internal static class DeepseekVendor
{
    private const string BalanceUrl = "https://api.deepseek.com/user/balance";

    public static async Task<VendorState> FetchAsync(Config cfg)
    {
        var key = Config.ResolveApiKey(cfg.Deepseek.ApiKeyEnv, cfg.Deepseek.ApiKey);
        if (key is null)
            return VendorState.NeedsLogin($"no API key — set {cfg.Deepseek.ApiKeyEnv} or [deepseek] api_key");

        var fetch = await VendorHttp.GetJsonAsync(BalanceUrl, req =>
        {
            req.Headers.TryAddWithoutValidation("Authorization", $"Bearer {key}");
            req.Headers.TryAddWithoutValidation("Accept", "application/json");
            req.Headers.TryAddWithoutValidation("User-Agent", VendorHttp.DefaultUserAgent);
        }, loginHintOn401: "API key rejected (401)").ConfigureAwait(false);

        if (fetch.Failure is { } f) return f;
        using var doc = fetch.Doc!;
        var root = doc.RootElement;

        var infos = new List<JsonElement>();
        if (root.Obj("balance_infos") is { ValueKind: JsonValueKind.Array } arr)
            infos.AddRange(arr.EnumerateArray());

        JsonElement? info =
            infos.FirstOrDefault(b => b.StrOr("currency", "") == "USD") is { ValueKind: JsonValueKind.Object } usd ? usd
            : infos.FirstOrDefault(b => b.StrOr("currency", "") == "CNY") is { ValueKind: JsonValueKind.Object } cny ? cny
            : infos.Count > 0 ? infos[0]
            : null;

        var snap = new DeepseekSnapshot(
            IsAvailable: root.BoolOr("is_available"),
            Balance: ParseF64(info?.StrOrNull("total_balance")),
            Granted: ParseF64(info?.StrOrNull("granted_balance")),
            ToppedUp: ParseF64(info?.StrOrNull("topped_up_balance")),
            Currency: info?.StrOrNull("currency") ?? "");

        return VendorState.Ok(snap);
    }

    private static double ParseF64(string? s)
        => double.TryParse(s?.Trim(), NumberStyles.Any, CultureInfo.InvariantCulture, out var v) ? v : 0.0;
}
