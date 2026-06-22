using System.Globalization;
using System.Text.Json;

namespace AiUsageBar.Services.Vendors;

/// <summary>Lenient JSON readers over <see cref="JsonElement"/>. These mirror the
/// permissive Rust serde deserializers: missing keys and unexpected types fall
/// back to defaults rather than throwing, and numbers may arrive as floats or
/// (for money/balance) as strings.</summary>
internal static class JsonUtil
{
    public static bool TryProp(this JsonElement e, string name, out JsonElement v)
    {
        if (e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out v)) return true;
        v = default;
        return false;
    }

    public static JsonElement? Obj(this JsonElement e, string name)
        => e.TryProp(name, out var v) ? v : null;

    public static string? StrOrNull(this JsonElement e, string name)
        => e.TryProp(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    public static string StrOr(this JsonElement e, string name, string def)
        => StrOrNull(e, name) ?? def;

    public static long LongOr(this JsonElement e, string name, long def = 0)
    {
        if (!e.TryProp(name, out var v) || v.ValueKind != JsonValueKind.Number) return def;
        if (v.TryGetInt64(out var i)) return i;
        return v.TryGetDouble(out var d) ? (long)d : def;
    }

    public static long? LongOrNull(this JsonElement e, string name)
    {
        if (!e.TryProp(name, out var v) || v.ValueKind != JsonValueKind.Number) return null;
        if (v.TryGetInt64(out var i)) return i;
        return v.TryGetDouble(out var d) ? (long)d : null;
    }

    public static double DblOr(this JsonElement e, string name, double def = 0)
    {
        if (!e.TryProp(name, out var v)) return def;
        if (v.ValueKind == JsonValueKind.Number && v.TryGetDouble(out var d)) return d;
        if (v.ValueKind == JsonValueKind.String
            && double.TryParse(v.GetString(), NumberStyles.Any, CultureInfo.InvariantCulture, out var s))
            return s;
        return def;
    }

    public static bool BoolOr(this JsonElement e, string name, bool def = false)
    {
        if (!e.TryProp(name, out var v)) return def;
        return v.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => def,
        };
    }
}
