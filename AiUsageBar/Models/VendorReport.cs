namespace AiUsageBar.Models;

public enum VendorStateKind
{
    /// <summary>Valid snapshot fetched.</summary>
    Ok,
    /// <summary>Credentials missing or expired — message names the CLI to run.</summary>
    NeedsLogin,
    /// <summary>Network / HTTP / parse failure.</summary>
    Error,
}

/// <summary>Result of polling one vendor. Mirrors the Rust <c>VendorState</c> enum.</summary>
public sealed class VendorState
{
    public VendorStateKind Kind { get; }
    public VendorSnapshot? Snapshot { get; }
    public string? Message { get; }

    private VendorState(VendorStateKind kind, VendorSnapshot? snapshot, string? message)
    {
        Kind = kind;
        Snapshot = snapshot;
        Message = message;
    }

    public static VendorState Ok(VendorSnapshot snapshot) => new(VendorStateKind.Ok, snapshot, null);
    public static VendorState NeedsLogin(string message) => new(VendorStateKind.NeedsLogin, null, message);
    public static VendorState Error(string message) => new(VendorStateKind.Error, null, message);
}

public sealed record VendorReport(VendorId Id, VendorState State);
