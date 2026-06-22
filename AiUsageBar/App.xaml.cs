using System;
using System.Collections.Generic;
using AiUsageBar.Models;
using AiUsageBar.Services;
using AiUsageBar.Views;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace AiUsageBar;

/// <summary>
/// Tray-first WinUI 3 app: there is no main window. <see cref="OnLaunched"/>
/// installs the notification-area icon and starts the background poller; the
/// popup and settings windows are created lazily on first use.
///
/// A background thread polls every vendor on an interval and marshals results
/// to the UI thread, which owns the tray icon and the windows.
/// </summary>
public partial class App : Application
{
    private DispatcherQueue _ui = null!;
    private TrayService _tray = null!;
    private Poller _poller = null!;

    private PopupWindow? _popup;
    private SettingsWindow? _settings;

    // Latest poll result, handed to windows when they open.
    private Config _cfg = new();
    private IReadOnlyList<VendorReport> _reports = Array.Empty<VendorReport>();

    public App() => InitializeComponent();

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _ui = DispatcherQueue.GetForCurrentThread();

        _tray = new TrayService();
        _tray.Clicked += OnTrayClicked;
        _tray.Init();

        _poller = new Poller(_ui);
        _poller.Updated += OnUpdated;
        _poller.Start();
    }

    /// <summary>Runs on the UI thread after each poll.</summary>
    private void OnUpdated(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        _cfg = cfg;
        _reports = reports;

        var rendered = Renderer.Render(reports, cfg, cfg.Primary(), DateTimeOffset.UtcNow);
        _tray.Update(rendered.Severity, rendered.Tooltip);

        // Only the popup rebuilds live. The settings form is intentionally not
        // refreshed on every poll — that would clobber unsaved edits. It is
        // repopulated when opened and again right after a save.
        _popup?.Refresh(cfg, reports);
    }

    private void OnTrayClicked()
    {
        _popup ??= CreatePopup();
        _popup.Toggle(_cfg, _reports);
    }

    private PopupWindow CreatePopup()
    {
        var p = new PopupWindow();
        p.RefreshRequested += () => _poller.TriggerRefresh();
        p.SettingsRequested += OpenSettings;
        p.QuitRequested += Quit;
        return p;
    }

    private void OpenSettings()
    {
        _popup?.HidePopup();
        _settings ??= CreateSettings();
        _settings.ShowWith(_cfg, _reports);
    }

    private SettingsWindow CreateSettings()
    {
        var s = new SettingsWindow();
        s.Saved += () => _poller.TriggerRefresh();
        return s;
    }

    private void Quit()
    {
        _tray.Dispose();
        _poller.Dispose();
        Exit();
    }
}
