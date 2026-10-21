import { useEffect, useMemo, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Archive,
  Boxes,
  Check,
  Download,
  Eraser,
  FolderOpen,
  LoaderCircle,
  Package,
  Power,
  Radar,
  RotateCcw,
  Search,
  ShieldAlert,
  ShieldCheck,
  ShoppingBag,
  Trash2,
  TriangleAlert,
  Zap,
} from "lucide-react";
import LightBar, { Backdrop, type Rgb } from "./Shader";
import logo from "./assets/logo.svg";
import { locale, t } from "./i18n";
import "./App.css";

type Program = {
  id: string;
  name: string;
  publisher: string;
  version: string;
  installDate: string;
  sizeKb: number;
  installLocation: string;
  uninstallString: string;
  quietUninstallString: string;
  icon: string;
  broken: boolean;
};
type Leftover = { kind: "file" | "registry"; path: string; sizeBytes: number };
type ScanResult = { stillInstalled: boolean; items: Leftover[] };
type Outcome = { done: number; errors: string[] };
type Session = {
  id: string;
  program: string;
  created: number;
  items: { kind: "file" | "registry"; original: string; sizeBytes: number }[];
};
type StartupItem = {
  name: string;
  command: string;
  exe: string;
  location: string;
  machineWide: boolean;
  enabled: boolean;
  approvedKey: string;
};
type StoreApp = { name: string; fullName: string; publisher: string; version: string; logo: string };
type JunkGroup = { id: string; path: string; sizeBytes: number };
type Trace = { id: string; name: string; created: number; items: Leftover[] };

type Step =
  | { kind: "running" }
  | { kind: "scanning"; note?: string }
  | { kind: "review"; scan: ScanResult; picked: Set<string>; note?: string; force: boolean }
  | { kind: "moving"; permanent: boolean }
  | { kind: "done"; outcome: Outcome; permanent: boolean };

type View = "programs" | "store" | "startup" | "cleaner" | "tracker" | "quarantine";
type Sort = "name" | "size" | "date";

const LIGHTS: Record<string, string> = { lunar: "#4fc8ff", plasma: "#a685ff", ember: "#ff9a3c", arctic: "#e4ecf6" };
const DANGER: Rgb = [1, 0.27, 0.23];
const rgb = (hex: string): Rgb => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255) as Rgb;

const nf = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
function fmtSize(bytes: number) {
  if (!bytes) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (bytes >= 1024 && i < u.length - 1) (bytes /= 1024), i++;
  return `${nf.format(i > 0 && bytes < 10 ? Math.round(bytes * 10) / 10 : Math.round(bytes))} ${u[i]}`;
}
function fmtDate(yyyymmdd: string) {
  const m = /^(\d{4})(\d{2})(\d{2})$/.exec(yyyymmdd);
  return m ? new Date(+m[1], +m[2] - 1, +m[3]).toLocaleDateString(locale) : "—";
}
const when = (ms: number) => new Date(ms).toLocaleString(locale, { dateStyle: "medium", timeStyle: "short" });

function readPref(key: string, fallback: string) {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}
function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* preference only */
  }
}

function AppIcon({ name, src, size = 34 }: { name: string; src?: string; size?: number }) {
  const letters = name
    .replace(/[^\p{L}\p{N} ]/gu, "")
    .split(" ")
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0])
    .join("");
  return (
    <span className={`appicon ${src ? "has-img" : ""}`} style={{ "--s": `${size}px` } as CSSProperties} aria-hidden>
      {src ? <img src={`data:image/png;base64,${src}`} alt="" /> : letters.toUpperCase() || "?"}
    </span>
  );
}

function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button role="switch" aria-checked={on} aria-label={label} className={`toggle ${on ? "on" : ""}`} onClick={() => onChange(!on)}>
      <span />
    </button>
  );
}

function Empty({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <div className="empty">
      {icon}
      <p>{children}</p>
    </div>
  );
}

export default function App() {
  const [view, setView] = useState<View>("programs");
  const [light, setLight] = useState(() => readPref("purge.light", "lunar"));
  const [programs, setPrograms] = useState<Program[] | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [startup, setStartup] = useState<StartupItem[]>([]);
  const [store, setStore] = useState<StoreApp[] | null>(null);
  const [junk, setJunk] = useState<JunkGroup[] | null>(null);
  const [junkPicked, setJunkPicked] = useState<Set<string>>(new Set());
  const [traces, setTraces] = useState<Trace[]>([]);
  const [trackingSince, setTrackingSince] = useState<number | null>(null);
  const [traceDraft, setTraceDraft] = useState<{ items: Leftover[]; picked: Set<string>; name: string } | null>(null);
  const [icons, setIcons] = useState<Record<string, string>>({});
  const [admin, setAdmin] = useState(true);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("name");
  const [onlyBroken, setOnlyBroken] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [queue, setQueue] = useState<Program[]>([]);
  const [index, setIndex] = useState(0);
  const [step, setStep] = useState<Step | null>(null);
  const [working, setWorking] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const loadIcons = (sources: string[]) => {
    const wanted = [...new Set(sources.filter(Boolean))];
    if (wanted.length)
      invoke<Record<string, string>>("program_icons", { sources: wanted }).then((m) => setIcons((o) => ({ ...o, ...m })));
  };

  const reload = async () => {
    const list = await invoke<Program[]>("list_programs");
    setPrograms(list);
    setSessions(await invoke<Session[]>("list_quarantine"));
    loadIcons(list.map((p) => p.icon));
  };
  const reloadStartup = async () => {
    const list = await invoke<StartupItem[]>("list_startup");
    setStartup(list);
    loadIcons(list.map((s) => s.exe));
  };
  const reloadStore = async () => {
    setStore(null);
    try {
      setStore(await invoke<StoreApp[]>("list_store_apps"));
    } catch (e) {
      setStore([]);
      setToast(String(e));
    }
  };
  const reloadJunk = async () => {
    setJunk(null);
    const groups = await invoke<JunkGroup[]>("scan_junk");
    setJunk(groups);
    setJunkPicked(new Set(groups.filter((g) => g.sizeBytes > 0).map((g) => g.id)));
  };
  const reloadTracker = async () => {
    setTraces(await invoke<Trace[]>("list_traces"));
    setTrackingSince(await invoke<number | null>("tracker_started"));
  };

  useEffect(() => {
    reload();
    reloadStartup();
    reloadTracker();
    invoke<boolean>("is_admin").then(setAdmin);
  }, []);

  // Lazy-load the slower views the first time they're opened
  useEffect(() => {
    if (view === "store" && store === null) reloadStore();
    if (view === "cleaner" && junk === null) reloadJunk();
  }, [view]);

  useEffect(() => {
    if (!toast) return;
    const id = setTimeout(() => setToast(null), 4500);
    return () => clearTimeout(id);
  }, [toast]);

  useEffect(() => writePref("purge.light", light), [light]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = (programs ?? []).filter(
      (p) => (!onlyBroken || p.broken) && (!q || p.name.toLowerCase().includes(q) || p.publisher.toLowerCase().includes(q)),
    );
    if (sort === "size") return [...list].sort((a, b) => b.sizeKb - a.sizeKb);
    if (sort === "date") return [...list].sort((a, b) => b.installDate.localeCompare(a.installDate));
    return list;
  }, [programs, query, sort, onlyBroken]);

  const totalKb = (programs ?? []).reduce((s, p) => s + p.sizeKb, 0);
  const brokenCount = (programs ?? []).filter((p) => p.broken).length;
  const selectedPrograms = (programs ?? []).filter((p) => selected.has(p.id));
  const selectedKb = selectedPrograms.reduce((s, p) => s + p.sizeKb, 0);
  const quarantineBytes = sessions.reduce((a, s) => a + s.items.reduce((b, i) => b + i.sizeBytes, 0), 0);
  const startupOn = startup.filter((s) => s.enabled).length;

  // Capacity strip: the biggest programs as blocks, the rest folded into one
  const capacity = useMemo(() => {
    const sized = [...(programs ?? [])].filter((p) => p.sizeKb > 0).sort((a, b) => b.sizeKb - a.sizeKb);
    const top = sized.slice(0, 6);
    const rest = sized.slice(6);
    return { top, rest, restKb: rest.reduce((s, p) => s + p.sizeKb, 0) };
  }, [programs]);

  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });

  const toggleIn = (set: Set<string>, key: string) => {
    const n = new Set(set);
    n.has(key) ? n.delete(key) : n.add(key);
    return n;
  };

  // --- uninstall flow -------------------------------------------------------
  const scan = async (p: Program, note?: string, force = false) => {
    setStep({ kind: "scanning", note });
    const res = await invoke<ScanResult>("scan_leftovers", { program: p });
    // Still installed => nothing pre-selected, unless the user explicitly forced it.
    const picked = new Set(res.stillInstalled && !force ? [] : res.items.map((i) => i.path));
    setStep({ kind: "review", scan: res, picked, note, force });
  };

  const runFor = async (p: Program) => {
    setStep({ kind: "running" });
    let note: string | undefined;
    try {
      const code = await invoke<number>("uninstall", { program: p });
      if (code !== 0) note = t.exitCode(code);
    } catch (e) {
      note = t.cantStart(String(e));
    }
    await scan(p, note);
  };

  const start = (list: Program[]) => {
    if (!list.length) return;
    setQueue(list);
    setIndex(0);
    runFor(list[0]);
  };

  const forceRemove = (p: Program) => {
    if (!confirm(t.confirmForce(p.name))) return;
    setQueue([p]);
    setIndex(0);
    scan(p, t.forceNote, true);
  };

  const next = async () => {
    const i = index + 1;
    if (i < queue.length) {
      setIndex(i);
      runFor(queue[i]);
    } else {
      setStep(null);
      setQueue([]);
      setSelected(new Set());
      await reload();
    }
  };

  const removePicked = async (permanent: boolean) => {
    if (step?.kind !== "review") return;
    const items = step.scan.items.filter((i) => step.picked.has(i.path));
    if (!items.length) return next();
    if (permanent && !confirm(t.confirmDelete(items.length))) return;
    setStep({ kind: "moving", permanent });
    try {
      const cmd = permanent ? "delete_items" : "quarantine_items";
      const outcome = await invoke<Outcome>(cmd, { program: queue[index].name, items });
      setStep({ kind: "done", outcome, permanent });
    } catch (e) {
      setStep({ kind: "done", outcome: { done: 0, errors: [String(e)] }, permanent });
    }
  };

  const openFolder = (path: string) => invoke("open_folder", { path }).catch((e) => setToast(String(e)));

  const exportCsv = () => {
    const esc = (v: string) => `"${v.replace(/"/g, '""')}"`;
    const rows = [
      [t.colName, "Publisher", t.colVersion, "Size (KB)", t.colInstalled, "Location"],
      ...(programs ?? []).map((p) => [p.name, p.publisher, p.version, String(p.sizeKb), p.installDate, p.installLocation]),
    ];
    const blob = new Blob(["﻿" + rows.map((r) => r.map(esc).join(",")).join("\r\n")], { type: "text/csv" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `purge-programs-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  // --- other views ----------------------------------------------------------
  const setStartupItem = async (item: StartupItem, enabled: boolean) => {
    try {
      await invoke("set_startup", { approvedKey: item.approvedKey, name: item.name, enabled });
      setStartup((list) => list.map((s) => (s === item ? { ...s, enabled } : s)));
    } catch (e) {
      setToast(item.machineWide && !admin ? t.needAdmin : String(e));
    }
  };

  const removeStoreApp = async (app: StoreApp) => {
    if (!confirm(t.confirmRemoveStore(app.name))) return;
    setWorking(app.fullName);
    try {
      await invoke("remove_store_app", { fullName: app.fullName });
      setStore((s) => s?.filter((a) => a !== app) ?? null);
      setToast(t.removed(app.name));
    } catch (e) {
      setToast(String(e));
    } finally {
      setWorking(null);
    }
  };

  const cleanJunk = async () => {
    setWorking("junk");
    try {
      const freed = await invoke<number>("clean_junk", { ids: [...junkPicked] });
      setToast(t.freed(fmtSize(freed)));
      await reloadJunk();
    } finally {
      setWorking(null);
    }
  };

  const trackerStart = async () => {
    setWorking("tracker");
    try {
      await invoke("tracker_start");
      await reloadTracker();
    } catch (e) {
      setToast(String(e));
    } finally {
      setWorking(null);
    }
  };
  const trackerFinish = async () => {
    setWorking("tracker");
    try {
      const items = await invoke<Leftover[]>("tracker_finish");
      setTraceDraft({ items, picked: new Set(items.map((i) => i.path)), name: "" });
      setTrackingSince(null);
    } catch (e) {
      setToast(String(e));
    } finally {
      setWorking(null);
    }
  };
  const trackerCancel = async () => {
    await invoke("tracker_cancel");
    setTrackingSince(null);
  };
  const saveTrace = async () => {
    if (!traceDraft) return;
    const items = traceDraft.items.filter((i) => traceDraft.picked.has(i.path));
    await invoke("save_trace", { name: traceDraft.name || t.tracker, items });
    setTraceDraft(null);
    await reloadTracker();
  };
  const removeTrace = async (tr: Trace) => {
    if (!confirm(t.confirmRemoveTrace(tr.name, tr.items.length))) return;
    setWorking(tr.id);
    try {
      const o = await invoke<Outcome>("quarantine_items", { program: tr.name, items: tr.items });
      await invoke("delete_trace", { id: tr.id });
      setToast(o.errors.length ? t.restoredPartial(o.done, o.errors.length) : t.doneMoved(o.done));
      await Promise.all([reloadTracker(), reload()]);
    } finally {
      setWorking(null);
    }
  };

  const restore = async (s: Session) => {
    const o = await invoke<Outcome>("restore_session", { id: s.id });
    setToast(o.errors.length ? t.restoredPartial(o.done, o.errors.length) : t.restored(s.program));
    await reload();
  };
  const purgeSession = async (s: Session) => {
    if (!confirm(t.confirmDeleteBackup(s.program))) return;
    await invoke("delete_session", { id: s.id });
    await reload();
  };

  // --- light bar state ------------------------------------------------------
  const deleting = step?.kind === "moving" && step.permanent;
  const busy =
    !!working ||
    (!!step && step.kind !== "review" && step.kind !== "done") ||
    programs === null ||
    (view === "store" && store === null) ||
    (view === "cleaner" && junk === null);
  const lightRgb = deleting ? DANGER : rgb(LIGHTS[light] ?? LIGHTS.lunar);

  const current = queue[index];
  const stepIndex = !step ? 0 : step.kind === "running" ? 0 : step.kind === "scanning" ? 1 : 2;

  const nav: { id: View; label: string; icon: ReactNode; count?: number }[] = [
    { id: "programs", label: t.programs, icon: <Boxes size={16} />, count: programs?.length },
    { id: "store", label: t.storeApps, icon: <ShoppingBag size={16} />, count: store?.length },
    { id: "startup", label: t.startup, icon: <Power size={16} />, count: startupOn },
    { id: "cleaner", label: t.cleaner, icon: <Eraser size={16} /> },
    { id: "tracker", label: t.tracker, icon: <Radar size={16} />, count: traces.length || undefined },
    { id: "quarantine", label: t.quarantine, icon: <Archive size={16} />, count: sessions.length || undefined },
  ];

  const junkTotal = (junk ?? []).filter((g) => junkPicked.has(g.id)).reduce((a, g) => a + g.sizeBytes, 0);

  return (
    <div className="shell" style={{ "--light": deleting ? "#ff453a" : LIGHTS[light] } as CSSProperties}>
      <Backdrop color={lightRgb} />
      <header className="top">
        <div className="brand">
          <img src={logo} alt="" />
          <span>Purge</span>
        </div>
        <nav aria-label="Sections">
          {nav.map((n) => (
            <button key={n.id} className={view === n.id ? "on" : ""} aria-current={view === n.id ? "page" : undefined} onClick={() => setView(n.id)}>
              {n.icon}
              {n.label}
              {n.count !== undefined && <span className="count">{n.count}</span>}
            </button>
          ))}
        </nav>
        <div className="top-right">
          <div className="swatches" role="radiogroup" aria-label={t.lightColor}>
            {Object.entries(LIGHTS).map(([id, hex]) => (
              <button
                key={id}
                role="radio"
                aria-checked={light === id}
                aria-label={t.lights[id]}
                title={t.lights[id]}
                className={light === id ? "on" : ""}
                style={{ "--sw": hex } as CSSProperties}
                onClick={() => setLight(id)}
              />
            ))}
          </div>
          <span className={`admin ${admin ? "" : "warn"}`} title={admin ? t.admin : t.limited}>
            {admin ? <ShieldCheck size={16} /> : <ShieldAlert size={16} />}
            {admin ? t.admin : t.limited.split(" — ")[0]}
          </span>
        </div>
      </header>
      <LightBar color={lightRgb} busy={busy} />

      <main>
        {view === "programs" && (
          <section className="view">
            <div className="hero">
              <h1>{programs ? t.heroPrograms(fmtSize(totalKb * 1024), programs.length) : t.readingRegistry}</h1>
              {programs && capacity.top.length > 0 && (
                <>
                  <h2 className="section-title">{t.biggest}</h2>
                  <div className="capacity" role="group" aria-label={t.capacityHint}>
                    {capacity.top.map((p) => (
                      <button
                        key={p.id}
                        className={selected.has(p.id) ? "lit" : ""}
                        style={{ flexGrow: p.sizeKb }}
                        title={`${p.name} · ${fmtSize(p.sizeKb * 1024)}`}
                        aria-pressed={selected.has(p.id)}
                        onClick={() => toggle(p.id)}
                      >
                        <AppIcon name={p.name} src={icons[p.icon]} size={26} />
                        <span className="cap-text">
                          <b>{p.name}</b>
                          <small>{fmtSize(p.sizeKb * 1024)}</small>
                        </span>
                      </button>
                    ))}
                    {capacity.restKb > 0 && (
                      <span className="rest" style={{ flexGrow: capacity.restKb }}>
                        <span className="cap-text">
                          <b>{t.other(capacity.rest.length)}</b>
                          <small>{fmtSize(capacity.restKb * 1024)}</small>
                        </span>
                      </span>
                    )}
                  </div>
                  <p className="hint">{t.capacityHint}</p>
                </>
              )}
            </div>

            <div className="toolbar">
              <label className="search">
                <Search size={16} />
                <input type="search" placeholder={t.searchPlaceholder} value={query} onChange={(e) => setQuery(e.target.value)} aria-label={t.searchLabel} />
              </label>
              <div className="segmented" role="radiogroup" aria-label={t.sortBy}>
                {(["name", "size", "date"] as Sort[]).map((s) => (
                  <button key={s} role="radio" aria-checked={sort === s} className={sort === s ? "on" : ""} onClick={() => setSort(s)}>
                    {s === "name" ? t.sortName : s === "size" ? t.sortSize : t.sortDate}
                  </button>
                ))}
              </div>
              {brokenCount > 0 && (
                <button className={`chip-btn ${onlyBroken ? "on" : ""}`} aria-pressed={onlyBroken} onClick={() => setOnlyBroken(!onlyBroken)}>
                  <TriangleAlert size={14} /> {t.brokenFilter(brokenCount)}
                </button>
              )}
              <span className="spacer" />
              <button className="key" onClick={exportCsv} disabled={!programs}>
                <Download size={15} /> {t.exportCsv}
              </button>
            </div>

            <div className="list" role="list">
              <div className="row head" aria-hidden>
                <span />
                <span>{t.colName}</span>
                <span>{t.colVersion}</span>
                <span className="num">{t.colSize}</span>
                <span className="num">{t.colInstalled}</span>
                <span />
              </div>
              {programs === null && <Empty icon={<LoaderCircle className="spin" size={22} />}>{t.readingRegistry}</Empty>}
              {programs && !visible.length && <Empty icon={<Search size={22} />}>{t.noMatch(query)}</Empty>}
              {visible.map((p) => (
                <div key={p.id} className={`row ${selected.has(p.id) ? "sel" : ""}`} role="listitem">
                  <input type="checkbox" className="check" checked={selected.has(p.id)} onChange={() => toggle(p.id)} aria-label={t.select(p.name)} />
                  <span className="name" onClick={() => toggle(p.id)}>
                    <AppIcon name={p.name} src={icons[p.icon]} />
                    <span>
                      <strong>
                        {p.name}
                        {p.broken && (
                          <em className="flag" title={t.brokenHint}>
                            {t.broken}
                          </em>
                        )}
                      </strong>
                      <small>{p.publisher || t.unknownPublisher}</small>
                    </span>
                  </span>
                  <span className="dim ellipsis">{p.version || "—"}</span>
                  <span className="num">{fmtSize(p.sizeKb * 1024)}</span>
                  <span className="num dim">{fmtDate(p.installDate)}</span>
                  <span className="actions">
                    {p.installLocation && (
                      <button className="icon-btn" title={t.openFolder} aria-label={t.openFolder} onClick={() => openFolder(p.installLocation)}>
                        <FolderOpen size={16} />
                      </button>
                    )}
                    <button className="icon-btn" title={t.forceRemove} aria-label={t.forceRemove} onClick={() => forceRemove(p)}>
                      <Zap size={16} />
                    </button>
                    <button className="icon-btn danger" title={t.uninstall} aria-label={t.uninstall} onClick={() => start([p])}>
                      <Trash2 size={16} />
                    </button>
                  </span>
                </div>
              ))}
            </div>

            <div className={`dock ${selected.size ? "show" : ""}`} aria-hidden={!selected.size}>
              <span>
                <strong>{selected.size}</strong> {t.selected} <span className="dim">{fmtSize(selectedKb * 1024)}</span>
              </span>
              <span className="spacer" />
              <button className="key" onClick={() => setSelected(new Set())}>
                {t.clear}
              </button>
              <button className="key danger" onClick={() => start(selectedPrograms)}>
                <Trash2 size={15} /> {t.uninstall}
              </button>
            </div>
          </section>
        )}

        {view === "store" && (
          <section className="view">
            <div className="hero">
              <h1>{t.storeApps}</h1>
              {store && <p className="lede">{t.storeDesc(store.length)}</p>}
            </div>
            <div className="list">
              {store === null && <Empty icon={<LoaderCircle className="spin" size={22} />}>{t.storeLoading}</Empty>}
              {store?.length === 0 && <Empty icon={<ShoppingBag size={22} />}>{t.storeEmpty}</Empty>}
              {store?.map((a) => (
                <div key={a.fullName} className="row simple">
                  <span className="name">
                    <AppIcon name={a.name} src={a.logo || undefined} />
                    <span>
                      <strong>{a.name}</strong>
                      <small>{a.publisher || t.unknownPublisher}</small>
                    </span>
                  </span>
                  <span className="dim ellipsis">{a.version}</span>
                  <button className="key" disabled={working === a.fullName} onClick={() => removeStoreApp(a)}>
                    {working === a.fullName ? <LoaderCircle className="spin" size={15} /> : <Trash2 size={15} />} {t.remove}
                  </button>
                </div>
              ))}
            </div>
          </section>
        )}

        {view === "startup" && (
          <section className="view">
            <div className="hero">
              <h1>{t.startup}</h1>
              <p className="lede">{t.startupDesc(startupOn, startup.length)}</p>
            </div>
            <div className="list">
              {!startup.length && <Empty icon={<Power size={22} />}>{t.noStartup}</Empty>}
              {startup.map((s) => (
                <div key={s.approvedKey + s.name} className={`row simple ${s.enabled ? "" : "off"}`}>
                  <span className="name">
                    <AppIcon name={s.name} src={icons[s.exe]} />
                    <span>
                      <strong>{s.name.replace(/\.lnk$/i, "")}</strong>
                      <small className="ellipsis" title={s.command}>
                        {t.locations[s.location] ?? s.location}
                      </small>
                    </span>
                  </span>
                  <span className="dim ellipsis path" title={s.command}>
                    {s.command}
                  </span>
                  <Toggle on={s.enabled} label={t.runAtStartup(s.name)} onChange={(v) => setStartupItem(s, v)} />
                </div>
              ))}
            </div>
          </section>
        )}

        {view === "cleaner" && (
          <section className="view">
            <div className="hero">
              <h1>{t.cleaner}</h1>
              <p className="lede">{t.cleanerDesc}</p>
            </div>
            <div className="list">
              {junk === null && <Empty icon={<LoaderCircle className="spin" size={22} />}>{t.scanningJunk}</Empty>}
              {junk?.map((g) => {
                const [title, desc] = t.junk[g.id] ?? [g.id, ""];
                return (
                  <label key={g.id} className="row simple pick">
                    <span className="name">
                      <input type="checkbox" className="check" checked={junkPicked.has(g.id)} disabled={!g.sizeBytes} onChange={() => setJunkPicked(toggleIn(junkPicked, g.id))} />
                      <span>
                        <strong>{title}</strong>
                        <small>{desc}</small>
                      </span>
                    </span>
                    <span className="dim ellipsis path" title={g.path}>
                      {g.path}
                    </span>
                    <span className="num big">{fmtSize(g.sizeBytes)}</span>
                  </label>
                );
              })}
            </div>
            {junk && (
              <div className="footer-actions">
                <button className="key" onClick={reloadJunk}>
                  <RotateCcw size={15} /> {t.rescan}
                </button>
                <button className="key primary" disabled={!junkTotal || working === "junk"} onClick={cleanJunk}>
                  {working === "junk" ? <LoaderCircle className="spin" size={15} /> : <Eraser size={15} />} {t.cleanBtn(fmtSize(junkTotal))}
                </button>
              </div>
            )}
          </section>
        )}

        {view === "tracker" && (
          <section className="view">
            <div className="hero">
              <h1>{t.tracker}</h1>
              <p className="lede">{t.trackerIntro}</p>
            </div>

            <div className="panel-inline">
              {traceDraft ? (
                <>
                  <p>{traceDraft.items.length ? t.trackerFound(traceDraft.items.length) : t.traceNothing}</p>
                  {traceDraft.items.length > 0 && (
                    <>
                      <ul className="leftovers">
                        {traceDraft.items.map((i) => (
                          <li key={i.path}>
                            <label>
                              <input type="checkbox" className="check" checked={traceDraft.picked.has(i.path)} onChange={() => setTraceDraft({ ...traceDraft, picked: toggleIn(traceDraft.picked, i.path) })} />
                              <span className={`tag ${i.kind}`}>{i.kind === "file" ? t.folder : t.registry}</span>
                              <span className="ellipsis path" title={i.path}>
                                {i.path}
                              </span>
                              <span className="num dim">{i.kind === "file" ? fmtSize(i.sizeBytes) : ""}</span>
                            </label>
                          </li>
                        ))}
                      </ul>
                      <div className="footer-actions">
                        <input className="field" placeholder={t.traceName} value={traceDraft.name} onChange={(e) => setTraceDraft({ ...traceDraft, name: e.target.value })} />
                        <button className="key" onClick={() => setTraceDraft(null)}>
                          {t.cancel}
                        </button>
                        <button className="key primary" disabled={!traceDraft.picked.size} onClick={saveTrace}>
                          <Check size={15} /> {t.saveTrace}
                        </button>
                      </div>
                    </>
                  )}
                  {!traceDraft.items.length && (
                    <div className="footer-actions">
                      <button className="key" onClick={() => setTraceDraft(null)}>
                        {t.done}
                      </button>
                    </div>
                  )}
                </>
              ) : trackingSince ? (
                <div className="tracking">
                  <span className="pulse" aria-hidden />
                  <p>{t.trackerStarted(new Date(trackingSince).toLocaleTimeString(locale, { timeStyle: "short" }))}</p>
                  <span className="spacer" />
                  <button className="key" onClick={trackerCancel}>
                    {t.cancel}
                  </button>
                  <button className="key primary" disabled={working === "tracker"} onClick={trackerFinish}>
                    {working === "tracker" ? <LoaderCircle className="spin" size={15} /> : <Check size={15} />} {t.trackerFinish}
                  </button>
                </div>
              ) : (
                <div className="tracking">
                  <Radar size={20} className="lit-icon" />
                  <span className="spacer" />
                  <button className="key primary" disabled={working === "tracker"} onClick={trackerStart}>
                    {working === "tracker" ? (
                      <>
                        <LoaderCircle className="spin" size={15} /> {t.trackerWorking}
                      </>
                    ) : (
                      <>
                        <Radar size={15} /> {t.trackerStart}
                      </>
                    )}
                  </button>
                </div>
              )}
            </div>

            {traces.length > 0 && (
              <>
                <h2 className="section-title">{t.traces}</h2>
                <div className="list">
                  {traces.map((tr) => (
                    <div key={tr.id} className="row simple">
                      <span className="name">
                        <AppIcon name={tr.name} />
                        <span>
                          <strong>{tr.name}</strong>
                          <small>
                            {when(tr.created)}, {t.items(tr.items.length)}, {fmtSize(tr.items.reduce((a, i) => a + i.sizeBytes, 0))}
                          </small>
                        </span>
                      </span>
                      <button className="key" onClick={() => invoke("delete_trace", { id: tr.id }).then(reloadTracker)}>
                        {t.forget}
                      </button>
                      <button className="key danger" disabled={working === tr.id} onClick={() => removeTrace(tr)}>
                        <Trash2 size={15} /> {t.removeAll}
                      </button>
                    </div>
                  ))}
                </div>
              </>
            )}
          </section>
        )}

        {view === "quarantine" && (
          <section className="view">
            <div className="hero">
              <h1>{t.quarantine}</h1>
              <p className="lede">{t.quarantineDesc(fmtSize(quarantineBytes))}</p>
            </div>
            <div className="list">
              {!sessions.length && <Empty icon={<ShieldCheck size={22} />}>{t.quarantineEmpty}</Empty>}
              {sessions.map((s) => (
                <div key={s.id} className="row simple">
                  <span className="name">
                    <span className="appicon" style={{ "--s": "34px" } as CSSProperties}>
                      <Package size={17} />
                    </span>
                    <span>
                      <strong>{s.program}</strong>
                      <small>
                        {when(s.created)}, {t.items(s.items.length)}, {fmtSize(s.items.reduce((a, i) => a + i.sizeBytes, 0))}
                      </small>
                    </span>
                  </span>
                  <button className="key" onClick={() => restore(s)}>
                    <RotateCcw size={15} /> {t.restore}
                  </button>
                  <button className="key danger-ghost" onClick={() => purgeSession(s)}>
                    <Trash2 size={15} /> {t.delete}
                  </button>
                </div>
              ))}
            </div>
          </section>
        )}
      </main>

      {step && current && (
        <div className="overlay" role="dialog" aria-modal aria-label={current.name}>
          <section className="sheet">
            <ol className="steps" aria-label={queue.length > 1 ? t.programOf(index + 1, queue.length) : undefined}>
              {t.steps.map((label, i) => (
                <li key={label} className={i < stepIndex ? "done" : i === stepIndex ? "now" : ""}>
                  <span>{i < stepIndex ? <Check size={12} /> : i + 1}</span>
                  {label}
                </li>
              ))}
              {queue.length > 1 && <li className="of">{t.programOf(index + 1, queue.length)}</li>}
            </ol>
            <h2>
              <AppIcon name={current.name} src={icons[current.icon]} size={44} /> {current.name}
            </h2>

            {step.kind === "running" && (
              <div className="busy">
                <LoaderCircle className="spin" size={20} /> {t.waiting}
              </div>
            )}
            {(step.kind === "scanning" || step.kind === "moving") && (
              <div className="busy">
                <LoaderCircle className="spin" size={20} />
                {step.kind === "scanning" ? t.scanning : step.permanent ? t.deleting : t.moving}
              </div>
            )}

            {step.kind === "review" && (
              <>
                {step.note && <p className="note">{step.note}</p>}
                {step.scan.stillInstalled && !step.force && <p className="note warn">{t.stillInstalled(current.name)}</p>}
                {step.scan.items.length ? (
                  <>
                    <div className="listhead">
                      <label>
                        <input
                          type="checkbox"
                          className="check"
                          checked={step.picked.size === step.scan.items.length}
                          onChange={(e) => setStep({ ...step, picked: new Set(e.target.checked ? step.scan.items.map((i) => i.path) : []) })}
                        />
                        {t.selectAll}
                      </label>
                      <span className="dim">
                        {t.items(step.scan.items.length)}, {fmtSize(step.scan.items.reduce((a, i) => a + i.sizeBytes, 0))}
                      </span>
                    </div>
                    <ul className="leftovers">
                      {step.scan.items.map((i) => (
                        <li key={i.path}>
                          <label>
                            <input type="checkbox" className="check" checked={step.picked.has(i.path)} onChange={() => setStep({ ...step, picked: toggleIn(step.picked, i.path) })} />
                            <span className={`tag ${i.kind}`}>{i.kind === "file" ? t.folder : t.registry}</span>
                            <span className="ellipsis path" title={i.path}>
                              {i.path}
                            </span>
                            <span className="num dim">{i.kind === "file" ? fmtSize(i.sizeBytes) : ""}</span>
                          </label>
                        </li>
                      ))}
                    </ul>
                  </>
                ) : (
                  <p className="clean">
                    <ShieldCheck size={18} /> {t.noLeftovers}
                  </p>
                )}
                <div className="footer-actions">
                  <button className="key" onClick={() => scan(current, step.note, step.force)}>
                    <RotateCcw size={15} /> {t.rescan}
                  </button>
                  <span className="spacer" />
                  <button className="key" onClick={next}>
                    {step.scan.items.length ? t.skip : t.continue}
                  </button>
                  {step.scan.items.length > 0 && (
                    <>
                      <button className="key danger-ghost" disabled={!step.picked.size} onClick={() => removePicked(true)}>
                        <Trash2 size={15} /> {t.deletePermanently}
                      </button>
                      <button className="key primary" disabled={!step.picked.size} onClick={() => removePicked(false)} title={t.quarantineHint}>
                        <Archive size={15} /> {t.quarantineBtn} {step.picked.size || ""}
                      </button>
                    </>
                  )}
                </div>
              </>
            )}

            {step.kind === "done" && (
              <>
                <p className="clean">
                  <ShieldCheck size={18} />
                  {step.permanent ? t.doneDeleted(step.outcome.done) : t.doneMoved(step.outcome.done)}
                </p>
                {step.outcome.errors.length > 0 && (
                  <ul className="errors">
                    {step.outcome.errors.map((e) => (
                      <li key={e}>{e}</li>
                    ))}
                  </ul>
                )}
                <div className="footer-actions">
                  <span className="spacer" />
                  <button className="key primary" onClick={next}>
                    {index + 1 < queue.length ? t.nextProgram : t.done}
                  </button>
                </div>
              </>
            )}
          </section>
        </div>
      )}

      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
    </div>
  );
}
