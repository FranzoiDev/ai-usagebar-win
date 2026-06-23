using System;
using System.Collections.Generic;
using System.Threading;
using System.Windows.Threading;
using AiUsageBar.Models;
using AiUsageBar.Services.Vendors;

namespace AiUsageBar.Services;

/// <summary>Background polling loop. Reloads config each cycle so settings
/// changes (and the resulting refresh ping) take effect without a restart.
/// Results are marshaled back to the UI thread via the dispatcher.</summary>
public sealed class Poller : IDisposable
{
    private readonly Dispatcher _ui;
    private readonly SemaphoreSlim _wake = new(0, 1);
    private readonly CancellationTokenSource _cts = new();

    /// <summary>Raised on the UI thread after each poll completes.</summary>
    public event Action<Config, IReadOnlyList<VendorReport>>? Updated;

    public Poller(Dispatcher uiThread) => _ui = uiThread;

    public void Start() => _ = LoopAsync(_cts.Token);

    /// <summary>Ask the loop to poll again immediately (e.g. after a save).</summary>
    public void TriggerRefresh()
    {
        try { _wake.Release(); }
        catch (SemaphoreFullException) { /* a refresh is already pending */ }
    }

    private async Task LoopAsync(CancellationToken ct)
    {
        while (!ct.IsCancellationRequested)
        {
            var cfg = Config.Load();
            var reports = await VendorClient.FetchAllAsync(cfg, DateTimeOffset.UtcNow).ConfigureAwait(false);

            _ui.BeginInvoke(() => Updated?.Invoke(cfg, reports));

            try
            {
                // Wake early on a refresh request; otherwise sleep for the interval.
                await _wake.WaitAsync(cfg.PollInterval(), ct).ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }
    }

    public void Dispose()
    {
        _cts.Cancel();
        _cts.Dispose();
        _wake.Dispose();
    }
}
