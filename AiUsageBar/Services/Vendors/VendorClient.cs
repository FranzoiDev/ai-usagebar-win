using System;
using System.Globalization;
using System.Net.Http;
using System.Text.Json;
using AiUsageBar.Models;

namespace AiUsageBar.Services.Vendors;

/// <summary>Vendor dispatch. Each vendor reverse-engineers one provider's usage
/// endpoint (ported from the Linux <c>ai-usagebar</c> crate). Named
/// <c>VendorClient</c> (not <c>Vendors</c>) to avoid colliding with the
/// enclosing namespace.</summary>
public static class VendorClient
{
    /// <summary>HTTP timeout shared by every vendor request.</summary>
    public static readonly TimeSpan HttpTimeout = TimeSpan.FromSeconds(10);

    /// <summary>One shared client. Per-request headers (auth, user-agent) are set
    /// on each <see cref="HttpRequestMessage"/> since they vary by vendor.</summary>
    internal static readonly HttpClient Client = new(new SocketsHttpHandler())
    {
        Timeout = HttpTimeout,
    };

    /// <summary>Poll every enabled vendor sequentially. <paramref name="now"/> is
    /// injected for testability.</summary>
    public static async Task<List<VendorReport>> FetchAllAsync(Config cfg, DateTimeOffset now)
    {
        var reports = new List<VendorReport>();
        foreach (var id in cfg.EnabledVendors())
            reports.Add(new VendorReport(id, await FetchOneAsync(cfg, id, now).ConfigureAwait(false)));
        return reports;
    }

    private static Task<VendorState> FetchOneAsync(Config cfg, VendorId id, DateTimeOffset now) => id switch
    {
        VendorId.Anthropic => AnthropicVendor.FetchAsync(cfg, now),
        VendorId.Openai => OpenAiVendor.FetchAsync(cfg, now),
        VendorId.Zai => ZaiVendor.FetchAsync(cfg),
        VendorId.Openrouter => OpenRouterVendor.FetchAsync(cfg),
        VendorId.Deepseek => DeepseekVendor.FetchAsync(cfg),
        _ => Task.FromResult(VendorState.Error("unknown vendor")),
    };
}

/// <summary>Shared HTTP helpers: send a GET, map common failure statuses, and
/// hand back the parsed JSON root.</summary>
internal static class VendorHttp
{
    public const string DefaultUserAgent = "ai-usagebar-win";

    /// <summary>The outcome of a GET: either the parsed JSON root, or a terminal
    /// <see cref="VendorState"/> (network error, login needed, bad HTTP, etc.).</summary>
    public sealed class Fetch
    {
        public JsonDocument? Doc { get; init; }
        public VendorState? Failure { get; init; }
    }

    public static async Task<Fetch> GetJsonAsync(
        string url,
        Action<HttpRequestMessage> configure,
        string? loginHintOn401 = null)
    {
        HttpResponseMessage resp;
        string body;
        try
        {
            using var req = new HttpRequestMessage(HttpMethod.Get, url);
            configure(req);
            resp = await VendorClient.Client.SendAsync(req).ConfigureAwait(false);
            body = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
        }
        catch (Exception e)
        {
            return new Fetch { Failure = VendorState.Error($"network: {e.Message}") };
        }

        var code = (int)resp.StatusCode;
        if (code == 401 || code == 403)
            return new Fetch { Failure = VendorState.NeedsLogin(loginHintOn401 ?? "session invalid") };
        if (code is < 200 or >= 300)
            return new Fetch { Failure = VendorState.Error($"HTTP {code}") };

        try
        {
            return new Fetch { Doc = JsonDocument.Parse(body) };
        }
        catch (Exception e)
        {
            return new Fetch { Failure = VendorState.Error($"bad response: {e.Message}") };
        }
    }

    public static string Capitalize(string s)
        => string.IsNullOrEmpty(s) ? string.Empty : char.ToUpperInvariant(s[0]) + s[1..];

    public static int RoundClamp(double v, int lo = 0, int hi = 100)
        => (int)Math.Clamp(Math.Round(v, MidpointRounding.AwayFromZero), lo, hi);
}
