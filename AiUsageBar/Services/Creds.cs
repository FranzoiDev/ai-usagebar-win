using System;
using System.IO;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace AiUsageBar.Services;

/// <summary>
/// Readers for the OAuth credential files the official Claude and Codex CLIs
/// maintain.
///
/// DEFAULT: strictly read-only. We read the access token; if it has already
/// expired we report <see cref="CredKind.Expired"/> and let the user
/// re-authenticate with their own CLI. This never touches the files.
///
/// OPT-IN REFRESH: when the user enables it (<see cref="Config.RefreshEnabled"/>),
/// the <c>...Async</c> readers will, on a near-expiry token, call the OAuth
/// endpoint and write the rotated tokens back to the same credential file the
/// CLI reads — sharing one source of truth instead of forking a stale copy.
/// This rotates the refresh token, so a CLI mid-session may need to re-login;
/// that trade-off is surfaced in the settings UI before the user turns it on.
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

    /// <summary>Raw on-disk OAuth fields, before any expiry/refresh policy.</summary>
    private sealed record AnthropicRaw(
        string AccessToken, string RefreshToken, long ExpiresAtMs, string Sub, string Tier);

    private static AnthropicRaw? ReadAnthropicRaw(string path, out string? error)
    {
        error = null;
        string raw;
        try { raw = File.ReadAllText(path); }
        catch (Exception e) { error = e.Message; return null; }

        try
        {
            using var doc = JsonDocument.Parse(raw);
            if (!doc.RootElement.TryGetProperty("claudeAiOauth", out var o))
            {
                error = "missing claudeAiOauth";
                return null;
            }
            return new AnthropicRaw(
                GetString(o, "accessToken") ?? "",
                GetString(o, "refreshToken") ?? "",
                GetLong(o, "expiresAt"),
                GetString(o, "subscriptionType") ?? "",
                GetString(o, "rateLimitTier") ?? "");
        }
        catch (Exception e)
        {
            error = e.Message;
            return null;
        }
    }

    /// <summary>Read-only: never refreshes. Flags <see cref="CredKind.Expired"/>
    /// once the token is past expiry.</summary>
    public static CredResult<AnthropicCreds> ReadAnthropic(string path, long nowSecs)
    {
        if (!File.Exists(path)) return CredResult<AnthropicCreds>.Missing();

        var c = ReadAnthropicRaw(path, out var error);
        if (c is null) return CredResult<AnthropicCreds>.Malformed(error ?? "unknown error");

        // Strict expiry: only flag once actually past expiry (we never refresh).
        if (c.ExpiresAtMs > 0 && c.ExpiresAtMs / 1000 <= nowSecs)
            return CredResult<AnthropicCreds>.Expired();

        return CredResult<AnthropicCreds>.Valid(
            new AnthropicCreds(c.AccessToken, AnthropicPlanLabel(c.Sub, c.Tier)));
    }

    /// <summary>Refresh-aware reader. Falls back to the read-only path when
    /// <paramref name="refreshEnabled"/> is false. When enabled and the token is
    /// near expiry, it refreshes via the OAuth endpoint and writes the rotated
    /// tokens back to disk (best-effort) before returning the fresh access
    /// token.</summary>
    public static async Task<CredResult<AnthropicCreds>> ReadAnthropicAsync(
        string path, long nowSecs, bool refreshEnabled, HttpClient http)
    {
        if (!refreshEnabled) return ReadAnthropic(path, nowSecs);
        if (!File.Exists(path)) return CredResult<AnthropicCreds>.Missing();

        var c = ReadAnthropicRaw(path, out var error);
        if (c is null) return CredResult<AnthropicCreds>.Malformed(error ?? "unknown error");

        var plan = AnthropicPlanLabel(c.Sub, c.Tier);
        var expiresSecs = c.ExpiresAtMs > 0 ? c.ExpiresAtMs / 1000 : 0;

        // Fresh enough, or no refresh token to use: keep the existing token.
        if (!OAuthClient.NeedsRefresh(expiresSecs, nowSecs) || string.IsNullOrEmpty(c.RefreshToken))
            return ResolveAnthropic(c.AccessToken, plan, expiresSecs, nowSecs);

        var r = await OAuthClient.RefreshAnthropicAsync(http, c.RefreshToken).ConfigureAwait(false);
        if (r.Ok && r.AccessToken is { } at)
        {
            var newRefresh = string.IsNullOrEmpty(r.RefreshToken) ? c.RefreshToken : r.RefreshToken!;
            var newExpiresMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()
                               + (r.ExpiresInSecs ?? 3600) * 1000;
            try { WriteAnthropicBack(path, at, newRefresh, newExpiresMs); }
            catch { /* best-effort: the refresh worked, so still serve fresh data */ }
            return CredResult<AnthropicCreds>.Valid(new AnthropicCreds(at, plan));
        }

        // Refresh failed: the existing token may still be valid for a few minutes.
        return ResolveAnthropic(c.AccessToken, plan, expiresSecs, nowSecs);
    }

    /// <summary>Decide what to return for a non-refreshed Anthropic token: still
    /// usable, or already expired.</summary>
    private static CredResult<AnthropicCreds> ResolveAnthropic(
        string accessToken, string plan, long expiresSecs, long nowSecs)
        => expiresSecs > 0 && expiresSecs <= nowSecs
            ? CredResult<AnthropicCreds>.Expired()
            : CredResult<AnthropicCreds>.Valid(new AnthropicCreds(accessToken, plan));

    /// <summary>Merge rotated tokens into the existing credentials JSON, preserving
    /// every other field the Claude CLI keeps there. Atomic.</summary>
    private static void WriteAnthropicBack(string path, string access, string refresh, long expiresAtMs)
    {
        var root = LoadJsonObject(path);
        if (root["claudeAiOauth"] is not JsonObject oauth)
        {
            oauth = new JsonObject();
            root["claudeAiOauth"] = oauth;
        }
        oauth["accessToken"] = access;
        oauth["refreshToken"] = refresh;
        oauth["expiresAt"] = expiresAtMs;
        WriteJsonAtomic(path, root);
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

    /// <summary>Raw on-disk OpenAI Codex token fields. <c>ExpSecs</c> is parsed
    /// from the id_token JWT (0 when unparseable).</summary>
    private sealed record OpenAiRaw(
        string AccessToken, string RefreshToken, string IdToken,
        string? AccountId, long ExpSecs, string? PlanHint);

    private static OpenAiRaw? ReadOpenAiRaw(string path, out string? error)
    {
        error = null;
        string raw;
        try { raw = File.ReadAllText(path); }
        catch (Exception e) { error = e.Message; return null; }

        try
        {
            using var doc = JsonDocument.Parse(raw);
            if (!doc.RootElement.TryGetProperty("tokens", out var t))
            {
                error = "missing tokens";
                return null;
            }

            var idToken = GetString(t, "id_token") ?? "";
            var (exp, planHint) = ParseIdToken(idToken);
            return new OpenAiRaw(
                GetString(t, "access_token") ?? "",
                GetString(t, "refresh_token") ?? "",
                idToken,
                GetString(t, "account_id"),
                exp,
                planHint);
        }
        catch (Exception e)
        {
            error = e.Message;
            return null;
        }
    }

    /// <summary>Extract <c>exp</c> (Unix seconds, 0 if absent) and the ChatGPT
    /// plan-type hint from an id_token JWT.</summary>
    private static (long Exp, string? PlanHint) ParseIdToken(string idToken)
    {
        var claims = ParseJwtClaims(idToken);
        if (claims is not { } c) return (0, null);
        var exp = GetLong(c, "exp");
        string? planHint = null;
        if (c.TryGetProperty("https://api.openai.com/auth", out var auth)
            && auth.ValueKind == JsonValueKind.Object)
        {
            planHint = GetString(auth, "chatgpt_plan_type");
        }
        return (exp, planHint);
    }

    /// <summary>Read-only: never refreshes.</summary>
    public static CredResult<OpenAiCreds> ReadOpenAi(string path, long nowSecs)
    {
        if (!File.Exists(path)) return CredResult<OpenAiCreds>.Missing();

        var c = ReadOpenAiRaw(path, out var error);
        if (c is null) return CredResult<OpenAiCreds>.Malformed(error ?? "unknown error");

        // exp == 0 means unparseable — attempt the fetch anyway; a 401 will then
        // surface as a re-login prompt. Only flag Expired when past.
        if (c.ExpSecs > 0 && c.ExpSecs <= nowSecs) return CredResult<OpenAiCreds>.Expired();

        return CredResult<OpenAiCreds>.Valid(new OpenAiCreds(c.AccessToken, c.AccountId, c.PlanHint));
    }

    /// <summary>Refresh-aware reader; see <see cref="ReadAnthropicAsync"/>.</summary>
    public static async Task<CredResult<OpenAiCreds>> ReadOpenAiAsync(
        string path, long nowSecs, bool refreshEnabled, HttpClient http)
    {
        if (!refreshEnabled) return ReadOpenAi(path, nowSecs);
        if (!File.Exists(path)) return CredResult<OpenAiCreds>.Missing();

        var c = ReadOpenAiRaw(path, out var error);
        if (c is null) return CredResult<OpenAiCreds>.Malformed(error ?? "unknown error");

        // Refresh when near expiry, or when the expiry is unknown (exp == 0).
        var needsRefresh = c.ExpSecs <= 0 || OAuthClient.NeedsRefresh(c.ExpSecs, nowSecs);
        if (!needsRefresh || string.IsNullOrEmpty(c.RefreshToken))
            return ResolveOpenAi(c.AccessToken, c.AccountId, c.PlanHint, c.ExpSecs, nowSecs);

        var r = await OAuthClient.RefreshOpenAiAsync(http, c.RefreshToken).ConfigureAwait(false);
        if (r.Ok && r.AccessToken is { } at)
        {
            var newRefresh = string.IsNullOrEmpty(r.RefreshToken) ? c.RefreshToken : r.RefreshToken!;
            var newId = string.IsNullOrEmpty(r.IdToken) ? c.IdToken : r.IdToken!;
            try { WriteOpenAiBack(path, at, newRefresh, newId); }
            catch { /* best-effort */ }
            // Re-derive the plan hint from the new id_token when we got one.
            var planHint = ParseIdToken(newId).PlanHint ?? c.PlanHint;
            return CredResult<OpenAiCreds>.Valid(new OpenAiCreds(at, c.AccountId, planHint));
        }

        return ResolveOpenAi(c.AccessToken, c.AccountId, c.PlanHint, c.ExpSecs, nowSecs);
    }

    private static CredResult<OpenAiCreds> ResolveOpenAi(
        string accessToken, string? accountId, string? planHint, long expSecs, long nowSecs)
        => expSecs > 0 && expSecs <= nowSecs
            ? CredResult<OpenAiCreds>.Expired()
            : CredResult<OpenAiCreds>.Valid(new OpenAiCreds(accessToken, accountId, planHint));

    /// <summary>Merge rotated tokens into <c>auth.json</c>, preserving every other
    /// field the Codex CLI keeps there. Atomic.</summary>
    private static void WriteOpenAiBack(string path, string access, string refresh, string idToken)
    {
        var root = LoadJsonObject(path);
        if (root["tokens"] is not JsonObject tokens)
        {
            tokens = new JsonObject();
            root["tokens"] = tokens;
        }
        tokens["access_token"] = access;
        tokens["refresh_token"] = refresh;
        tokens["id_token"] = idToken;
        // The Codex CLI stamps this on its own refreshes; mirror it.
        root["last_refresh"] = DateTimeOffset.UtcNow.ToString("o");
        WriteJsonAtomic(path, root);
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

    // -- write-back helpers (only reached on opt-in refresh) ------------------

    /// <summary>Parse an existing JSON file into a mutable object, tolerating a
    /// missing/garbage file by starting fresh — so a write-back never throws on
    /// read and always lands a valid document.</summary>
    private static JsonObject LoadJsonObject(string path)
    {
        try
        {
            if (File.Exists(path) && JsonNode.Parse(File.ReadAllText(path)) is JsonObject obj)
                return obj;
        }
        catch { /* fall through to a fresh object */ }
        return new JsonObject();
    }

    /// <summary>Write <paramref name="root"/> via a temp file + atomic replace, so
    /// a crash mid-write can't truncate the credential file the CLI depends on.</summary>
    private static void WriteJsonAtomic(string path, JsonObject root)
    {
        var json = root.ToJsonString(new JsonSerializerOptions { WriteIndented = true });
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, json);
        File.Move(tmp, path, overwrite: true);
    }
}
