using System;
using AiUsageBar.Models;
using H.NotifyIcon;

namespace AiUsageBar.Services;

/// <summary>Owns the notification-area icon via H.NotifyIcon. Any click (left or
/// right — there is no context menu) toggles the popup, matching the original.
///
/// NOTE (verify on first Windows build): the exact H.NotifyIcon event/property
/// names can vary by package version. This targets H.NotifyIcon.WinUI 2.x:
///   - <c>TaskbarIcon.Icon</c> is a <c>System.Drawing.Icon</c>;
///   - <c>ToolTipText</c> is the hover tooltip;
///   - <c>LeftClicked</c> / <c>RightClicked</c> are the click events;
///   - <c>ForceCreate()</c> realizes the icon.
/// If any name differs, adjust here only — the rest of the app is unaffected.</summary>
public sealed class TrayService : IDisposable
{
    private readonly TaskbarIcon _icon = new();

    /// <summary>Raised on the UI thread when the icon is clicked.</summary>
    public event Action? Clicked;

    public void Init()
    {
        _icon.ToolTipText = "ai-usagebar — loading…";
        _icon.Icon = TrayIconFactory.For(Severity.Low);
        _icon.LeftClicked += (_, _) => Clicked?.Invoke();
        _icon.RightClicked += (_, _) => Clicked?.Invoke();
        _icon.ForceCreate();
    }

    public void Update(Severity severity, string tooltip)
    {
        _icon.Icon = TrayIconFactory.For(severity);
        _icon.ToolTipText = ClampTooltip(tooltip);
    }

    /// <summary>Win32 tray tooltips are length-limited (~127 chars). Trim defensively.</summary>
    private static string ClampTooltip(string s)
    {
        const int max = 120;
        if (s.Length <= max) return s;
        return string.Concat(s.AsSpan(0, max - 1), "…");
    }

    public void Dispose() => _icon.Dispose();
}
