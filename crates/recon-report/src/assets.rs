//! Inline, self-contained CSS + JS for the report and index pages.
//!
//! Everything is embedded (no CDN, no fetch, no build step — decision 13/14).

/// Shared stylesheet for report + index pages.
pub const STYLE: &str = r#"
:root {
  color-scheme: light;
  --bg:#f0eee6; --surface:#faf9f5; --surface-2:#f4f2ea;
  --fg:#191917; --muted:#73706a; --border:#e4e1d7; --head:#f4f2ea;
  --accent:#e8552b; --accent-hover:#cf451d; --accent-soft:#fce1d6;
  --pass:#12a150; --pass-soft:#d2f4df; --fail:#e5341f; --fail-soft:#fcdcd5;
  --diff-bg:#ffe6a3; --diff-fg:#9a6a00;
  --shadow-sm:0 1px 2px rgba(25,25,23,.05);
  --shadow-md:0 4px 16px rgba(25,25,23,.06),0 1px 3px rgba(25,25,23,.04);
  --font-display:ui-serif,Georgia,"Times New Roman",serif;
}
* { box-sizing:border-box; }
body { margin:0; color:var(--fg); background:var(--bg); font:14px/1.5 Inter,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif; -webkit-font-smoothing:antialiased; }
a { color:var(--accent); font-weight:600; text-decoration:none; }
a:hover { color:var(--accent-hover); text-decoration:underline; }
.topbar { position:sticky; top:0; z-index:5; border-bottom:1px solid var(--border); background:color-mix(in srgb,var(--head) 88%,transparent); backdrop-filter:blur(12px); }
.topbar-inner, main, .footer-inner { width:min(100% - 48px,1024px); margin:0 auto; }
.topbar-inner { display:flex; align-items:center; gap:10px; min-height:57px; }
.brand-mark { display:grid; width:28px; height:28px; place-items:center; border-radius:8px; color:white; background:var(--accent); box-shadow:var(--shadow-sm); font-size:17px; }
.brand { font-family:var(--font-display); font-size:17px; font-weight:600; letter-spacing:-.01em; }
.nav-label { margin-left:auto; color:var(--muted); font-size:13px; font-weight:600; }
main { padding:34px 0 64px; }
.report-header { display:flex; align-items:start; justify-content:space-between; gap:20px; margin-bottom:24px; }
h1,h2 { font-family:var(--font-display); letter-spacing:-.01em; }
h1 { margin:0 0 4px; font-size:30px; line-height:1.15; font-weight:600; }
h2 { margin:34px 0 10px; font-size:20px; line-height:1.25; }
.sub { color:var(--muted); font-size:13px; }
.badge { display:inline-flex; align-items:center; border-radius:999px; padding:3px 10px; font-size:12px; font-weight:800; letter-spacing:.02em; vertical-align:middle; }
.badge.pass { color:var(--pass); background:var(--pass-soft); }
.badge.fail { color:var(--fail); background:var(--fail-soft); }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(130px,1fr)); gap:12px; margin:18px 0 16px; }
.card { min-width:0; border:1px solid var(--border); border-radius:12px; padding:14px 16px; background:var(--surface); box-shadow:var(--shadow-sm); }
.card .n { font-size:24px; line-height:1.2; font-weight:700; letter-spacing:-.03em; }
.card .l { margin-top:4px; color:var(--muted); font-size:11px; font-weight:700; letter-spacing:.06em; text-transform:uppercase; }
.meta { display:grid; grid-template-columns:minmax(130px,max-content) minmax(0,1fr); gap:8px 20px; margin:0; padding:18px; border:1px solid var(--border); border-radius:12px; background:var(--surface); box-shadow:var(--shadow-sm); font-size:13px; }
.meta dt { color:var(--muted); font-weight:600; }
.meta dd { min-width:0; margin:0; overflow-wrap:anywhere; font-family:ui-monospace,SFMono-Regular,Menlo,monospace; font-size:12px; }
.section-heading { display:flex; align-items:baseline; gap:7px; }
.count { color:var(--muted); font-family:Inter,system-ui,sans-serif; font-size:13px; font-weight:500; letter-spacing:0; }
.tablewrap { overflow-x:auto; border:1px solid var(--border); border-radius:12px; background:var(--surface); box-shadow:var(--shadow-md); }
table { width:100%; border-collapse:collapse; font-size:13px; }
th,td { padding:10px 12px; border-bottom:1px solid var(--border); text-align:left; white-space:nowrap; }
th { position:sticky; top:57px; background:var(--surface-2); color:var(--muted); cursor:pointer; font-size:11px; font-weight:700; letter-spacing:.06em; text-transform:uppercase; user-select:none; }
th:hover { color:var(--accent); }
tbody tr { transition:background .12s ease; }
tbody tr:hover { background:var(--surface-2); }
tbody tr:last-child td { border-bottom:0; }
td.diff,th.diff { color:var(--diff-fg); background:var(--diff-bg); font-weight:700; }
.filter { display:block; width:min(100%,340px); margin:10px 0; border:1px solid var(--border); border-radius:8px; padding:9px 11px; color:var(--fg); background:var(--surface); box-shadow:var(--shadow-sm); font:inherit; }
.filter:focus { outline:0; border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
.empty { margin:0; padding:12px 0; color:var(--muted); font-style:italic; }
footer { border-top:1px solid var(--border); color:var(--muted); font-size:12px; }
.footer-inner { padding:18px 0 24px; }
.footer-inner p { margin:4px 0; }
@media (max-width:600px) { .topbar-inner,main,.footer-inner { width:min(100% - 32px,1024px); } main { padding-top:24px; } .report-header { display:block; } .badge { margin-top:8px; } .meta { grid-template-columns:1fr; gap:2px; } .meta dd { margin-bottom:8px; } th { top:57px; } }
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
