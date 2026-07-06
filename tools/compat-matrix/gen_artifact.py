#!/usr/bin/env python3
import json, os, re
HERE = os.path.dirname(__file__)
rows = [json.loads(l) for l in open(os.path.join(HERE, "macatrium-models.jsonl"))
        if l.strip() and not l.lstrip().startswith("#")]

# trim to the fields the page needs
keep = []
for r in rows:
    keep.append({
        "model": r["model"], "gid": r.get("gestaltID"), "mnum": r.get("modelNumber"),
        "code": r.get("codeName"), "arch": r["arch"], "group": r["group"],
        "year": (re.search(r"(19|20)\d\d", str(r.get("introduced") or "")) or [None])[0]
                 if re.search(r"(19|20)\d\d", str(r.get("introduced") or "")) else "",
        "min": r["minSystem"], "max": r["maxOS"],
        "mink": r.get("minKey") or 0, "maxk": r.get("maxKey") or 0,
        "env": bool(r["inEnvelope"]),
    })

total = len(keep)
n68 = sum(1 for r in keep if r["arch"] == "68K")
nppc = sum(1 for r in keep if r["arch"] == "PPC")
ngid = sum(1 for r in keep if r["gid"] is not None)
nbelow = sum(1 for r in keep if not r["env"])

HTML = r"""
<title>MacAtrium — Mac model → System matrix</title>
<style>
  :root{
    --bg:#eef0f2; --surface:#fff; --surface2:#f5f7f9; --ink:#1b1e23; --muted:#626a74;
    --hair:#d7dbe0; --accent:#2f6db0; --accent-weak:rgba(47,109,176,.10);
    --c68bg:#efe6d5; --c68fg:#7c5a1c; --cppcbg:#dbe9f3; --cppcfg:#1e5b85; --warn:#9c5d16;
    --shadow:0 1px 2px rgba(20,30,45,.06),0 1px 1px rgba(20,30,45,.04);
  }
  @media (prefers-color-scheme:dark){
    :root{
      --bg:#15171a; --surface:#1c1f24; --surface2:#22262c; --ink:#e7e9ec; --muted:#99a1aa;
      --hair:#2d323a; --accent:#6fa8e6; --accent-weak:rgba(111,168,230,.14);
      --c68bg:#332a1a; --c68fg:#d5ac67; --cppcbg:#153042; --cppcfg:#84c1e7; --warn:#d9a441;
      --shadow:0 1px 2px rgba(0,0,0,.3);
    }
  }
  :root[data-theme="light"]{
    --bg:#eef0f2; --surface:#fff; --surface2:#f5f7f9; --ink:#1b1e23; --muted:#626a74;
    --hair:#d7dbe0; --accent:#2f6db0; --accent-weak:rgba(47,109,176,.10);
    --c68bg:#efe6d5; --c68fg:#7c5a1c; --cppcbg:#dbe9f3; --cppcfg:#1e5b85; --warn:#9c5d16;
    --shadow:0 1px 2px rgba(20,30,45,.06),0 1px 1px rgba(20,30,45,.04);
  }
  :root[data-theme="dark"]{
    --bg:#15171a; --surface:#1c1f24; --surface2:#22262c; --ink:#e7e9ec; --muted:#99a1aa;
    --hair:#2d323a; --accent:#6fa8e6; --accent-weak:rgba(111,168,230,.14);
    --c68bg:#332a1a; --c68fg:#d5ac67; --cppcbg:#153042; --cppcfg:#84c1e7; --warn:#d9a441;
    --shadow:0 1px 2px rgba(0,0,0,.3);
  }
  *{box-sizing:border-box}
  body{margin:0;background:var(--bg);color:var(--ink);
    font-family:system-ui,-apple-system,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;
    line-height:1.5;-webkit-font-smoothing:antialiased;}
  .wrap{max-width:1120px;margin:0 auto;padding:34px 20px 80px;}
  .eyebrow{font-size:12px;font-weight:600;letter-spacing:.14em;text-transform:uppercase;
    color:var(--accent);margin:0 0 6px;}
  h1{font-size:clamp(26px,4vw,38px);font-weight:800;letter-spacing:-.02em;
    text-wrap:balance;margin:0 0 8px;}
  .lede{color:var(--muted);max-width:64ch;margin:0 0 26px;font-size:15px;}
  .lede code{background:var(--surface2);border:1px solid var(--hair);border-radius:5px;
    padding:.05em .35em;font-size:.9em;}

  .tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(140px,1fr));gap:12px;margin-bottom:26px;}
  .tile{background:var(--surface);border:1px solid var(--hair);border-radius:12px;
    padding:14px 16px;box-shadow:var(--shadow);}
  .tile .n{font-size:26px;font-weight:800;letter-spacing:-.02em;
    font-variant-numeric:tabular-nums;}
  .tile .k{font-size:11px;font-weight:600;letter-spacing:.08em;text-transform:uppercase;color:var(--muted);margin-top:2px;}
  .tile .sub{font-size:12px;color:var(--muted);margin-top:3px;}

  .bar{position:sticky;top:0;z-index:6;display:flex;flex-wrap:wrap;gap:10px;align-items:center;
    background:color-mix(in srgb,var(--bg) 86%,transparent);backdrop-filter:blur(8px);
    padding:12px 0;margin-bottom:6px;border-bottom:1px solid var(--hair);}
  .search{flex:1 1 220px;min-width:180px;}
  .search input{width:100%;padding:9px 12px;border:1px solid var(--hair);border-radius:9px;
    background:var(--surface);color:var(--ink);font-size:14px;}
  .search input:focus-visible{outline:2px solid var(--accent);outline-offset:1px;border-color:var(--accent);}
  .seg{display:inline-flex;background:var(--surface2);border:1px solid var(--hair);border-radius:9px;padding:3px;gap:2px;}
  .seg button{border:0;background:transparent;color:var(--muted);font:inherit;font-size:13px;font-weight:600;
    padding:6px 12px;border-radius:6px;cursor:pointer;}
  .seg button[aria-pressed="true"]{background:var(--surface);color:var(--ink);box-shadow:var(--shadow);}
  .seg button:focus-visible{outline:2px solid var(--accent);outline-offset:1px;}
  .toggle{display:inline-flex;align-items:center;gap:7px;font-size:13px;color:var(--muted);cursor:pointer;user-select:none;}
  .toggle input{accent-color:var(--accent);width:15px;height:15px;}
  .btn{border:1px solid var(--hair);background:var(--surface);color:var(--ink);font:inherit;font-size:13px;font-weight:600;
    padding:8px 13px;border-radius:9px;cursor:pointer;box-shadow:var(--shadow);}
  .btn:hover{border-color:var(--accent);color:var(--accent);}
  .btn:focus-visible{outline:2px solid var(--accent);outline-offset:1px;}

  .tablewrap{overflow-x:auto;border:1px solid var(--hair);border-radius:12px;background:var(--surface);box-shadow:var(--shadow);}
  table{border-collapse:collapse;width:100%;font-size:13.5px;}
  thead th{position:sticky;top:0;z-index:3;background:var(--surface2);text-align:left;
    font-size:11px;font-weight:700;letter-spacing:.05em;text-transform:uppercase;color:var(--muted);
    padding:10px 12px;white-space:nowrap;border-bottom:1px solid var(--hair);}
  thead th.sortable{cursor:pointer;}
  thead th.sortable:hover{color:var(--ink);}
  thead th .ar{opacity:0;margin-left:4px;font-size:10px;}
  thead th[aria-sort]:not([aria-sort="none"]) .ar{opacity:1;color:var(--accent);}
  th.num,td.num{text-align:right;font-variant-numeric:tabular-nums;
    font-family:ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace;}
  tbody td{padding:9px 12px;border-bottom:1px solid var(--hair);vertical-align:baseline;}
  tbody tr:last-child td{border-bottom:0;}
  tbody tr.data:hover td{background:var(--accent-weak);}
  td.model{font-weight:600;white-space:nowrap;}
  td.code,td.mnum{color:var(--muted);}
  .ver{font-family:ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace;font-variant-numeric:tabular-nums;white-space:nowrap;}
  .gid{font-family:ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace;color:var(--accent);font-weight:600;}
  .chip{display:inline-block;font-size:11px;font-weight:700;letter-spacing:.02em;padding:2px 8px;border-radius:20px;}
  .chip.a68{background:var(--c68bg);color:var(--c68fg);}
  .chip.appc{background:var(--cppcbg);color:var(--cppcfg);}
  tr.grp td{background:var(--surface2);position:sticky;top:37px;z-index:2;
    font-size:11px;font-weight:800;letter-spacing:.08em;text-transform:uppercase;color:var(--ink);
    padding:8px 12px;border-bottom:1px solid var(--hair);border-top:1px solid var(--hair);}
  tr.grp td .gc{color:var(--muted);font-weight:600;margin-left:6px;}
  tr.out td{color:var(--muted);}
  tr.out td.model{color:var(--warn);}
  .warnmark{color:var(--warn);font-weight:700;cursor:help;}
  .empty{padding:40px;text-align:center;color:var(--muted);}

  .foot{margin-top:26px;font-size:12.5px;color:var(--muted);}
  .foot h2{font-size:12px;letter-spacing:.08em;text-transform:uppercase;color:var(--ink);margin:0 0 8px;}
  .foot ul{margin:0;padding-left:18px;display:flex;flex-direction:column;gap:5px;}
  .foot b{color:var(--ink);font-weight:600;}
  @media (prefers-reduced-motion:no-preference){
    tbody tr.data{animation:fade .3s ease both;}
    @keyframes fade{from{opacity:0}to{opacity:1}}
  }
</style>

<div class="wrap">
  <p class="eyebrow">MacAtrium · hardware compatibility</p>
  <h1>Mac model → System matrix</h1>
  <p class="lede">Every Macintosh in MacAtrium's envelope (System <b>6.0.4</b>–<b>9.2.2</b>), keyed on
    <b>Gestalt&nbsp;ID</b> — the number the machine reports at runtime. <b>Min&nbsp;Sys</b> / <b>Max&nbsp;OS</b>
    are the range the <em>stock</em> machine boots. Sourced from LowEndMac profiles; sort or filter, then
    <code>Copy JSON</code> for the raw data.</p>

  <div class="tiles">
    <div class="tile"><div class="n">__TOTAL__</div><div class="k">Models</div></div>
    <div class="tile"><div class="n">__N68K__</div><div class="k">68K</div></div>
    <div class="tile"><div class="n">__NPPC__</div><div class="k">PowerPC</div><div class="sub">68k app under emulation</div></div>
    <div class="tile"><div class="n">__NGID__</div><div class="k">Numeric Gestalt ID</div><div class="sub">rest are New-World PPC</div></div>
    <div class="tile"><div class="n">6.0.4–9.2.2</div><div class="k">OS envelope</div><div class="sub">__NBELOW__ below floor</div></div>
  </div>

  <div class="bar">
    <div class="search"><input id="q" type="search" placeholder="Filter by model, code name, model #, Gestalt…" aria-label="Filter models"></div>
    <div class="seg" role="group" aria-label="Filter by CPU">
      <button data-arch="all" aria-pressed="true">All</button>
      <button data-arch="68K" aria-pressed="false">68K</button>
      <button data-arch="PPC" aria-pressed="false">PPC</button>
    </div>
    <label class="toggle"><input type="checkbox" id="envOnly"> Bootable only (≥6.0.4)</label>
    <button class="btn" id="copy">Copy JSON</button>
  </div>

  <div class="tablewrap">
    <table>
      <thead><tr>
        <th class="sortable" data-col="model" aria-sort="none">Model <span class="ar"></span></th>
        <th class="sortable num" data-col="gid" aria-sort="none">Gestalt <span class="ar"></span></th>
        <th>Model #</th>
        <th>Code name</th>
        <th>CPU</th>
        <th class="sortable" data-col="mink" aria-sort="none">Min Sys <span class="ar"></span></th>
        <th class="sortable" data-col="maxk" aria-sort="none">Max OS <span class="ar"></span></th>
        <th class="sortable num" data-col="year" aria-sort="none">Year <span class="ar"></span></th>
      </tr></thead>
      <tbody id="tb"></tbody>
    </table>
  </div>
  <div id="none" class="empty" hidden>No models match that filter.</div>

  <div class="foot">
    <h2>Notes</h2>
    <ul>
      <li><b>Gestalt ID is the runtime key, not unique per model.</b> A board family shares one ID — Performa rebadges reuse their sibling's ID; the 500-series PowerBooks are all 72; clones report their host Apple board (PowerCenter/PowerTower = 108, same as a Power Mac 7200).</li>
      <li><b>New-World PowerPC Macs report no numeric Gestalt ID</b> (iMac/iBook G3, B&amp;W G3, all G4) — they identify by a string model property instead. Those rows show “—”.</li>
      <li><b>Corrections applied over the raw scrape:</b> Mac IIsi Gestalt 10→18 (10 is the Portable); 68030 PowerBook Duos capped at 7.6.1 (LowEndMac lists 8.1, but Mac OS 8 needs a 68040+); Centris/Quadra 660AV set to 60. Clone clock-speed SKUs collapsed to one row per board family.</li>
      <li><b>Below floor:</b> the Macintosh 128K and 512K top out under System 6.0.4 and can't run MacAtrium; shown for completeness, flagged ⚠.</li>
    </ul>
  </div>
</div>

<script>
const ROWS = __DATA__;
const GORDER = ["Compact & Mac II","LC & all-in-one","Centris/Quadra/Performa","68K PowerBook/Duo","PPC desktop","PPC laptop / G3-G4","iMac/iBook G3","Clone"];
const state = {q:"", arch:"all", envOnly:false, col:null, dir:1};
const tb = document.getElementById("tb"), none = document.getElementById("none");
const esc = s => String(s==null?"":s).replace(/[&<>]/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;"}[c]));

function filtered(){
  const q = state.q.trim().toLowerCase();
  return ROWS.filter(r=>{
    if(state.arch!=="all" && r.arch!==state.arch) return false;
    if(state.envOnly && !r.env) return false;
    if(!q) return true;
    return [r.model,r.code,r.mnum,r.gid,r.min,r.max].some(v=>String(v==null?"":v).toLowerCase().includes(q));
  });
}
function cells(r){
  const chip = r.arch==="68K"?'<span class="chip a68">68K</span>':'<span class="chip appc">PPC</span>';
  const warn = r.env?"":' <span class="warnmark" title="Max OS is below System 6.0.4 — cannot run MacAtrium">⚠</span>';
  return `<td class="model">${esc(r.model)}${warn}</td>`+
    `<td class="num"><span class="gid">${r.gid==null?"—":r.gid}</span></td>`+
    `<td class="mnum">${esc(r.mnum)||"—"}</td>`+
    `<td class="code">${esc(r.code)||"—"}</td>`+
    `<td>${chip}</td>`+
    `<td><span class="ver">${esc(r.min)||"—"}</span></td>`+
    `<td><span class="ver">${esc(r.max)||"—"}</span></td>`+
    `<td class="num">${esc(r.year)||"—"}</td>`;
}
function render(){
  const data = filtered();
  none.hidden = data.length>0;
  let html = "";
  if(state.col){
    const s=[...data].sort((a,b)=>{
      let x=a[state.col], y=b[state.col];
      if(state.col==="model"){x=x.toLowerCase();y=y.toLowerCase();}
      if(state.col==="year"){x=+x||0;y=+y||0;}
      if(state.col==="gid"){x=x==null?-1:x;y=y==null?-1:y;}
      return (x<y?-1:x>y?1:0)*state.dir;
    });
    for(const r of s) html+=`<tr class="data${r.env?"":" out"}">${cells(r)}</tr>`;
  }else{
    for(const g of GORDER){
      const rs = data.filter(r=>r.group===g);
      if(!rs.length) continue;
      html+=`<tr class="grp"><td colspan="8">${esc(g)}<span class="gc">${rs.length}</span></td></tr>`;
      for(const r of rs) html+=`<tr class="data${r.env?"":" out"}">${cells(r)}</tr>`;
    }
  }
  tb.innerHTML = html;
}
document.getElementById("q").addEventListener("input",e=>{state.q=e.target.value;render();});
document.querySelectorAll(".seg button").forEach(b=>b.addEventListener("click",()=>{
  state.arch=b.dataset.arch;
  document.querySelectorAll(".seg button").forEach(x=>x.setAttribute("aria-pressed",x===b));
  render();
}));
document.getElementById("envOnly").addEventListener("change",e=>{state.envOnly=e.target.checked;render();});
document.querySelectorAll("th.sortable").forEach(th=>th.addEventListener("click",()=>{
  const c=th.dataset.col;
  if(state.col===c){ if(state.dir===1){state.dir=-1;} else {state.col=null;} }
  else {state.col=c;state.dir=1;}
  document.querySelectorAll("th.sortable").forEach(x=>x.setAttribute("aria-sort","none"));
  if(state.col) th.setAttribute("aria-sort",state.dir===1?"ascending":"descending");
  document.querySelectorAll("th .ar").forEach(a=>a.textContent="");
  if(state.col) th.querySelector(".ar").textContent = state.dir===1?"▲":"▼";
  render();
}));
document.getElementById("copy").addEventListener("click",async e=>{
  try{ await navigator.clipboard.writeText(JSON.stringify(ROWS,null,2));
    e.target.textContent="Copied ✓"; setTimeout(()=>e.target.textContent="Copy JSON",1400);
  }catch(_){ e.target.textContent="Copy failed"; }
});
render();
</script>
"""

HTML = (HTML.replace("__DATA__", json.dumps(keep, ensure_ascii=False))
            .replace("__TOTAL__", str(total)).replace("__N68K__", str(n68))
            .replace("__NPPC__", str(nppc)).replace("__NGID__", str(ngid))
            .replace("__NBELOW__", str(nbelow)))
open(os.path.join(HERE, "models-table.html"), "w").write(HTML)
print("wrote models-table.html", total, "models;", n68, "68K,", nppc, "PPC,", ngid, "with gid,", nbelow, "below floor")
