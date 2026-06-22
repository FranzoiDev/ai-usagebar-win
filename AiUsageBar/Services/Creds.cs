using System;
using System.IO;
using System.Text;
using System.Text.Json;

namespace AiUsageBar.Services;

/// <summary>
/// Read-only readers for the OAuth credential files the official Claude and
/// Codex CLIs maintain.
///
/// DESIGN RULE: this app never writes these files and never refreshes tokens.
/// Refreshing would rotate the refresh-token out from under the user's CLI and
/// risk logging them out. We only *read* the access token; if it has already
/// expired we report <see cref="CredKind.Expired"/> and let the user
/// re-authenticate with their own CLI.
/// </summary>
public enum CredKind { Valid, Expired, Missing, Malformed }

public sealed class CredResult<T>
{
    public CredKind Kind { get; }
    public T? Value { get; }
    public string? Error { get; }

    private CredResult(CredKind kind, T? value, string? error)
    {
        Kind = kind;
        Value = value;
        Error = error;
    }

    public static CredResult<T> Valid(T value) => new(CredKind.Valid, value, null);
    public static CredResult<T> Expired() => new(CredKind.Expired, default, null);
    public static CredResult<T> Missing() => new(CredKind.Missing, default, null);
    public static CredResult<T> Malformed(string error) => new(CredKind.Malformed, default, error);
}

public sealed record AnthropicCreds(string AccessToken, string PlanLabel);

public sealed record OpenAiCreds(string AccessToken, string? AccountId, string? PlanHint);

public static class Creds
{
    // -- Anthropic — ~/.claude/.credentials.json ------------------------------

    public static CredResult<AnthropicCreds> ReadAnthropic(string path, long nowSecs)
    {
        if (!File.Exists(path)) return CredResult<AnthropicCreds>.Missing();

        string raw;
        try { raw = File.ReadAllText(path); }
        catch (Exception e) { return CredResult<AnthropicCreds>.Malformed(e.Message); }

        try
        {
            using var doc = JsonDocument.Parse(raw);
            if (!doc.RootElement.TryGetProperty("claudeAiOauth", out var o))
                return CredResult<AnthropicCreds>.Malformed("missing claudeAiOauth");

            var accessToken = GetString(o, "accessToken") ?? "";
            var expiresAtMs = GetLong(o, "expiresAt");
            var sub = GetString(o, "subscriptionType") ?? "";
            var tier = GetString(o, "rateLimitTier") ?? "";

            // Strict expiry: only flag once actually past expiry (we never refresh).
            if (expiresAtMs > 0 && expiresAtMs / 1000 <= nowSecs)
                return CredResult<AnthropicCreds>.Expired();

            return CredResult<AnthropicCreds>.Valid(
                new AnthropicCreds(accessToken, AnthropicPlanLabel(sub, tier)));
        }
        catch (Exception e)
        {
            return CredResult<AnthropicCreds>.Malformed(e.Message);
        }
    }

    private static string AnthropicPlanLabel(string sub, string tier)
    {
        var name = Capitalize(sub);
        if (string.IsNullOrEmpty(name)) name = "Unknown";
        if (tier.Contains("20x")) name += " 20x";
        else if (tier.Contains("5x")) name += " 5x";
        return name;
    }

    // -- OpenAI Codex — ~/.codex/auth.json ------------------------------------

    public static CredResult<OpenAiCreds> ReadOpenAi(string path, long nowSecs)
    {
        if (!File.Exists(path)) return CredResult<OpenAiCreds>.Missing();

        string raw;
        try { raw = File.ReadAllText(path); }
        catch (Exception e) { return CredResult<OpenAiCreds>.Malformed(e.Message); }

        try
        {
            using var doc = JsonDocument.Parse(raw);
            if (!doc.RootElement.TryGetProperty("tokens", out var t))
                return CredResult<OpenAiCreds>.Malformed("missing tokens");

            var accessToken = GetString(t, "access_token") ?? "";
            var idToken = GetString(t, "id_token") ?? "";
            var accountId = GetString(t, "account_id");

            var claims = ParseJwtClaims(idToken);
            long exp = 0;
            string? planHint = null;
            if (claims is { } c)
            {
                exp = GetLong(c, "exp");
                if (c.TryGetProperty("https://api.openai.com/auth", out var auth)
                    && auth.ValueKind == JsonValueKind.Object)
                {
                    planHint = GetString(auth, "chatgpt_plan_type");
                }
            }

            // exp == 0 means unparseable — attempt the fetch anyway; a 401 will
            // then surface as a re-login prompt. Only flag Expired when past.
            if (exp > 0 && exp <= nowSecs) return CredResult<OpenAiCreds>.Expired();

            return CredResult<OpenAiCreds>.Valid(new OpenAiCreds(accessToken, accountId, planHint));
        }
        catch (Exception e)
        {
            return CredResult<OpenAiCreds>.Malformed(e.Message);
        }
    }

    private static JsonElement? ParseJwtClaims(string token)
    {
        var parts = token.Split('.');
        if (parts.Length < 2) return null;
        try
        {
            var bytes = DecodeBase64Url(parts[1]);
            // Returned JsonElement must outlive the JsonDocument, so clone it.
            using var doc = JsonDocument.Parse(bytes);
            return doc.RootElement.Clone();
        }
        catch
        {
            return null;
        }
    }

    private static byte[] DecodeBase64Url(string s)
    {
        var b = s.Replace('-', '+').Replace('_', '/');
        switch (b.Length % 4)
        {
            case 2: b += "=="; break;
            case 3: b += "="; break;
        }
        return Convert.FromBase64String(b);
    }

    // -- lenient JSON helpers (mirror the tolerant Rust deserializers) --------

    private static string? GetString(JsonElement obj, string name)
        => obj.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String
            ? v.GetString()
            : null;

    private static long GetLong(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var v) || v.ValueKind != JsonValueKind.Number) return 0;
        if (v.TryGetInt64(out var i)) return i;
        return v.TryGetDouble(out var d) ? (long)d : 0;
    }

    private static string Capitalize(string s)
    {
        if (string.IsNullOrEmpty(s)) return string.Empty;
        return char.ToUpperInvariant(s[0]) + s[1..];
    }
}
