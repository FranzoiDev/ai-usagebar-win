using System;
using System.Collections.Generic;
using System.Windows;
using System.Windows.Media;
using AiUsageBar.Models;
using AiUsageBar.Services;

namespace AiUsageBar.Views;

/// <summary>Frameless, always-on-top popup anchored near the tray click. It
/// light-dismisses when it loses focus, with a short grace period so the click
/// that opened it does not immediately close it (mirrors the Win32 original).</summary>
public partial class PopupWindow : Window
{
    public event Action? RefreshRequested;
    public event Action? SettingsRequested;
    public event Action? QuitRequested;

    private bool _visible;
    private DateTimeOffset _shownAt;
    private DateTimeOffset _hiddenAt;

    public PopupWindow()
    {
        InitializeComponent();
        Deactivated += OnDeactivated;
    }

    public void Toggle(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        if (_visible)
        {
            HidePopup();
            return;
        }
        // The same click that dismissed it also re-fires here — ignore it.
        if ((DateTimeOffset.UtcNow - _hiddenAt).TotalMilliseconds < 300) return;

        Populate(cfg, reports);
        Show();
        UpdateLayout(); // realize SizeToContent so ActualWidth/Height are known
        PositionNearCursor();
        Activate();
        _visible = true;
        _shownAt = DateTimeOffset.UtcNow;
    }

    public void HidePopup()
    {
        if (!_visible) return;
        Hide();
        _visible = false;
        _hiddenAt = DateTimeOffset.UtcNow;
    }

    /// <summary>Show the popup (or re-focus it if already visible). Used when a
    /// second app launch — e.g. from Windows Search — asks the running instance
    /// to surface, where a plain toggle could wrongly dismiss an open popup.</summary>
    public void EnsureShown(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        if (_visible)
        {
            Activate();
            return;
        }
        Toggle(cfg, reports);
    }

    /// <summary>Rebuild in place when a new poll arrives while visible.</summary>
    public void Refresh(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        if (_visible) Populate(cfg, reports);
    }

    private void Populate(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        var model = Renderer.PopupModel(reports, cfg, cfg.Primary(), DateTimeOffset.UtcNow);
        VendorsList.ItemsSource = model.Vendors;
        EmptyLabel.Visibility = model.IsEmpty ? Visibility.Visible : Visibility.Collapsed;
    }

    private void OnDeactivated(object? sender, EventArgs e)
    {
        // Grace period so the activating click does not instantly dismiss.
        if ((DateTimeOffset.UtcNow - _shownAt).TotalMilliseconds < 400) return;
        HidePopup();
    }

    private void PositionNearCursor()
    {
        // GetCursorPos is in physical pixels; WPF Left/Top are in DIPs.
        NativeMethods.GetCursorPos(out var pt);
        var dpi = VisualTreeHelper.GetDpi(this);
        var cx = pt.X / dpi.DpiScaleX;
        var cy = pt.Y / dpi.DpiScaleY;

        var w = ActualWidth;
        var h = ActualHeight;
        var work = SystemParameters.WorkArea; // DIPs (primary monitor)

        const double margin = 8;
        var x = cx - w / 2;
        var y = cy - h - margin; // prefer above the cursor (taskbar at bottom)

        if (x + w + margin > work.Right) x = work.Right - w - margin;
        if (x < work.Left + margin) x = work.Left + margin;
        if (y < work.Top + margin) y = cy + margin; // flip below if no room above

        Left = x;
        Top = y;
    }

    private void OnRefresh(object sender, RoutedEventArgs e) => RefreshRequested?.Invoke();
    private void OnSettings(object sender, RoutedEventArgs e) => SettingsRequested?.Invoke();
    private void OnQuit(object sender, RoutedEventArgs e) => QuitRequested?.Invoke();
}
