//! Inline, self-contained CSS + JS for the report and index pages.
//!
//! Everything is embedded (no CDN, no fetch, no build step — decision 13/14).

/// Shared stylesheet for report + index pages.
pub const STYLE: &str = r#"
:root { color-scheme: light dark; --fg:#1a1a1a; --bg:#fff; --muted:#666;
  --border:#ddd; --accent:#2563eb; --pass:#15803d; --fail:#b91c1c;
  --diff-bg:#fff3cd; --diff-fg:#7a5b00; --head:#f5f6f8; --stripe:#fafbfc; }
@media (prefers-color-scheme: dark) {
  :root { --fg:#e6e6e6; --bg:#111418; --muted:#9aa4b2; --border:#2a2f37;
    --accent:#60a5fa; --pass:#4ade80; --fail:#f87171; --diff-bg:#3a2f00;
    --diff-fg:#ffd764; --head:#1a1f26; --stripe:#151a20; } }
* { box-sizing:border-box; }
body { font:14px/1.5 system-ui,-apple-system,Segoe UI,Roboto,sans-serif;
  margin:0; color:var(--fg); background:var(--bg); }
header { padding:20px 24px; border-bottom:1px solid var(--border); }
h1 { margin:0 0 4px; font-size:20px; }
h2 { font-size:16px; margin:28px 0 10px; }
.sub { color:var(--muted); font-size:13px; }
main { padding:16px 24px 60px; }
.badge { display:inline-block; padding:2px 10px; border-radius:999px;
  font-weight:600; font-size:12px; }
.badge.pass { background:var(--pass); color:#fff; }
.badge.fail { background:var(--fail); color:#fff; }
.cards { display:flex; flex-wrap:wrap; gap:12px; margin:16px 0; }
.card { border:1px solid var(--border); border-radius:8px; padding:12px 16px;
  min-width:120px; }
.card .n { font-size:22px; font-weight:700; }
.card .l { color:var(--muted); font-size:12px; text-transform:uppercase;
  letter-spacing:.03em; }
.meta { display:grid; grid-template-columns:max-content 1fr; gap:4px 16px;
  font-size:13px; margin:12px 0; }
.meta dt { color:var(--muted); }
.meta dd { margin:0; font-family:ui-monospace,SFMono-Regular,Menlo,monospace; }
.tablewrap { overflow-x:auto; border:1px solid var(--border); border-radius:8px; }
table { border-collapse:collapse; width:100%; font-size:13px; }
th, td { text-align:left; padding:6px 10px; border-bottom:1px solid var(--border);
  white-space:nowrap; }
th { background:var(--head); position:sticky; top:0; cursor:pointer;
  user-select:none; }
th:hover { color:var(--accent); }
tbody tr:nth-child(even) { background:var(--stripe); }
td.diff, th.diff { background:var(--diff-bg); color:var(--diff-fg); font-weight:600; }
.filter { margin:8px 0; padding:6px 10px; width:100%; max-width:340px;
  border:1px solid var(--border); border-radius:6px; background:var(--bg);
  color:var(--fg); }
.empty { color:var(--muted); font-style:italic; padding:8px 0; }
.count { color:var(--muted); font-weight:400; font-size:13px; }
footer { padding:16px 24px; border-top:1px solid var(--border);
  color:var(--muted); font-size:12px; }
a { color:var(--accent); }
"#;

/// Report-page interactivity: per-table filter, sortable headers, changed-cell
/// highlight. Pure vanilla JS, reads the inline JSON data island.
pub const REPORT_JS: &str = r#"
(function () {
  var data = JSON.parse(document.getElementById("recon-data").textContent);
  var tables = data.tables;

  function pairedDiff(cols) {
    // Map column index -> its partner index for base__a / base__b pairs.
    var map = {};
    var byName = {};
    cols.forEach(function (c, i) { byName[c] = i; });
    cols.forEach(function (c, i) {
      if (c.endsWith("__a")) {
        var b = c.slice(0, -3) + "__b";
        if (b in byName) { map[i] = byName[b]; map[byName[b]] = i; }
      }
    });
    return map;
  }

  function render(id, tbl) {
    var host = document.getElementById(id);
    if (!tbl || tbl.rows.length === 0) {
      host.innerHTML = '<p class="empty">No rows in this category.</p>';
      return;
    }
    var diffMap = pairedDiff(tbl.columns);
    var wrap = document.createElement("div");
    wrap.className = "tablewrap";
    var t = document.createElement("table");
    var thead = document.createElement("thead");
    var htr = document.createElement("tr");
    tbl.columns.forEach(function (c, i) {
      var th = document.createElement("th");
      th.textContent = c;
      th.addEventListener("click", function () { sortBy(t, i); });
      htr.appendChild(th);
    });
    thead.appendChild(htr);
    t.appendChild(thead);
    var tb = document.createElement("tbody");
    tbl.rows.forEach(function (r) {
      var tr = document.createElement("tr");
      r.forEach(function (cell, i) {
        var td = document.createElement("td");
        td.textContent = cell === null ? "∅" : cell;
        if (i in diffMap) {
          var other = r[diffMap[i]];
          if ((cell || "") !== (other || "")) td.className = "diff";
        }
        tr.appendChild(td);
      });
      tb.appendChild(tr);
    });
    t.appendChild(tb);
    wrap.appendChild(t);
    host.innerHTML = "";
    var filter = document.createElement("input");
    filter.className = "filter";
    filter.placeholder = "Filter rows…";
    filter.addEventListener("input", function () {
      var q = filter.value.toLowerCase();
      Array.prototype.forEach.call(tb.rows, function (row) {
        row.style.display = row.textContent.toLowerCase().indexOf(q) >= 0 ? "" : "none";
      });
    });
    host.appendChild(filter);
    host.appendChild(wrap);
  }

  function sortBy(table, col) {
    var tb = table.tBodies[0];
    var rows = Array.prototype.slice.call(tb.rows);
    var asc = table.getAttribute("data-sort-col") == col
      ? table.getAttribute("data-sort-dir") !== "asc" : true;
    rows.sort(function (a, b) {
      var x = a.cells[col].textContent, y = b.cells[col].textContent;
      var nx = parseFloat(x), ny = parseFloat(y);
      if (!isNaN(nx) && !isNaN(ny)) return asc ? nx - ny : ny - nx;
      return asc ? x.localeCompare(y) : y.localeCompare(x);
    });
    rows.forEach(function (r) { tb.appendChild(r); });
    table.setAttribute("data-sort-col", col);
    table.setAttribute("data-sort-dir", asc ? "asc" : "desc");
  }

  render("t-only-a", tables.only_in_a);
  render("t-only-b", tables.only_in_b);
  render("t-changed", tables.changed);
  render("t-duplicates", tables.duplicates);
})();
"#;

/// Index-page interactivity: filter + sortable columns over the run history.
pub const INDEX_JS: &str = r#"
(function () {
  var table = document.getElementById("runs");
  if (!table) return;
  var tb = table.tBodies[0];
  var filter = document.getElementById("q");
  if (filter) filter.addEventListener("input", function () {
    var q = filter.value.toLowerCase();
    Array.prototype.forEach.call(tb.rows, function (row) {
      row.style.display = row.textContent.toLowerCase().indexOf(q) >= 0 ? "" : "none";
    });
  });
  Array.prototype.forEach.call(table.tHead.rows[0].cells, function (th, i) {
    th.addEventListener("click", function () {
      var rows = Array.prototype.slice.call(tb.rows);
      var asc = table.getAttribute("data-sc") == i
        ? table.getAttribute("data-sd") !== "asc" : true;
      rows.sort(function (a, b) {
        var x = a.cells[i].getAttribute("data-v") || a.cells[i].textContent;
        var y = b.cells[i].getAttribute("data-v") || b.cells[i].textContent;
        var nx = parseFloat(x), ny = parseFloat(y);
        if (!isNaN(nx) && !isNaN(ny)) return asc ? nx - ny : ny - nx;
        return asc ? x.localeCompare(y) : y.localeCompare(x);
      });
      rows.forEach(function (r) { tb.appendChild(r); });
      table.setAttribute("data-sc", i);
      table.setAttribute("data-sd", asc ? "asc" : "desc");
    });
  });
})();
"#;
