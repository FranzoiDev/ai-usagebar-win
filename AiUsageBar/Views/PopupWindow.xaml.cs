using System;
using System.Collections.Generic;
using AiUsageBar.Models;
using AiUsageBar.Services;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Foundation;
using Windows.Graphics;

namespace AiUsageBar.Views;

/// <summary>Frameless, always-on-top popup anchored near the tray click. It
/// light-dismisses when it loses focus, with a short grace period so the click
/// that opened it does not immediately close it (mirrors the Win32 original).</summary>
public sealed partial class PopupWindow : Window
{
    public event Action? RefreshRequested;
    public event Action? SettingsRequested;
    public event Action? QuitRequested;

    private readonly AppWindow _appWindow;
    private bool _visible;
    private DateTimeOffset _shownAt;
    private DateTimeOffset _hiddenAt;

    public PopupWindow()
    {
        InitializeComponent();

        _appWindow = AppWindow;
        var presenter = OverlappedPresenter.Create();
        presenter.IsResizable = false;
        presenter.IsMaximizable = false;
        presenter.IsMinimizable = false;
        presenter.IsAlwaysOnTop = true;
        presenter.SetBorderAndTitleBar(false, false);
        _appWindow.SetPresenter(presenter);
        _appWindow.IsShownInSwitchers = false;

        Activated += OnActivated;
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
        _appWindow.Show();
        Activate();
        _visible = true;
        _shownAt = DateTimeOffset.UtcNow;
        DispatcherQueue.TryEnqueue(PositionNearCursor);
    }

    public void HidePopup()
    {
        if (!_visible) return;
        _appWindow.Hide();
        _visible = false;
        _hiddenAt = DateTimeOffset.UtcNow;
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

    private void OnActivated(object sender, WindowActivatedEventArgs e)
    {
        if (e.WindowActivationState != WindowActivationState.Deactivated) return;
        // Grace period so the activating click does not instantly dismiss.
        if ((DateTimeOffset.UtcNow - _shownAt).TotalMilliseconds < 400) return;
        HidePopup();
    }

    private void PositionNearCursor()
    {
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        var dpi = NativeMethods.GetDpiForWindow(hwnd);
        var scale = dpi == 0 ? 1.0 : dpi / 96.0;

        const double widthEpx = 360.0;
        RootGrid.Measure(new Size(widthEpx, double.PositiveInfinity));
        var heightEpx = RootGrid.DesiredSize.Height;

        var wPx = (int)(widthEpx * scale);
        var hPx = (int)(heightEpx * scale);

        NativeMethods.GetCursorPos(out var pt);
        var work = DisplayArea.GetFromPoint(new PointInt32(pt.X, pt.Y), DisplayAreaFallback.Primary).WorkArea;

        const int margin = 8;
        var x = pt.X - wPx / 2;
        var y = pt.Y - hPx - margin; // prefer above the cursor (taskbar at bottom)

        if (x + wPx + margin > work.X + work.Width) x = work.X + work.Width - wPx - margin;
        if (x < work.X + margin) x = work.X + margin;
        if (y < work.Y + margin) y = pt.Y + margin; // flip below if no room above

        _appWindow.MoveAndResize(new RectInt32(x, y, wPx, hPx));
    }

    private void OnRefresh(object sender, RoutedEventArgs e) => RefreshRequested?.Invoke();
    private void OnSettings(object sender, RoutedEventArgs e) => SettingsRequested?.Invoke();
    private void OnQuit(object sender, RoutedEventArgs e) => QuitRequested?.Invoke();
}
