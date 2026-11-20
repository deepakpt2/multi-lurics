"use client";

import { Download, FileUp, ShieldCheck } from "lucide-react";
import { useTranslations } from "next-intl";
import { useRef, useState } from "react";
import { api, call, toProblem } from "@/api/client";
import { COLUMNS, parseImport, template, type ImportResult, type ImportRow, type Parsed } from "@/lib/import-rows";
import { ErrorState } from "../states";
import { useSession } from "../session";

const BATCH = 25;

type Phase =
  | { kind: "idle" }
  | { kind: "working"; mode: "check" | "import"; done: number }
  | { kind: "done"; mode: "check" | "import"; results: ImportResult[] }
  | { kind: "error"; error: unknown };

/** `File.text()` where there is one; a FileReader where there is not (older browsers). */
const readText = (file: File): Promise<string> =>
  typeof file.text === "function"
    ? file.text()
    : new Promise((resolve, reject) => {
        const r = new FileReader();
        r.onload = () => resolve(String(r.result ?? ""));
        r.onerror = () => reject(r.error);
        r.readAsText(file);
      });

function download(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/csv;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

/**
 * Bulk import for moderators: download the template (or use a spreadsheet you already have), choose the
 * file, check it, then import. Checking changes nothing and says, row by row, what would happen.
 */
export function BulkImport() {
  const t = useTranslations("import");
  const tv = useTranslations("validation");
  const { user, isModerator } = useSession();
  const [parsed, setParsed] = useState<Parsed | null>(null);
  const [fileName, setFileName] = useState("");
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const input = useRef<HTMLInputElement>(null);

  if (user === undefined) return null;
  if (!isModerator) return <p className="alert alert-warning">{t("moderatorsOnly")}</p>;

  const choose = async (file: File | undefined) => {
    if (!file) return;
    setFileName(file.name);
    setPhase({ kind: "idle" });
    setParsed(parseImport(await readText(file)));
  };

  const run = async (mode: "check" | "import") => {
    if (!parsed) return;
    const results: ImportResult[] = [];
    setPhase({ kind: "working", mode, done: 0 });
    try {
      for (let i = 0; i < parsed.rows.length; i += BATCH) {
        const rows: ImportRow[] = parsed.rows.slice(i, i + BATCH);
        const r = await call(api.POST("/api/v1/import/songs", { body: { rows, dry_run: mode === "check" } }));
        // Row numbers restart in each request; the file's own numbering (the header is line 1) is what people look for.
        results.push(...r.results.map((x) => ({ ...x, row: i + x.row })));
        setPhase({ kind: "working", mode, done: Math.min(parsed.rows.length, i + BATCH) });
      }
      setPhase({ kind: "done", mode, results });
    } catch (error) {
      setPhase({ kind: "error", error });
    }
  };

  const problemText = (field: string, code: string) => {
    const where = field === "row" ? "" : `${t.has(`columns.${field}`) ? t(`columns.${field}` as never) : field}: `;
    const what = tv.has(code) ? tv(code as never) : t.has(`rowProblems.${code}`) ? t(`rowProblems.${code}` as never) : code;
    return `${where}${what}`;
  };

  const results = phase.kind === "done" ? phase.results : [];
  const count = (s: string) => results.filter((r) => r.status === s).length;
  const ready = count("would_create");
  const busy = phase.kind === "working";

  return (
    <div className="import">
      <ol className="import-steps">
        <li>
          <h2>{t("step1.title")}</h2>
          <p>{t("step1.body")}</p>
          <button type="button" className="button" onClick={() => download("lyrics-import-template.csv", template())}>
            <Download size={16} aria-hidden="true" /> {t("step1.download")}
          </button>
          <details>
            <summary>{t("step1.columns")}</summary>
            <dl className="import-columns">
              {COLUMNS.map((c) => (
                <div key={c}>
                  <dt>
                    <code>{c}</code>
                  </dt>
                  <dd>{t(`columns.${c}`)}</dd>
                </div>
              ))}
            </dl>
          </details>
        </li>
        <li>
          <h2>{t("step2.title")}</h2>
          <p>{t("step2.body")}</p>
          <input ref={input} type="file" accept=".csv,.tsv,.txt,text/csv,text/plain" className="visually-hidden" aria-label={t("step2.choose")} onChange={(e) => void choose(e.target.files?.[0])} />
          <button type="button" className="button primary" onClick={() => input.current?.click()}>
            <FileUp size={16} aria-hidden="true" /> {t("step2.choose")}
          </button>
          {parsed && (
            <div className="import-summary" role="status">
              <p className="import-file">
                <strong dir="auto">{fileName}</strong>
                <span>{t("step2.rows", { count: parsed.rows.length })}</span>
              </p>
              <p className="muted small">{t("step2.recognised", { columns: parsed.columns.join(", ") })}</p>
              {!parsed.columns.includes("title") && <p className="alert alert-error">{t("step2.noTitle")}</p>}
              {parsed.ignored.length > 0 && <p className="alert alert-warning">{t("step2.ignored", { columns: parsed.ignored.join(", ") })}</p>}
              {parsed.rows.length === 0 && <p className="alert alert-warning">{t("step2.empty")}</p>}
            </div>
          )}
        </li>
        <li>
          <h2>{t("step3.title")}</h2>
          <p>{t("step3.body")}</p>
          <p className="actions">
            <button type="button" className="button" disabled={!parsed || parsed.rows.length === 0 || !parsed.columns.includes("title") || busy} onClick={() => void run("check")}>
              <ShieldCheck size={16} aria-hidden="true" /> {t("step3.check")}
            </button>
            <button type="button" className="button primary" disabled={busy || !(phase.kind === "done" && phase.mode === "check" && ready > 0)} onClick={() => void run("import")}>
              {t("step3.import", { count: ready })}
            </button>
          </p>
          {phase.kind === "working" && parsed && (
            <p role="status" aria-live="polite">
              <progress value={phase.done} max={parsed.rows.length} /> {t("progress", { done: phase.done, total: parsed.rows.length })}
            </p>
          )}
          {phase.kind === "error" && <ErrorState error={toProblem(phase.error, 0)} />}
        </li>
      </ol>

      {phase.kind === "done" && (
        <section aria-labelledby="import-result" className="import-result">
          <h2 id="import-result">{phase.mode === "check" ? t("result.checked") : t("result.imported")}</h2>
          <ul className="import-counts">
            <li>{t(phase.mode === "check" ? "result.ready" : "result.created", { count: phase.mode === "check" ? ready : count("created") })}</li>
            <li>{t("result.skipped", { count: count("skipped") })}</li>
            <li className={count("failed") > 0 ? "import-bad" : undefined}>{t("result.failed", { count: count("failed") })}</li>
          </ul>
          {results.some((r) => r.status !== "created" && r.status !== "would_create") && (
            <table className="import-table">
              <caption className="visually-hidden">{t("result.table")}</caption>
              <thead>
                <tr>
                  <th scope="col">{t("result.row")}</th>
                  <th scope="col">{t("columns.title")}</th>
                  <th scope="col">{t("result.what")}</th>
                </tr>
              </thead>
              <tbody>
                {results
                  .filter((r) => r.status === "failed" || r.status === "skipped")
                  .map((r) => (
                    <tr key={r.row} data-status={r.status}>
                      <td>{r.row + 1}</td>
                      <td dir="auto">{r.title}</td>
                      <td>
                        {r.status === "skipped"
                          ? t("result.alreadyThere")
                          : r.problems.map((p) => problemText(p.field, p.code)).join(" · ")}
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          )}
        </section>
      )}
    </div>
  );
}
