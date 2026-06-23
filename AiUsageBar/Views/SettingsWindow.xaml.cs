using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Windows;
using AiUsageBar.Models;
using AiUsageBar.Services;
using Wpf.Ui.Controls;

namespace AiUsageBar.Views;

/// <summary>A normal decorated window listing every supported vendor (configured
/// or not). Editable fields are bound two-way to the <see cref="SettingsModel"/>;
/// Save reconstructs a <see cref="Config"/> from it, persists, and pings the
/// poller. Closing hides the window so it can be reopened cheaply.</summary>
public partial class SettingsWindow : FluentWindow
{
    public event Action? Saved;

    private IReadOnlyList<VendorReport> _reports = Array.Empty<VendorReport>();
    private SettingsModel _model = new();

    public SettingsWindow()
    {
        InitializeComponent();

        // The X button hides rather than destroys, so the window can reopen.
        Closing += OnClosing;

        foreach (var id in VendorIdExtensions.All)
            PrimaryBox.Items.Add(id.Display());
    }

    public void ShowWith(Config cfg, IReadOnlyList<VendorReport> reports)
    {
        _reports = reports;
        Populate(cfg);
        Show();
        Activate();
    }

    private void OnClosing(object? sender, CancelEventArgs e)
    {
        e.Cancel = true;
        Hide();
    }

    private void Populate(Config cfg)
    {
        _model = Renderer.SettingsModel(cfg, _reports);
        PollBox.Value = _model.PollSeconds;
        PrimaryBox.SelectedIndex = Array.IndexOf(VendorIdExtensions.All, VendorIdExtensions.FromSlug(_model.Primary));
        StartupBox.IsChecked = StartupService.IsEnabled();
        VendorsList.ItemsSource = _model.Vendors;
    }

    private void OnSave(object sender, RoutedEventArgs e)
    {
        var cfg = new Config
        {
            PollSeconds = PollBox.Value is double d ? (long)d : 60,
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

        // "Start with Windows" lives in the registry, not the TOML config.
        StartupService.SetEnabled(StartupBox.IsChecked == true);

        Saved?.Invoke();
        // Rebuild so "configured" badges reflect the just-saved keys.
        Populate(sane);
    }

    private void OnClose(object sender, RoutedEventArgs e) => Hide();
}
