import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

type Step =
  | { kind: "running" }
  | { kind: "scanning"; note?: string }
  | { kind: "review"; scan: ScanResult; picked: Set<string>; note?: string }
  | { kind: "moving"; permanent: boolean }
  | { kind: "done"; outcome: Outcome; permanent: boolean };

type Sort = "name" | "size" | "date";

function fmtSize(bytes: number) {
  if (!bytes) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (bytes >= 1024 && i < u.length - 1) (bytes /= 1024), i++;
  return `${bytes.toFixed(bytes < 10 && i > 0 ? 1 : 0)} ${u[i]}`;
}

function fmtDate(yyyymmdd: string) {
  const m = /^(\d{4})(\d{2})(\d{2})$/.exec(yyyymmdd);
  return m ? `${m[3]}/${m[2]}/${m[1]}` : "—";
}

function Avatar({ name }: { name: string }) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  const letters = name.replace(/[^\p{L}\p{N} ]/gu, "").split(" ").filter(Boolean).slice(0, 2).map((w) => w[0]).join("");
  return (
    <span className="avatar" style={{ "--h": h } as React.CSSProperties} aria-hidden>
      {letters.toUpperCase() || "?"}
    </span>
  );
}

export default function App() {
  const [view, setView] = useState<"programs" | "quarantine">("programs");
  const [programs, setPrograms] = useState<Program[] | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [admin, setAdmin] = useState(true);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("name");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [queue, setQueue] = useState<Program[]>([]);
  const [index, setIndex] = useState(0);
  const [step, setStep] = useState<Step | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const reload = async () => {
    setPrograms(await invoke<Program[]>("list_programs"));
    setSessions(await invoke<Session[]>("list_quarantine"));
  };

  useEffect(() => {
    reload();
    invoke<boolean>("is_admin").then(setAdmin);
  }, []);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = (programs ?? []).filter(
      (p) => !q || p.name.toLowerCase().includes(q) || p.publisher.toLowerCase().includes(q),
    );
    if (sort === "size") return [...list].sort((a, b) => b.sizeKb - a.sizeKb);
    if (sort === "date") return [...list].sort((a, b) => b.installDate.localeCompare(a.installDate));
    return list;
  }, [programs, query, sort]);

  const totalKb = (programs ?? []).reduce((s, p) => s + p.sizeKb, 0);
  const selectedPrograms = (programs ?? []).filter((p) => selected.has(p.id));
  const selectedKb = selectedPrograms.reduce((s, p) => s + p.sizeKb, 0);

  const toggle = (id: string) =>
    setSelected((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });

  // --- uninstall flow -------------------------------------------------------
  const scan = async (p: Program, note?: string) => {
    setStep({ kind: "scanning", note });
    const res = await invoke<ScanResult>("scan_leftovers", { program: p });
    // If the program still looks installed, nothing is pre-selected: the user must opt in.
    const picked = new Set(res.stillInstalled ? [] : res.items.map((i) => i.path));
    setStep({ kind: "review", scan: res, picked, note });
  };

  const runFor = async (p: Program) => {
    setStep({ kind: "running" });
    let note: string | undefined;
    try {
      const code = await invoke<number>("uninstall", { program: p });
      if (code !== 0) note = `The uninstaller exited with code ${code}.`;
    } catch (e) {
      note = `Couldn't start the uninstaller: ${e}`;
    }
    await scan(p, note);
  };

  const start = (list: Program[]) => {
    if (!list.length) return;
    setQueue(list);
    setIndex(0);
    runFor(list[0]);
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
    if (permanent && !confirm(`Permanently delete ${items.length} items? This can't be undone.`)) return;
    setStep({ kind: "moving", permanent });
    try {
      const cmd = permanent ? "delete_items" : "quarantine_items";
      const outcome = await invoke<Outcome>(cmd, { program: queue[index].name, items });
      setStep({ kind: "done", outcome, permanent });
    } catch (e) {
      setStep({ kind: "done", outcome: { done: 0, errors: [String(e)] }, permanent });
    }
  };

  // --- quarantine actions ---------------------------------------------------
  const restore = async (s: Session) => {
    const o = await invoke<Outcome>("restore_session", { id: s.id });
    setToast(o.errors.length ? `Restored ${o.done}, ${o.errors.length} failed` : `Restored ${s.program}`);
    await reload();
  };

  const purgeSession = async (s: Session) => {
    if (!confirm(`Permanently delete the backup of ${s.program}? This can't be undone.`)) return;
    await invoke("delete_session", { id: s.id });
    await reload();
  };

  const current = queue[index];

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="mark" aria-hidden />
          PURGE
        </div>
        <nav>
          <button className={view === "programs" ? "active" : ""} onClick={() => setView("programs")}>
            Programs <span className="count">{programs?.length ?? "…"}</span>
          </button>
          <button className={view === "quarantine" ? "active" : ""} onClick={() => setView("quarantine")}>
            Quarantine <span className="count">{sessions.length}</span>
          </button>
        </nav>
        <div className={`status ${admin ? "ok" : "warn"}`}>
          <span className="dot" />
          {admin ? "Administrator" : "Limited mode — run as admin to clean system leftovers"}
        </div>
      </aside>

      <main>
        {view === "programs" ? (
          <>
            <header>
              <div>
                <h1>Programs</h1>
                <p className="muted">
                  {programs ? `${programs.length} installed · ${fmtSize(totalKb * 1024)}` : "Reading the registry…"}
                </p>
              </div>
              <div className="tools">
                <input
                  className="search"
                  type="search"
                  placeholder="Search name or publisher"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  aria-label="Search programs"
                />
                <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort by">
                  <option value="name">Name</option>
                  <option value="size">Largest</option>
                  <option value="date">Newest</option>
                </select>
              </div>
            </header>

            <div className="table" role="list">
              <div className="row head" aria-hidden>
                <span />
                <span>Name</span>
                <span>Version</span>
                <span className="num">Size</span>
                <span className="num">Installed</span>
              </div>
              {programs === null && <div className="empty">Loading…</div>}
              {programs && !visible.length && <div className="empty">No programs match “{query}”.</div>}
              {visible.map((p) => (
                <label key={p.id} className={`row ${selected.has(p.id) ? "sel" : ""}`} role="listitem">
                  <input type="checkbox" checked={selected.has(p.id)} onChange={() => toggle(p.id)} />
                  <span className="name">
                    <Avatar name={p.name} />
                    <span>
                      <strong>{p.name}</strong>
                      <small>{p.publisher || "Unknown publisher"}</small>
                    </span>
                  </span>
                  <span className="muted mono">{p.version || "—"}</span>
                  <span className="num">{fmtSize(p.sizeKb * 1024)}</span>
                  <span className="num muted">{fmtDate(p.installDate)}</span>
                </label>
              ))}
            </div>

            <div className={`actionbar ${selected.size ? "show" : ""}`}>
              <span>
                <strong>{selected.size}</strong> selected · {fmtSize(selectedKb * 1024)}
              </span>
              <span className="spacer" />
              <button className="ghost" onClick={() => setSelected(new Set())}>
                Clear
              </button>
              <button className="danger" onClick={() => start(selectedPrograms)}>
                Uninstall {selected.size > 1 ? selected.size : ""}
              </button>
            </div>
          </>
        ) : (
          <>
            <header>
              <div>
                <h1>Quarantine</h1>
                <p className="muted">Removed leftovers are kept here so you can undo. Delete to free the space.</p>
              </div>
            </header>
            {!sessions.length && <div className="empty">Nothing in quarantine.</div>}
            <div className="cards">
              {sessions.map((s) => (
                <article key={s.id} className="card">
                  <div>
                    <strong>{s.program}</strong>
                    <small className="muted">
                      {new Date(s.created).toLocaleString()} · {s.items.length} items ·{" "}
                      {fmtSize(s.items.reduce((a, i) => a + i.sizeBytes, 0))}
                    </small>
                  </div>
                  <span className="spacer" />
                  <button className="ghost" onClick={() => restore(s)}>
                    Restore
                  </button>
                  <button className="danger ghost" onClick={() => purgeSession(s)}>
                    Delete
                  </button>
                </article>
              ))}
            </div>
          </>
        )}
      </main>

      {step && current && (
        <div className="overlay" role="dialog" aria-modal aria-label={`Uninstalling ${current.name}`}>
          <section className="panel">
            <p className="eyebrow">
              {queue.length > 1 ? `Program ${index + 1} of ${queue.length}` : "Uninstall"}
            </p>
            <h2>
              <Avatar name={current.name} /> {current.name}
            </h2>

            {step.kind === "running" && (
              <div className="busy">
                <span className="spinner" /> Waiting for the program's uninstaller to finish…
              </div>
            )}
            {(step.kind === "scanning" || step.kind === "moving") && (
              <div className="busy">
                <span className="spinner" />
                {step.kind === "scanning"
                  ? "Scanning for leftovers…"
                  : step.permanent
                    ? "Deleting…"
                    : "Moving to quarantine…"}
              </div>
            )}

            {step.kind === "review" && (
              <>
                {step.note && <p className="note">{step.note}</p>}
                {step.scan.stillInstalled && (
                  <p className="note warn">
                    {current.name} still looks installed — the uninstaller may still be open or was cancelled.
                    Finish it, then rescan. Nothing is selected for you.
                  </p>
                )}
                {step.scan.items.length ? (
                  <>
                    <div className="listhead">
                      <label>
                        <input
                          type="checkbox"
                          checked={step.picked.size === step.scan.items.length}
                          onChange={(e) =>
                            setStep({
                              ...step,
                              picked: new Set(e.target.checked ? step.scan.items.map((i) => i.path) : []),
                            })
                          }
                        />
                        Select all
                      </label>
                      <span className="muted">
                        {step.scan.items.length} leftovers ·{" "}
                        {fmtSize(step.scan.items.reduce((a, i) => a + i.sizeBytes, 0))}
                      </span>
                    </div>
                    <ul className="leftovers">
                      {step.scan.items.map((i) => (
                        <li key={i.path}>
                          <label>
                            <input
                              type="checkbox"
                              checked={step.picked.has(i.path)}
                              onChange={() => {
                                const picked = new Set(step.picked);
                                picked.has(i.path) ? picked.delete(i.path) : picked.add(i.path);
                                setStep({ ...step, picked });
                              }}
                            />
                            <span className={`tag ${i.kind}`}>{i.kind === "file" ? "Folder" : "Registry"}</span>
                            <span className="mono path" title={i.path}>
                              {i.path}
                            </span>
                            <span className="num muted">{i.kind === "file" ? fmtSize(i.sizeBytes) : ""}</span>
                          </label>
                        </li>
                      ))}
                    </ul>
                  </>
                ) : (
                  <p className="clean">No leftovers found. Clean uninstall.</p>
                )}
                <div className="buttons">
                  <button className="ghost" onClick={() => scan(current)}>
                    Rescan
                  </button>
                  <span className="spacer" />
                  <button className="ghost" onClick={next}>
                    {step.scan.items.length ? "Skip" : "Continue"}
                  </button>
                  {step.scan.items.length > 0 && (
                    <>
                      <button className="danger ghost" disabled={!step.picked.size} onClick={() => removePicked(true)}>
                        Delete permanently
                      </button>
                      <button
                        className="primary"
                        disabled={!step.picked.size}
                        onClick={() => removePicked(false)}
                        title="Moved aside so you can restore it later"
                      >
                        Quarantine {step.picked.size || ""}
                      </button>
                    </>
                  )}
                </div>
              </>
            )}

            {step.kind === "done" && (
              <>
                <p className="clean">
                  {step.permanent ? "Deleted" : "Moved"} {step.outcome.done} items
                  {step.permanent ? "." : " to quarantine — restore them anytime from Quarantine."}
                </p>
                {step.outcome.errors.length > 0 && (
                  <ul className="errors">
                    {step.outcome.errors.map((e) => (
                      <li key={e} className="mono">
                        {e}
                      </li>
                    ))}
                  </ul>
                )}
                <div className="buttons">
                  <span className="spacer" />
                  <button className="primary" onClick={next}>
                    {index + 1 < queue.length ? "Next program" : "Done"}
                  </button>
                </div>
              </>
            )}
          </section>
        </div>
      )}

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}
