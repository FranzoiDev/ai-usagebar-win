using System;
using System.IO;
using System.Text;
using System.Text.Json;
using AiUsageBar.Services;
using Xunit;

namespace AiUsageBar.Tests;

public class CredsTests : IDisposable
{
    private readonly List<string> _temps = new();

    private string Tmp(string contents)
    {
        var path = Path.Combine(Path.GetTempPath(), $"aub-{Guid.NewGuid():N}.json");
        File.WriteAllText(path, contents);
        _temps.Add(path);
        return path;
    }

    private static string Jwt(object claims)
    {
        static string B64Url(byte[] b) => Convert.ToBase64String(b)
            .TrimEnd('=').Replace('+', '-').Replace('/', '_');
        var header = B64Url(Encoding.UTF8.GetBytes("{}"));
        var payload = B64Url(Encoding.UTF8.GetBytes(JsonSerializer.Serialize(claims)));
        return $"{header}.{payload}.sig";
    }

    [Fact]
    public void Anthropic_Valid_Token()
    {
        var f = Tmp("""
            {"claudeAiOauth":{"accessToken":"AT","refreshToken":"RT",
            "expiresAt":2000000000000,"subscriptionType":"max",
            "rateLimitTier":"default_claude_max_20x"}}
            """);
        var r = Creds.ReadAnthropic(f, 1_000_000_000);
        Assert.Equal(CredKind.Valid, r.Kind);
        Assert.Equal("AT", r.Value!.AccessToken);
        Assert.Equal("Max 20x", r.Value!.PlanLabel);
    }

    [Fact]
    public void Anthropic_Expired_Is_Flagged_Not_Refreshed()
    {
        var f = Tmp("""
            {"claudeAiOauth":{"accessToken":"AT","refreshToken":"RT",
            "expiresAt":1000,"subscriptionType":"pro","rateLimitTier":""}}
            """);
        Assert.Equal(CredKind.Expired, Creds.ReadAnthropic(f, 1_000_000_000).Kind);
    }

    [Fact]
    public void Anthropic_Missing_File()
    {
        var r = Creds.ReadAnthropic(Path.Combine(Path.GetTempPath(), "nope-aub", ".credentials.json"), 0);
        Assert.Equal(CredKind.Missing, r.Kind);
    }

    [Fact]
    public void OpenAi_Valid_With_Plan_Hint()
    {
        var id = Jwt(new Dictionary<string, object>
        {
            ["exp"] = 2_000_000_000,
            ["https://api.openai.com/auth"] = new Dictionary<string, object> { ["chatgpt_plan_type"] = "plus" },
        });
        var f = Tmp($$"""{"tokens":{"access_token":"AT","refresh_token":"RT","id_token":"{{id}}","account_id":"acc"}}""");
        var r = Creds.ReadOpenAi(f, 1_000_000_000);
        Assert.Equal(CredKind.Valid, r.Kind);
        Assert.Equal("AT", r.Value!.AccessToken);
        Assert.Equal("acc", r.Value!.AccountId);
        Assert.Equal("plus", r.Value!.PlanHint);
    }

    [Fact]
    public void OpenAi_Expired()
    {
        var id = Jwt(new Dictionary<string, object> { ["exp"] = 1000 });
        var f = Tmp($$"""{"tokens":{"access_token":"AT","refresh_token":"RT","id_token":"{{id}}"}}""");
        Assert.Equal(CredKind.Expired, Creds.ReadOpenAi(f, 1_000_000_000).Kind);
    }

    [Fact]
    public void OpenAi_Unparseable_Exp_Attempts_Anyway()
    {
        var f = Tmp("""{"tokens":{"access_token":"AT","refresh_token":"RT","id_token":"not.a.jwt"}}""");
        Assert.Equal(CredKind.Valid, Creds.ReadOpenAi(f, 1_000_000_000).Kind);
    }

    public void Dispose()
    {
        foreach (var t in _temps)
            try { File.Delete(t); } catch { /* ignore */ }
    }
}
