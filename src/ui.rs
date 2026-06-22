//! Embedded WebView documents. Kept as self-contained HTML strings (no asset
//! files) so the binary stays a single artifact, mirroring the rest of the app.
//!
//! Rust → JS: the host calls `window.__data(model)` / `window.__config(model)`
//! via `evaluate_script`. JS → Rust: the page calls `send({cmd: ...})` which
//! forwards a JSON string over `window.ipc.postMessage`.

/// Frameless popup shown near the tray icon: vendor cards + progress bars.
pub const POPUP_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  :root {
    --bg: #16171d;
    --card: #20222b;
    --card2: #262934;
    --line: #2e3140;
    --text: #e7e9f0;
    --muted: #9aa0b4;
    --accent: #6d8bff;
    --low: #4caf50;
    --mid: #ffc107;
    --high: #ff9800;
    --crit: #f44336;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  html, body {
    background: transparent;
    font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
    color: var(--text);
    -webkit-user-select: none;
    user-select: none;
    overflow: hidden;
  }
  #shell {
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 14px;
    overflow: hidden;
  }
  main { padding: 12px 12px 8px; display: flex; flex-direction: column; gap: 10px; }
  .card {
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 12px 13px;
  }
  .card.err { border-color: rgba(244,67,54,.4); }
  .card .top { display: flex; align-items: baseline; gap: 8px; margin-bottom: 10px; }
  .card .name { font-size: 13px; font-weight: 600; }
  .card .plan { font-size: 11px; color: var(--muted); margin-left: auto; }
  .bar { margin: 9px 0; }
  .bar .meta { display: flex; justify-content: space-between; font-size: 11px; margin-bottom: 4px; }
  .bar .meta .label { color: var(--muted); }
  .bar .meta .reset { color: var(--muted); font-variant-numeric: tabular-nums; }
  .bar .meta .pct { font-weight: 600; font-variant-numeric: tabular-nums; }
  .track { height: 7px; border-radius: 6px; background: var(--card2); overflow: hidden; }
  .fill { height: 100%; border-radius: 6px; transition: width .45s ease; }
  .fill.low { background: linear-gradient(90deg,#3ea043,#4caf50); }
  .fill.mid { background: linear-gradient(90deg,#e0a800,#ffc107); }
  .fill.high { background: linear-gradient(90deg,#fb8c00,#ff9800); }
  .fill.critical { background: linear-gradient(90deg,#e53935,#ff5252); }
  .facts { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .chip {
    font-size: 11px; color: var(--muted);
    background: var(--card2); border-radius: 999px; padding: 3px 9px;
  }
  .chip b { color: var(--text); font-weight: 600; }
  .errmsg { font-size: 11px; color: #ff8a80; }
  .empty { color: var(--muted); font-size: 12px; text-align: center; padding: 22px 8px; }
  footer {
    display: flex; gap: 8px; padding: 10px 12px 12px;
    border-top: 1px solid var(--line);
  }
  footer button {
    flex: 1; display: flex; align-items: center; justify-content: center; gap: 6px;
    background: var(--card); color: var(--text);
    border: 1px solid var(--line); border-radius: 9px;
    padding: 8px 6px; font-size: 12px; cursor: pointer;
  }
  footer button:hover { background: var(--card2); }
  footer button.icon { flex: 0 0 44px; font-size: 15px; color: var(--muted); }
  footer button.icon:hover { color: var(--text); }
  footer button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  footer button.primary:hover { filter: brightness(1.08); }
  footer button.quit:hover { background: rgba(244,67,54,.18); border-color: rgba(244,67,54,.5); color: #ff8a80; }
</style>
</head>
<body>
  <div id="shell">
    <main id="list"></main>
    <footer>
      <button class="icon" id="refresh" title="Refresh now">⟳</button>
      <button class="primary" id="settings">⚙ Settings</button>
      <button class="quit" id="quit">⏻ Quit</button>
    </footer>
  </div>
<script>
  function send(o){ try { window.ipc.postMessage(JSON.stringify(o)); } catch(e){} }
  function el(tag, cls, txt){ var e=document.createElement(tag); if(cls)e.className=cls; if(txt!=null)e.textContent=txt; return e; }

  function reportHeight(){
    var h = document.getElementById('shell').offsetHeight + 2;
    send({cmd:'resize', h: Math.ceil(h)});
  }

  window.__data = function(model){
    var list = document.getElementById('list');
    list.innerHTML = '';
    var vendors = (model && model.vendors) || [];
    if(!vendors.length){
      list.appendChild(el('div','empty','No models configured yet.\nOpen Settings to add an API key.'));
      requestAnimationFrame(reportHeight);
      return;
    }
    vendors.forEach(function(v){
      var card = el('div','card' + (v.status==='error'?' err':''));
      var top = el('div','top');
      top.appendChild(el('span','name', v.name));
      if(v.plan) top.appendChild(el('span','plan', v.plan));
      card.appendChild(top);

      if(v.status==='error'){
        card.appendChild(el('div','errmsg', v.message || 'unavailable'));
      }
      (v.bars||[]).forEach(function(b){
        var bar = el('div','bar');
        var meta = el('div','meta');
        meta.appendChild(el('span','label', b.label));
        var right = el('span','reset', b.reset ? ('resets '+b.reset) : '');
        meta.appendChild(right);
        bar.appendChild(meta);
        var pctRow = el('div','meta');
        pctRow.appendChild(el('span','label',''));
        pctRow.style.margin = '0 0 4px';
        var track = el('div','track');
        var fill = el('div','fill ' + (b.level||'low'));
        fill.style.width = Math.max(0, Math.min(100, b.pct)) + '%';
        track.appendChild(fill);
        // percentage label on the right of the meta row
        right.textContent = (b.reset ? ('resets '+b.reset+'  ·  ') : '') + b.pct + '%';
        bar.appendChild(track);
        card.appendChild(bar);
      });
      if((v.facts||[]).length){
        var facts = el('div','facts');
        v.facts.forEach(function(f){
          var chip = el('span','chip');
          chip.appendChild(document.createTextNode(f.label+': '));
          chip.appendChild(el('b',null,f.value));
          facts.appendChild(chip);
        });
        card.appendChild(facts);
      }
      list.appendChild(card);
    });
    requestAnimationFrame(reportHeight);
  };

  document.getElementById('refresh').onclick = function(){ send({cmd:'refresh'}); };
  document.getElementById('settings').onclick = function(){ send({cmd:'settings'}); };
  document.getElementById('quit').onclick = function(){ send({cmd:'quit'}); };
  window.addEventListener('keydown', function(e){ if(e.key==='Escape') send({cmd:'hide'}); });
  send({cmd:'popupReady'});
</script>
</body>
</html>"##;

/// Decorated OS window: enable/disable vendors and manage API keys.
pub const SETTINGS_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  :root {
    --bg: #16171d; --card: #20222b; --card2: #262934; --line: #2e3140;
    --text: #e7e9f0; --muted: #9aa0b4; --accent: #6d8bff; --ok: #4caf50;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    background: var(--bg); color: var(--text);
    font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
    font-size: 13px; min-height: 100vh; display: flex; flex-direction: column;
  }
  header { padding: 16px 20px; border-bottom: 1px solid var(--line); }
  header h1 { font-size: 16px; font-weight: 600; }
  header p { color: var(--muted); font-size: 12px; margin-top: 3px; }
  main { padding: 16px 20px; flex: 1; display: flex; flex-direction: column; gap: 14px; }
  .row { display: flex; gap: 16px; }
  .field { flex: 1; }
  label.lbl { display: block; color: var(--muted); font-size: 11px; margin-bottom: 5px; text-transform: uppercase; letter-spacing: .4px; }
  input, select {
    width: 100%; background: var(--card2); color: var(--text);
    border: 1px solid var(--line); border-radius: 8px; padding: 8px 10px; font-size: 13px;
  }
  input:focus, select:focus { outline: none; border-color: var(--accent); }
  .vendor {
    background: var(--card); border: 1px solid var(--line);
    border-radius: 12px; padding: 14px;
  }
  .vendor .head { display: flex; align-items: center; gap: 10px; }
  .vendor .head .name { font-weight: 600; font-size: 14px; }
  .badge { font-size: 10px; padding: 2px 8px; border-radius: 999px; background: var(--card2); color: var(--muted); }
  .badge.on { background: rgba(76,175,80,.18); color: #7bd17f; }
  .vendor .head .spacer { flex: 1; }
  .status { font-size: 11px; color: var(--muted); margin-top: 6px; }
  .body { margin-top: 12px; display: none; flex-direction: column; gap: 12px; }
  .vendor.open .body { display: flex; }
  .hint { font-size: 12px; color: var(--muted); line-height: 1.5; }
  /* toggle */
  .switch { position: relative; width: 40px; height: 22px; }
  .switch input { display: none; }
  .slider { position: absolute; inset: 0; background: var(--card2); border: 1px solid var(--line); border-radius: 999px; cursor: pointer; transition: .2s; }
  .slider::before { content:""; position:absolute; height:16px; width:16px; left:2px; top:2px; background:#cfd3e0; border-radius:50%; transition:.2s; }
  .switch input:checked + .slider { background: var(--accent); border-color: var(--accent); }
  .switch input:checked + .slider::before { transform: translateX(18px); background:#fff; }
  footer {
    padding: 14px 20px; border-top: 1px solid var(--line);
    display: flex; gap: 10px; justify-content: flex-end; position: sticky; bottom: 0; background: var(--bg);
  }
  footer button { border-radius: 9px; padding: 9px 18px; font-size: 13px; cursor: pointer; border: 1px solid var(--line); background: var(--card); color: var(--text); }
  footer button:hover { background: var(--card2); }
  footer button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  footer button.primary:hover { filter: brightness(1.08); }
  #saved { color: var(--ok); font-size: 12px; align-self: center; margin-right: auto; opacity: 0; transition: opacity .2s; }
  #saved.show { opacity: 1; }
</style>
</head>
<body>
  <header>
    <h1>Settings</h1>
    <p>Enable models and manage API keys. Saved to your config file.</p>
  </header>
  <main>
    <div class="row">
      <div class="field">
        <label class="lbl">Refresh interval (seconds)</label>
        <input id="poll" type="number" min="15" step="5">
      </div>
      <div class="field">
        <label class="lbl">Primary (tray tooltip)</label>
        <select id="primary"></select>
      </div>
    </div>
    <div id="vendors"></div>
  </main>
  <footer>
    <span id="saved">Saved ✓</span>
    <button id="cancel">Close</button>
    <button class="primary" id="save">Save</button>
  </footer>
<script>
  function send(o){ try { window.ipc.postMessage(JSON.stringify(o)); } catch(e){} }
  function el(tag, cls, txt){ var e=document.createElement(tag); if(cls)e.className=cls; if(txt!=null)e.textContent=txt; return e; }
  var MODEL = null;

  window.__config = function(model){
    MODEL = model;
    document.getElementById('poll').value = model.poll_seconds;
    var prim = document.getElementById('primary');
    prim.innerHTML = '';
    model.vendors.forEach(function(v){
      var o = el('option', null, v.name); o.value = v.id;
      if(v.id === model.primary) o.selected = true;
      prim.appendChild(o);
    });

    var wrap = document.getElementById('vendors');
    wrap.innerHTML = '';
    model.vendors.forEach(function(v){
      var card = el('div','vendor' + (v.enabled?' open':''));
      card.dataset.id = v.id;

      var head = el('div','head');
      head.appendChild(el('span','name', v.name));
      head.appendChild(el('span','badge'+(v.configured?' on':''), v.configured?'configured':'not set'));
      head.appendChild(el('span','spacer'));
      var sw = el('label','switch');
      var cb = el('input'); cb.type='checkbox'; cb.checked=v.enabled; cb.className='enable';
      cb.onchange = function(){ card.classList.toggle('open', cb.checked); };
      sw.appendChild(cb); sw.appendChild(el('span','slider'));
      head.appendChild(sw);
      card.appendChild(head);

      if(v.status) card.appendChild(el('div','status', v.status));

      var body = el('div','body');
      if(v.kind === 'oauth'){
        body.appendChild(el('div','hint', v.hint || ''));
      } else {
        var f1 = el('div','field');
        f1.appendChild(el('label','lbl','Environment variable'));
        var i1 = el('input'); i1.className='env'; i1.value = v.api_key_env || '';
        i1.placeholder = 'e.g. ' + v.id.toUpperCase() + '_API_KEY';
        f1.appendChild(i1); body.appendChild(f1);

        var f2 = el('div','field');
        f2.appendChild(el('label','lbl','API key (optional — overrides env var)'));
        var i2 = el('input'); i2.className='key'; i2.type='password';
        i2.value = v.api_key || ''; i2.placeholder = v.configured ? '•••••••• (set)' : 'paste key';
        f2.appendChild(i2); body.appendChild(f2);

        if(v.id === 'zai'){
          var f3 = el('div','field');
          f3.appendChild(el('label','lbl','Plan tier (display only)'));
          var i3 = el('input'); i3.className='tier'; i3.value = v.plan_tier || '';
          i3.placeholder = 'lite | pro | max';
          f3.appendChild(i3); body.appendChild(f3);
        }
      }
      card.appendChild(body);
      wrap.appendChild(card);
    });
  };

  function collect(){
    var cfg = {
      poll_seconds: parseInt(document.getElementById('poll').value, 10) || 60,
      ui: { primary: document.getElementById('primary').value },
      anthropic: {}, openai: {}, zai: {}, openrouter: {}, deepseek: {}
    };
    document.querySelectorAll('.vendor').forEach(function(card){
      var id = card.dataset.id;
      var o = cfg[id];
      o.enabled = card.querySelector('.enable').checked;
      var env = card.querySelector('.env'); if(env) o.api_key_env = env.value.trim();
      var key = card.querySelector('.key'); if(key) o.api_key = key.value;
      var tier = card.querySelector('.tier'); if(tier) o.plan_tier = tier.value.trim();
    });
    return cfg;
  }

  document.getElementById('save').onclick = function(){
    send({cmd:'save', config: collect()});
    var s = document.getElementById('saved'); s.classList.add('show');
    setTimeout(function(){ s.classList.remove('show'); }, 1600);
  };
  document.getElementById('cancel').onclick = function(){ send({cmd:'closeSettings'}); };
  window.addEventListener('keydown', function(e){ if(e.key==='Escape') send({cmd:'closeSettings'}); });
  send({cmd:'settingsReady'});
</script>
</body>
</html>"##;
