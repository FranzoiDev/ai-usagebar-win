using System;
using System.Net.Http;
using System.Text;
using System.Text.Json;

namespace AiUsageBar.Services;

/// <summary>
/// OAuth token refresh for the two CLI-managed providers (Anthropic Claude and
/// OpenAI Codex). Ported from the Linux <c>ai-usagebar</c> crate
/// (<c>anthropic/oauth.rs</c>, <c>openai/oauth.rs</c>).
///
/// This is the one place the app talks to the OAuth token endpoints. It is only
/// invoked when the user opts into token refresh (see <see cref="Config.RefreshEnabled"/>);
/// the default stays read-only. The refreshed tokens are written back to the
/// same credential files the CLIs read, so the app shares a single source of
/// truth rather than forking a stale copy.
/// </summary>
internal static class OAuthClient
{
    // Public CLI OAuth client IDs (not secrets) + endpoints.
    private const string AnthropicTokenUrl = "https://platform.claude.com/v1/oauth/token";
    private const string AnthropicClientId = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
    private const string AnthropicBeta = "oauth-2025-04-20";
    private const string AnthropicUserAgent = "claude-cli/1.0";

    private const string OpenAiTokenUrl = "https://auth.openai.com/oauth/token";
    private const string OpenAiClientId = "app_EMoamEEZ73f0CkXaXp7hrann";
    private const string OpenAiScope = "openid profile email";

    /// <summary>Refresh if the token expires within this many seconds (or already
    /// has). Mirrors the crate's <c>REFRESH_BUFFER_SECS = 300</c>.</summary>
    public const long RefreshBufferSecs = 300;

    /// <summary>True when a token expiring at <paramref name="expiresAtSecs"/>
    /// falls inside the refresh window relative to <paramref name="nowSecs"/>.</summary>
    public static bool NeedsRefresh(long expiresAtSecs, long nowSecs)
        => expiresAtSecs < nowSecs + RefreshBufferSecs;

    /// <summary>Outcome of a refresh attempt. On success <see cref="AccessToken"/>
    /// is set; <see cref="RefreshToken"/> / <see cref="IdToken"/> are present only
    /// when the server rotated them (callers keep the old ones otherwise).</summary>
    public sealed record RefreshResult(
        bool Ok,
        string? AccessToken,
        string? RefreshToken,
        string? IdToken,
        long? ExpiresInSecs,
        string? Error);

    private static RefreshResult Fail(string error)
        => new(false, null, null, null, null, error);

    public static Task<RefreshResult> RefreshAnthropicAsync(HttpClient http, string refreshToken)
    {
        var body = JsonSerializer.Serialize(new
        {
            grant_type = "refresh_token",
            client_id = AnthropicClientId,
            refresh_token = refreshToken,
        });
        var req = new HttpRequestMessage(HttpMethod.Post, AnthropicTokenUrl)
        {
            Content = new StringContent(body, Encoding.UTF8, "application/json"),
        };
        req.Headers.TryAddWithoutValidation("anthropic-beta", AnthropicBeta);
        req.Headers.TryAddWithoutValidation("User-Agent", AnthropicUserAgent);
        return SendAsync(http, req);
    }

    public static Task<RefreshResult> RefreshOpenAiAsync(HttpClient http, string refreshToken)
    {
        var body = JsonSerializer.Serialize(new
        {
            client_id = OpenAiClientId,
            grant_type = "refresh_token",
            refresh_token = refreshToken,
            scope = OpenAiScope,
        });
        var req = new HttpRequestMessage(HttpMethod.Post, OpenAiTokenUrl)
        {
            Content = new StringContent(body, Encoding.UTF8, "application/json"),
        };
        return SendAsync(http, req);
    }

    private static async Task<RefreshResult> SendAsync(HttpClient http, HttpRequestMessage req)
    {
        HttpResponseMessage resp;
        string body;
        try
        {
            using (req)
            {
                resp = await http.SendAsync(req).ConfigureAwait(false);
                body = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
            }
        }
        catch (Exception e)
        {
            return Fail($"network: {e.Message}");
        }

        if (!resp.IsSuccessStatusCode)
            return Fail(ParseErrorBody(body) ?? $"HTTP {(int)resp.StatusCode}");

        try
        {
            using var doc = JsonDocument.Parse(body);
            var root = doc.RootElement;
            var access = Str(root, "access_token");
            if (string.IsNullOrEmpty(access))
                return Fail("refresh response had no access_token");
            return new RefreshResult(
                true,
                access,
                Str(root, "refresh_token"),
                Str(root, "id_token"),
                LongOrNull(root, "expires_in"),
                null);
        }
        catch (Exception e)
        {
            return Fail($"bad refresh response: {e.Message}");
        }
    }

    /// <summary>Pull a human-readable message out of an OAuth error body, tolerating
    /// the three shapes the crate handles: <c>{error_description}</c>,
    /// <c>{error:{message}}</c>, <c>{error:"..."}</c>.</summary>
    private static string? ParseErrorBody(string body)
    {
        try
        {
            using var doc = JsonDocument.Parse(body);
            var root = doc.RootElement;
            if (Str(root, "error_description") is { } d) return d;
            if (root.TryGetProperty("error", out var err))
            {
                if (err.ValueKind == JsonValueKind.Object && Str(err, "message") is { } m) return m;
                if (err.ValueKind == JsonValueKind.String) return err.GetString();
            }
        }
        catch { /* not JSON — fall through */ }
        return null;
    }

    private static string? Str(JsonElement obj, string name)
        => obj.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String
            ? v.GetString()
            : null;

    private static long? LongOrNull(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var v) || v.ValueKind != JsonValueKind.Number) return null;
        if (v.TryGetInt64(out var i)) return i;
        return v.TryGetDouble(out var d) ? (long)d : null;
    }
}
