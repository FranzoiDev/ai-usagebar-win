using System;
using AiUsageBar.Models;
using AiUsageBar.Services;
using Tomlyn;
using Xunit;

namespace AiUsageBar.Tests;

public class ConfigTests
{
    [Fact]
    public void Defaults_Enable_Four_Vendors()
    {
        var c = new Config();
        Assert.True(c.IsEnabled(VendorId.Anthropic));
        Assert.True(c.IsEnabled(VendorId.Openai));
        Assert.True(c.IsEnabled(VendorId.Zai));
        Assert.True(c.IsEnabled(VendorId.Openrouter));
        Assert.False(c.IsEnabled(VendorId.Deepseek));
        Assert.Equal(4, c.EnabledVendors().Count);
    }

    [Fact]
    public void Parses_Partial_Config()
    {
        const string toml = """
            poll_seconds = 30
            [openai]
            enabled = false
            [zai]
            api_key = "sk-zai-inline"
            """;
        var c = Toml.ToModel<Config>(toml, options: new Tomlyn.TomlModelOptions { IgnoreMissingProperties = true });
        Assert.False(c.IsEnabled(VendorId.Openai));
        Assert.True(c.IsEnabled(VendorId.Anthropic));
        Assert.Equal("sk-zai-inline", c.Zai.ApiKey);
        Assert.Equal(30, c.PollInterval().TotalSeconds);
    }

    [Fact]
    public void Poll_Interval_Floor_Is_15()
    {
        var c = new Config { PollSeconds = 1 };
        Assert.Equal(15, c.PollInterval().TotalSeconds);
    }

    [Fact]
    public void Serializes_To_Toml_And_Round_Trips()
    {
        var c = new Config { PollSeconds = 45 };
        c.Ui.Primary = "openai";
        c.Zai.ApiKey = "sk-test";

        var opts = new Tomlyn.TomlModelOptions { IgnoreMissingProperties = true };
        var text = Toml.FromModel(c, opts);
        var back = Toml.ToModel<Config>(text, options: opts);

        Assert.Equal(45, back.PollSeconds);
        Assert.Equal("openai", back.Ui.Primary);
        Assert.Equal("sk-test", back.Zai.ApiKey);
    }

    [Fact]
    public void Sanitized_Drops_Blank_Keys_And_Floors_Poll()
    {
        var c = new Config { PollSeconds = 3 };
        c.Zai.ApiKey = "   ";
        c.Openrouter.ApiKey = "sk-real";
        c.Sanitized();

        Assert.Equal(15, c.PollSeconds);
        Assert.Null(c.Zai.ApiKey);
        Assert.Equal("sk-real", c.Openrouter.ApiKey);
    }

    [Fact]
    public void Resolve_Api_Key_Prefers_Inline_When_Env_Absent()
    {
        Assert.Equal("inline", Config.ResolveApiKey("DEFINITELY_UNSET_ENV_XYZ", "inline"));
        Assert.Null(Config.ResolveApiKey("DEFINITELY_UNSET_ENV_XYZ", null));
    }
}

public class UsageTests
{
    [Theory]
    [InlineData(10, Severity.Low)]
    [InlineData(50, Severity.Mid)]
    [InlineData(75, Severity.High)]
    [InlineData(90, Severity.Critical)]
    public void Severity_Thresholds(int pct, Severity expected)
        => Assert.Equal(expected, SeverityRules.ForPct(pct));

    [Fact]
    public void FmtReset_Buckets()
    {
        var now = DateTimeOffset.UtcNow;
        Assert.Equal("—", Format.Reset(null, now));
        Assert.Equal("now", Format.Reset(now.AddMinutes(-1), now));
        Assert.Equal("45m", Format.Reset(now.AddMinutes(45), now));
        Assert.StartsWith("2h", Format.Reset(now.AddHours(2), now));
        Assert.StartsWith("3d", Format.Reset(now.AddDays(3), now));
    }

    [Fact]
    public void ExtraUsage_Percent_And_Fmt()
    {
        var e = new ExtraUsage(5000, 250);
        Assert.Equal(5, e.Percent);
        Assert.Equal("$2.50 / $50.00", e.Fmt());
    }

    [Fact]
    public void OpenRouter_Consumed_Pct_Guards_Zero()
    {
        var s = new OpenRouterSnapshot("x", 0.0, 5.0, 0, 0, 0, true, null, null);
        Assert.Equal(0, s.ConsumedPct());
    }
}

public class RenderTests
{
    private static VendorReport Anthropic(int session, int weekly) => new(
        VendorId.Anthropic,
        VendorState.Ok(new AnthropicSnapshot(
            "Max 5x",
            new UsageWindow(session, null),
            new UsageWindow(weekly, null),
            null, null)));

    [Fact]
    public void Severity_Tracks_Worst_Window()
    {
        var r = Renderer.Render(new[] { Anthropic(40, 95) }, new Config(), VendorId.Anthropic, DateTimeOffset.UtcNow);
        Assert.Equal(Severity.Critical, r.Severity);
    }

    [Fact]
    public void Tooltip_Has_Compact_Line()
    {
        var r = Renderer.Render(new[] { Anthropic(29, 10) }, new Config(), VendorId.Anthropic, DateTimeOffset.UtcNow);
        Assert.Contains("cld 29%", r.Tooltip);
    }

    [Fact]
    public void Tooltip_Hides_Unconfigured_Login_Needed_Vendor()
    {
        var reports = new[] { new VendorReport(VendorId.Openai, VendorState.NeedsLogin("run codex login")) };
        var r = Renderer.Render(reports, new Config(), VendorId.Anthropic, DateTimeOffset.UtcNow);
        Assert.DoesNotContain("gpt", r.Tooltip);
        Assert.Equal("ai-usagebar — no models configured", r.Tooltip);
        Assert.Equal(Severity.Low, r.Severity);
    }

    [Fact]
    public void Popup_Includes_Ok_Vendor_With_Bars()
    {
        var m = Renderer.PopupModel(new[] { Anthropic(62, 24) }, new Config(), VendorId.Anthropic, DateTimeOffset.UtcNow);
        Assert.Single(m.Vendors);
        Assert.Equal("ok", m.Vendors[0].Status);
        Assert.Equal(62, m.Vendors[0].Bars[0].Pct);
        Assert.Equal("mid", m.Vendors[0].Bars[0].Level);
    }

    [Fact]
    public void Settings_Lists_Every_Vendor()
    {
        var m = Renderer.SettingsModel(new Config(), Array.Empty<VendorReport>());
        Assert.Equal(VendorIdExtensions.All.Length, m.Vendors.Count);
        Assert.Equal("anthropic", m.Primary);
    }
}
