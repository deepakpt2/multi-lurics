"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { FileUp, ImagePlus, Trash2 } from "lucide-react";
import { api, call } from "@/api/client";
import { Link, useRouter } from "@/i18n/navigation";
import { mediaUrl } from "@/lib/images";
import { parseMarkdownFile, uploadArticleImage, type Article, type ArticleImage } from "@/lib/articles";
import { useSession } from "../session";
import { ErrorState, LoadingState, ProblemAlert } from "../states";
import { ArticleBody } from "./markdown";

const readText = (file: File) =>
  new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result ?? ""));
    r.onerror = () => reject(r.error);
    r.readAsText(file);
  });

/**
 * Write or paste Markdown made elsewhere (or load a .md file), add pictures, preview it as readers will
 * see it, and save as a draft or publish. Pictures can be added once the article is first saved.
 */
export function ArticleEditor({ slug }: { slug?: string }) {
  const t = useTranslations("articles");
  const router = useRouter();
  const { user, isModerator } = useSession();
  const [article, setArticle] = useState<Article>();
  const [loadError, setLoadError] = useState<unknown>();
  const [title, setTitle] = useState("");
  const [address, setAddress] = useState("");
  const [summary, setSummary] = useState("");
  const [body, setBody] = useState("");
  const [preview, setPreview] = useState(false);
  const [images, setImages] = useState<ArticleImage[]>([]);
  const [error, setError] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const picInput = useRef<HTMLInputElement>(null);
  const credit = useRef<HTMLInputElement>(null);

  const adopt = useCallback((a: Article) => {
    setArticle(a);
    setTitle(a.title);
    setAddress(a.slug);
    setSummary(a.summary ?? "");
    setBody(a.body_md);
  }, []);
  const load = useCallback(async () => {
    if (!slug) return;
    try {
      const a = await call(api.GET("/api/v1/articles/{key}", { params: { path: { key: slug } } }));
      adopt(a);
      setImages((await call(api.GET("/api/v1/articles/{key}/images", { params: { path: { key: a.id } } }))).items);
      setLoadError(undefined);
    } catch (e) {
      setLoadError(e);
    }
  }, [slug, adopt]);
  useEffect(() => {
    if (isModerator) void load();
  }, [load, isModerator]);

  if (user === undefined) return null;
  if (!isModerator) return <p className="alert alert-warning">{t("moderatorsOnly")}</p>;
  if (loadError !== undefined) return <ErrorState error={loadError} onRetry={load} />;
  if (slug && !article) return <LoadingState />;

  const loadFile = async (file: File | undefined) => {
    if (!file) return;
    const p = parseMarkdownFile(await readText(file));
    if (p.title) setTitle(p.title);
    if (p.summary) setSummary(p.summary);
    if (p.slug && !article) setAddress(p.slug);
    setBody(p.body);
    setPreview(false);
  };

  const save = async (publish?: boolean) => {
    if (busy) return;
    setBusy(true);
    setSaved(false);
    try {
      if (article) {
        const a = await call(
          api.PATCH("/api/v1/articles/{key}", {
            params: { path: { key: article.id } },
            body: { expected_version: article.version, title, slug: address, summary: summary.trim() ? summary : null, body_md: body, ...(publish === undefined ? {} : { publish }) },
          }),
        );
        adopt(a);
        setError(undefined);
        setSaved(true);
        if (a.slug !== slug) router.replace(`/articles/${a.slug}/edit`);
      } else {
        const a = await call(api.POST("/api/v1/articles", { body: { title, slug: address.trim() || null, summary: summary.trim() || null, body_md: body, publish: publish ?? false } }));
        router.replace(`/articles/${a.slug}/edit`);
      }
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };

  const addPicture = async (file: File | undefined) => {
    if (!file || !article || busy) return;
    setBusy(true);
    try {
      const img = await uploadArticleImage(article.id, file, credit.current?.value);
      setImages((l) => [...l, img]);
      if (credit.current) credit.current.value = "";
      setError(undefined);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
      if (picInput.current) picInput.current.value = "";
    }
  };
  const setCover = async (id: string | null) => {
    if (!article || busy) return;
    setBusy(true);
    try {
      adopt(await call(api.PATCH("/api/v1/articles/{key}", { params: { path: { key: article.id } }, body: { expected_version: article.version, cover_image_id: id } })));
      setError(undefined);
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  const removePicture = async (img: ArticleImage) => {
    if (!article || busy) return;
    setBusy(true);
    try {
      await call(api.DELETE("/api/v1/articles/{key}/images/{image_id}", { params: { path: { key: article.id, image_id: img.id } } }));
      setImages((l) => l.filter((x) => x.id !== img.id));
      await load();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  const published = article?.status === "published";
  return (
    <form
      noValidate
      className="contribute-form article-editor"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {error !== undefined && <ProblemAlert error={error} onReload={load} />}
      {saved && (
        <p className="alert alert-success" role="status">
          {t("saved")}
        </p>
      )}
      <div className="field">
        <label htmlFor="art-title">{t("fieldTitle")}</label>
        <input id="art-title" dir="auto" value={title} onChange={(e) => setTitle(e.target.value)} />
      </div>
      <div className="field">
        <label htmlFor="art-summary">{t("fieldSummary")}</label>
        <textarea id="art-summary" rows={2} dir="auto" value={summary} onChange={(e) => setSummary(e.target.value)} />
        <p className="muted small">{t("summaryHint")}</p>
      </div>
      <div className="field">
        <label htmlFor="art-slug">{t("fieldAddress")}</label>
        <input id="art-slug" dir="ltr" value={address} onChange={(e) => setAddress(e.target.value)} placeholder={t("addressPlaceholder")} />
        <p className="muted small">{t("addressHint")}</p>
      </div>
      <div className="field">
        <div className="article-editor-bar">
          <label htmlFor="art-body">{t("fieldBody")}</label>
          <span className="segmented" role="radiogroup" aria-label={t("view")}>
            <button type="button" role="radio" aria-checked={!preview} onClick={() => setPreview(false)}>
              {t("write")}
            </button>
            <button type="button" role="radio" aria-checked={preview} onClick={() => setPreview(true)}>
              {t("preview")}
            </button>
          </span>
          <button type="button" className="button" onClick={() => fileInput.current?.click()}>
            <FileUp size={16} aria-hidden="true" /> {t("loadFile")}
          </button>
          <input ref={fileInput} type="file" accept=".md,.markdown,.txt,text/markdown,text/plain" className="visually-hidden" tabIndex={-1} aria-label={t("loadFile")} onChange={(e) => void loadFile(e.target.files?.[0])} />
        </div>
        {preview ? (
          <div className="article-preview">{body.trim() ? <ArticleBody markdown={body} /> : <p className="muted">{t("nothingToPreview")}</p>}</div>
        ) : (
          <textarea id="art-body" className="article-source" rows={22} dir="auto" spellCheck value={body} onChange={(e) => setBody(e.target.value)} />
        )}
        <p className="muted small">{t("bodyHint")}</p>
      </div>

      <section className="article-pictures" aria-label={t("pictures")}>
        <h2>{t("pictures")}</h2>
        {!article ? (
          <p className="muted">{t("picturesAfterSave")}</p>
        ) : (
          <>
            <div className="article-upload">
              <label htmlFor="art-credit" className="visually-hidden">
                {t("credit")}
              </label>
              <input id="art-credit" ref={credit} placeholder={t("credit")} />
              <button type="button" className="button" disabled={busy} onClick={() => picInput.current?.click()}>
                <ImagePlus size={16} aria-hidden="true" /> {t("addPicture")}
              </button>
              <input ref={picInput} type="file" accept="image/jpeg,image/png,image/webp,image/gif" className="visually-hidden" tabIndex={-1} aria-label={t("addPicture")} onChange={(e) => void addPicture(e.target.files?.[0])} />
            </div>
            <p className="muted small">{t("picturesHint")}</p>
            <ul className="article-picture-list">
              {images.map((img) => (
                <li key={img.id}>
                  {/* eslint-disable-next-line @next/next/no-img-element */}
                  <img src={mediaUrl(img.id, 480)} alt="" width={120} loading="lazy" />
                  <span className="article-picture-actions">
                    <button type="button" className="button" onClick={() => setBody((b) => `${b}${b && !b.endsWith("\n") ? "\n\n" : ""}${img.markdown}\n`)}>
                      {t("insert")}
                    </button>
                    <button type="button" className="button" aria-pressed={article.cover_image_id === img.id} disabled={busy} onClick={() => void setCover(article.cover_image_id === img.id ? null : img.id)}>
                      {article.cover_image_id === img.id ? t("coverRemove") : t("coverSet")}
                    </button>
                    <button type="button" className="icon-button" aria-label={t("deletePicture")} title={t("deletePicture")} disabled={busy} onClick={() => void removePicture(img)}>
                      <Trash2 size={16} aria-hidden="true" />
                    </button>
                  </span>
                </li>
              ))}
            </ul>
          </>
        )}
      </section>

      <p className="actions">
        <button className="button primary" type="submit" disabled={busy || !title.trim() || !body.trim()} aria-busy={busy}>
          {article ? t("save") : t("saveDraft")}
        </button>
        {!published && (
          <button className="button" type="button" disabled={busy || !title.trim() || !body.trim()} onClick={() => void save(true)}>
            {t("saveAndPublish")}
          </button>
        )}
        {article && (
          <Link className="button ghost" href={`/articles/${article.slug}`}>
            {t("viewArticle")}
          </Link>
        )}
      </p>
    </form>
  );
}
