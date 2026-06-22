using System;
using System.Collections.Generic;
using AiUsageBar.Models;
using AiUsageBar.Services;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Graphics;

namespace AiUsageBar.Views;

/// <summary>A normal decorated window listing every supported vendor (configured
/// or not). Editable fields are bound two-way to the <see cref="SettingsModel"/>;
/// Save reconstructs a <see cref="Config"/> from it, persists, and pings the
/// poller. Closing hides the window so it can be reopened cheaply.</summary>
public sealed partial class SettingsWindow : Window
{
    public event Action? Saved;

    private readonly AppWindow _appWindow;
    private IReadOnlyList<VendorReport> _reports = Array.Empty<VendorReport>();
    private SettingsModel _model = new();

    public SettingsWindow()
    {
        InitializeComponent();

        _appWindow = AppWindow;
        _appWindow.Title = "AI Usage — Settings";
        _appWindow.Resize(new SizeInt32(580, 780));
        if (_appWindow.Presenter is OverlappedPresenter presenter)
            presenter.IsMaximizable = false;

        // The X button hides rather than destroys, so the window can reopen.
        _appWindow.Closing += (_, e) =>
        {
            e.Cancel = true;
            _appWindow.Hide();
        };

        foreach (var id in VendorIdExtensions.All)
            PrimaryBox.Items.Add(id.Display());
    }

    public void ShowWith(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        _reports = reports;
        Populate(cfg);
        _appWindow.Show();
        Activate();
    }

    private void Populate(Config cfg)
    {
        _model = Renderer.SettingsModel(cfg, _reports);
        PollBox.Value = _model.PollSeconds;
        PrimaryBox.SelectedIndex = Array.IndexOf(VendorIdExtensions.All, VendorIdExtensions.FromSlug(_model.Primary));
        VendorsList.ItemsSource = _model.Vendors;
    }

    private void OnSave(object sender, RoutedEventArgs e)
    {
        var cfg = new Config
        {
            PollSeconds = double.IsNaN(PollBox.Value) ? 60 : (long)PollBox.Value,
        };

        var idx = PrimaryBox.SelectedIndex < 0 ? 0 : PrimaryBox.SelectedIndex;
        cfg.Ui.Primary = VendorIdExtensions.All[idx].Slug();

        foreach (var v in _model.Vendors)
        {
            switch (VendorIdExtensions.FromSlug(v.Id))
            {
                case VendorId.Anthropic:
                    cfg.Anthropic.Enabled = v.Enabled;
                    break;
                case VendorId.Openai:
                    cfg.Openai.Enabled = v.Enabled;
                    break;
                case VendorId.Zai:
                    cfg.Zai.Enabled = v.Enabled;
                    if (!string.IsNullOrEmpty(v.ApiKeyEnv)) cfg.Zai.ApiKeyEnv = v.ApiKeyEnv;
                    cfg.Zai.ApiKey = v.ApiKey;
                    cfg.Zai.PlanTier = v.PlanTier;
                    break;
                case VendorId.Openrouter:
                    cfg.Openrouter.Enabled = v.Enabled;
                    if (!string.IsNullOrEmpty(v.ApiKeyEnv)) cfg.Openrouter.ApiKeyEnv = v.ApiKeyEnv;
                    cfg.Openrouter.ApiKey = v.ApiKey;
                    break;
                case VendorId.Deepseek:
                    cfg.Deepseek.Enabled = v.Enabled;
                    if (!string.IsNullOrEmpty(v.ApiKeyEnv)) cfg.Deepseek.ApiKeyEnv = v.ApiKeyEnv;
                    cfg.Deepseek.ApiKey = v.ApiKey;
                    break;
            }
        }

        var sane = cfg.Sanitized();
        try
        {
            sane.Save();
        }
        catch
        {
            // Best-effort: a failed save shouldn't crash the app.
        }

        Saved?.Invoke();
        // Rebuild so "configured" badges reflect the just-saved keys.
        Populate(sane);
    }

    private void OnClose(object sender, RoutedEventArgs e) => _appWindow.Hide();
}
