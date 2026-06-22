namespace AiUsageBar.Models;

/// <summary>The set of supported providers. Order is significant: it is the
/// default display order in the popup, settings, and tooltip.</summary>
public enum VendorId
{
    Anthropic,
    Openai,
    Zai,
    Openrouter,
    Deepseek,
}

public static class VendorIdExtensions
{
    public static readonly VendorId[] All =
    {
        VendorId.Anthropic,
        VendorId.Openai,
        VendorId.Zai,
        VendorId.Openrouter,
        VendorId.Deepseek,
    };

    /// <summary>Short tag for the compact tooltip line, e.g. "cld".</summary>
    public static string Short(this VendorId id) => id switch
    {
        VendorId.Anthropic => "cld",
        VendorId.Openai => "gpt",
        VendorId.Zai => "zai",
        VendorId.Openrouter => "or",
        VendorId.Deepseek => "ds",
        _ => "?",
    };

    public static string Display(this VendorId id) => id switch
    {
        VendorId.Anthropic => "Anthropic",
        VendorId.Openai => "OpenAI",
        VendorId.Zai => "Z.AI",
        VendorId.Openrouter => "OpenRouter",
        VendorId.Deepseek => "DeepSeek",
        _ => "Unknown",
    };

    /// <summary>Lowercase stable slug used in config + view-models.</summary>
    public static string Slug(this VendorId id) => id switch
    {
        VendorId.Anthropic => "anthropic",
        VendorId.Openai => "openai",
        VendorId.Zai => "zai",
        VendorId.Openrouter => "openrouter",
        VendorId.Deepseek => "deepseek",
        _ => "anthropic",
    };

    public static VendorId FromSlug(string? slug) => slug switch
    {
        "openai" => VendorId.Openai,
        "zai" => VendorId.Zai,
        "openrouter" => VendorId.Openrouter,
        "deepseek" => VendorId.Deepseek,
        _ => VendorId.Anthropic,
    };
}
