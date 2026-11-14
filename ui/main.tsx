import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Search,
  FolderOpen,
  FileText,
  Settings,
  Pause,
  Play,
  X,
  ArrowUpRight,
  Copy,
  HardDrive,
  Plus,
  ChevronRight,
  Moon,
  Sun,
  AlertCircle,
  RefreshCw,
  ScanText,
  Check,
  SlidersHorizontal,
} from "lucide-react";
import { api, native, Root, Hit, Block, Status, sourceLabel } from "./api";
import "./style.css";
const formatSize = (n: number) =>
  n < 1024
    ? `${n} Б`
    : n < 1024 ** 2
      ? `${(n / 1024).toFixed(1)} КБ`
      : `${(n / 1024 ** 2).toFixed(1)} МБ`;
function Highlight({ text, query = "" }: { text: string; query?: string }) {
  if (text.includes("⟦"))
    return (
      <>
        {text
          .split(/(⟦.*?⟧)/gs)
          .map((part, i) =>
            part.startsWith("⟦") ? (
              <mark key={i}>{part.slice(1, -1)}</mark>
            ) : (
              part
            ),
          )}
      </>
    );
  const words =
    query
      .match(/"[^"]+"|[^\s"]+/g)
      ?.map((x) => x.replaceAll('"', ""))
      .filter(Boolean) ?? [];
  if (!words.length) return <>{text}</>;
  const pattern = new RegExp(
    `(${words.map((x) => x.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`,
    "giu",
  );
  return (
    <>
      {text
        .split(pattern)
        .map((part, i) => (i % 2 ? <mark key={i}>{part}</mark> : part))}
    </>
  );
}
function App() {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [selected, setSelected] = useState<Hit | null>(null);
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [roots, setRoots] = useState<Root[]>([]);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [panel, setPanel] = useState<"settings" | "folders" | "errors" | null>(
    null,
  );
  const [settings, setSettings] = useState<Record<string, string>>({
    theme: localStorage.getItem("theme") ?? "light",
    ocr: "true",
    workers: "auto",
    all_disks: "false",
    tesseract: "tesseract",
    tessdata: "",
  });
  const [errors, setErrors] = useState<
    { id: number; path: string; error: string }[]
  >([]);
  const [exclusions, setExclusions] = useState<string[]>([]);
  const [mode, setMode] = useState("all");
  const [extension, setExtension] = useState("");
  const [folder, setFolder] = useState("");
  const [filters, setFilters] = useState(false);
  const [after, setAfter] = useState("");
  const [maxSize, setMaxSize] = useState("");
  const [minSize, setMinSize] = useState("");
  const [before, setBefore] = useState("");
  const [copied, setCopied] = useState(false);
  const [revision, setRevision] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const generation = useRef(0);
  async function refresh() {
    const [s, r] = await Promise.all([
      api<Status>("status"),
      api<Root[]>("roots"),
    ]);
    setStatus(s);
    setRoots(r);
  }
  useEffect(() => {
    if (!native) return;
    let alive = true;
    async function poll() {
      try {
        const [s, r] = await Promise.all([
          api<Status>("status"),
          api<Root[]>("roots"),
        ]);
        if (alive) {
          setStatus(s);
          setRoots(r);
          setRevision((v) => v + 1);
        }
      } catch (e) {
        if (alive) setError(String(e));
      }
    }
    void poll();
    void api<Record<string, string>>("settings")
      .then((v) => alive && setSettings(v))
      .catch((e) => setError(String(e)));
    const timer = setInterval(poll, 2500);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = settings.theme;
    localStorage.setItem("theme", settings.theme);
  }, [settings.theme]);
  useEffect(() => {
    const listener = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "k") {
        e.preventDefault();
        input.current?.focus();
      }
      if (e.key === "Escape") setPanel(null);
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
  useEffect(() => {
    const request = ++generation.current;
    if (!query.trim()) {
      setHits([]);
      setSelected(null);
      setLoading(false);
      return;
    }
    if (!native) return;
    const timer = setTimeout(async () => {
      setLoading(true);
      try {
        const result = await api<Hit[]>("search", {
          text: query,
          mode,
          extension,
          folder,
          after: after ? new Date(after + "T00:00:00").getTime() * 1e6 : null,
          max_size: maxSize ? Number(maxSize) * 1024 ** 2 : null,
          min_size: minSize ? Number(minSize) * 1024 ** 2 : null,
          before: before
            ? new Date(before + "T23:59:59.999").getTime() * 1e6 + 999999
            : null,
        });
        if (generation.current === request) {
          setHits(result);
          setSelected(
            (old) => result.find((h) => h.id === old?.id) ?? result[0] ?? null,
          );
        }
      } catch (e) {
        if (generation.current === request) setError(String(e));
      } finally {
        if (generation.current === request) setLoading(false);
      }
    }, 180);
    return () => clearTimeout(timer);
  }, [
    query,
    mode,
    extension,
    folder,
    after,
    before,
    minSize,
    maxSize,
    revision,
  ]);
  useEffect(() => {
    let active = true;
    setBlocks([]);
    if (!selected) return;
    setPreviewLoading(true);
    void api<Block[]>("preview", { id: selected.id })
      .then((v) => {
        if (active) setBlocks(v);
      })
      .catch((e) => active && setError(String(e)))
      .finally(() => active && setPreviewLoading(false));
    return () => {
      active = false;
    };
  }, [selected?.id, revision]);
  useEffect(() => {
    if (!panel) return;
    const previous = document.activeElement as HTMLElement | null;
    const trap = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const nodes = Array.from(
        document.querySelectorAll<HTMLElement>(
          ".modal button:not([disabled]), .modal input, .modal select, .modal summary",
        ),
      );
      const first = nodes[0],
        last = nodes.at(-1);
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first?.focus();
      }
    };
    document.addEventListener("keydown", trap);
    return () => {
      document.removeEventListener("keydown", trap);
      previous?.focus();
    };
  }, [panel]);
  async function action(op: string, args: unknown = {}) {
    try {
      setError("");
      await api(op, args);
      await refresh();
      setRevision((v) => v + 1);
    } catch (e) {
      setError(String(e));
    }
  }
  async function choose(exclude = false) {
    try {
      const path = await open({
        directory: true,
        multiple: false,
        title: exclude ? "Исключить папку" : "Добавить папку или диск",
      });
      if (typeof path === "string") {
        await action(exclude ? "exclude" : "add_root", { path });
        if (exclude) setExclusions(await api("exclusions"));
      }
    } catch (e) {
      setError(String(e));
    }
  }
  async function startAll() {
    try {
      const drives = await api<string[]>("drives");
      if (!drives.length) {
        await choose();
        return;
      }
      await api("all_drives");
      setSettings({ ...settings, all_disks: "true" });
      await refresh();
    } catch (e) {
      setError(String(e));
      await refresh();
    }
  }
  async function showPanel(p: "settings" | "folders" | "errors") {
    setPanel(p);
    try {
      if (p === "errors") setErrors(await api("errors"));
      if (p === "folders") setExclusions(await api("exclusions"));
    } catch (e) {
      setError(String(e));
    }
  }
  async function saveSettings(next: Record<string, string>) {
    try {
      if (native) await api("set_settings", next);
      setSettings(next);
    } catch (e) {
      setError(String(e));
    }
  }
  const incomplete =
    !!status &&
    (status.scanning ||
      status.pending > 0 ||
      status.processing > 0 ||
      status.errors > 0 ||
      status.offline > 0 ||
      roots.some((r) => r.error));
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-icon">
            <Search size={21} />
          </div>
          <span>
            Локальный
            <br />
            <b>поиск</b>
          </span>
        </div>
        <div className="nav-label">РАБОЧЕЕ ПРОСТРАНСТВО</div>
        <button
          className="nav-item active"
          onClick={() => input.current?.focus()}
        >
          <Search size={18} /> Поиск файлов{" "}
          <span className="shortcut">Ctrl K</span>
        </button>
        <button className="nav-item" onClick={() => showPanel("folders")}>
          <FolderOpen size={18} /> Расположения{" "}
          <span className="count">{roots.length}</span>
        </button>
        <button className="nav-item" onClick={() => showPanel("errors")}>
          <AlertCircle size={18} /> Ошибки{" "}
          {status && status.errors > 0 && (
            <span className="error-count">{status.errors}</span>
          )}
        </button>
        <div className="locations">
          <div className="nav-label">
            ГДЕ ИЩЕМ{" "}
            <button
              className="icon-button"
              aria-label="Добавить папку"
              onClick={() => choose()}
            >
              <Plus size={15} />
            </button>
          </div>
          {roots.map((r) => (
            <button
              key={r.id}
              className={`location ${folder === r.path ? "chosen" : ""}`}
              onClick={() => setFolder(folder === r.path ? "" : r.path)}
              title={r.path}
            >
              <HardDrive size={16} />
              <span>{r.path}</span>
              <i className={r.online ? "online" : "offline"} />
            </button>
          ))}
          {!roots.length && (
            <p className="muted small">
              Добавьте диски или папки,
              <br />
              чтобы начать поиск.
            </p>
          )}
        </div>
        <div className="sidebar-bottom">
          <div className="privacy">
            <Check size={15} />
            <div>
              Всё остаётся на компьютере<small>Без аккаунта и облака</small>
            </div>
          </div>
          <button className="nav-item" onClick={() => showPanel("settings")}>
            <Settings size={18} /> Настройки
          </button>
          <div className="version">
            Локальный поиск <span>0.1 · в разработке</span>
          </div>
        </div>
      </aside>
      <main>
        <header>
          <div>
            <span className="eyebrow">ВАШИ ФАЙЛЫ. ПОД РУКОЙ.</span>
            <h1>Найдётся на вашем компьютере.</h1>
            <p>По имени, содержимому документа или тексту на скане.</p>
          </div>
          <button
            className="icon-button theme"
            aria-label="Переключить тему"
            onClick={() =>
              saveSettings({
                ...settings,
                theme: settings.theme === "dark" ? "light" : "dark",
              })
            }
          >
            {settings.theme === "dark" ? <Sun size={20} /> : <Moon size={20} />}
          </button>
        </header>
        {!native && (
          <div className="notice">
            <AlertCircle size={17} />
            <span>
              Это веб-просмотр интерфейса. Для поиска и индексации запустите
              настольное приложение.
            </span>
          </div>
        )}
        {error && (
          <div role="alert" className="notice error">
            <AlertCircle size={17} />
            <span>{error}</span>
            <button
              aria-label="Закрыть ошибку"
              className="icon-button"
              onClick={() => setError("")}
            >
              <X size={16} />
            </button>
          </div>
        )}
        <section className="search-section">
          <div className="search-box">
            <Search size={23} />
            <input
              ref={input}
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Название, фраза или текст внутри файла…"
              aria-label="Поиск файлов"
            />
            {query ? (
              <button
                className="icon-button"
                aria-label="Очистить поиск"
                onClick={() => setQuery("")}
              >
                <X size={18} />
              </button>
            ) : (
              <kbd>Ctrl K</kbd>
            )}
          </div>
          <div className="filter-bar">
            <div className="segmented">
              {[
                ["all", "Везде"],
                ["names", "Имена и пути"],
                ["text", "Содержимое"],
              ].map(([v, label]) => (
                <button
                  key={v}
                  aria-pressed={mode === v}
                  className={mode === v ? "selected" : ""}
                  onClick={() => setMode(v)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="filter-right">
              <select
                aria-label="Тип файла"
                value={extension}
                onChange={(e) => setExtension(e.target.value)}
              >
                <option value="">Все типы</option>
                {[
                  "pdf",
                  "docx",
                  "xlsx",
                  "pptx",
                  "txt",
                  "md",
                  "csv",
                  "png",
                  "jpg",
                  "tiff",
                ].map((ext) => (
                  <option key={ext}>{ext}</option>
                ))}
              </select>
              <button
                className={`filter-button ${filters ? "selected" : ""}`}
                onClick={() => setFilters(!filters)}
              >
                <SlidersHorizontal size={15} /> Фильтры
              </button>
            </div>
          </div>
          {filters && (
            <div className="advanced">
              <label>
                Папка
                <select
                  value={folder}
                  onChange={(e) => setFolder(e.target.value)}
                >
                  <option value="">Все расположения</option>
                  {roots.map((r) => (
                    <option key={r.id} value={r.path}>
                      {r.path}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Изменён после
                <input
                  type="date"
                  value={after}
                  onChange={(e) => setAfter(e.target.value)}
                />
              </label>
              <label>
                Изменён до
                <input
                  type="date"
                  value={before}
                  onChange={(e) => setBefore(e.target.value)}
                />
              </label>
              <label>
                Размер от, МБ
                <input
                  type="number"
                  min="0"
                  value={minSize}
                  onChange={(e) => setMinSize(e.target.value)}
                />
              </label>
              <label>
                Размер до, МБ
                <input
                  type="number"
                  min="0"
                  value={maxSize}
                  onChange={(e) => setMaxSize(e.target.value)}
                />
              </label>
              <button
                onClick={() => {
                  setFolder("");
                  setAfter("");
                  setBefore("");
                  setMinSize("");
                  setMaxSize("");
                  setExtension("");
                }}
              >
                Сбросить
              </button>
            </div>
          )}
        </section>
        <section className="workspace">
          <div className="results">
            <div className="section-heading">
              <span>
                {query
                  ? loading
                    ? "Поиск…"
                    : `Найдено: ${hits.length}${hits.length === 200 ? " · первые 200" : ""}`
                  : "Поиск файлов"}
              </span>
              {incomplete && <span className="subtle">Индекс ещё неполон</span>}
            </div>
            {!query ? (
              <div className="empty">
                <div className="empty-icon">
                  <FolderOpen size={34} />
                  <span className="tiny-search">
                    <Search size={16} />
                  </span>
                </div>
                <h2>
                  {roots.length ? "Что будем искать?" : "Начнём с ваших файлов"}
                </h2>
                <p>
                  {roots.length
                    ? "Введите часть имени или слова из документа. Для точной фразы используйте кавычки."
                    : "Индексируйте локальные диски. Поиск станет доступен сразу, пока документы обрабатываются в фоне."}
                </p>
                {!roots.length && (
                  <>
                    <button
                      className="primary"
                      disabled={!native}
                      onClick={startAll}
                    >
                      <HardDrive size={17} /> Индексировать все диски{" "}
                      <ChevronRight size={17} />
                    </button>
                    <button
                      className="text-button"
                      disabled={!native}
                      onClick={() => choose()}
                    >
                      Выбрать отдельные папки
                    </button>
                  </>
                )}
                <div className="hints">
                  <span>
                    <FileText size={15} /> Документы
                  </span>
                  <span>
                    <ScanText size={15} /> Сканы и фотографии
                  </span>
                </div>
              </div>
            ) : hits.length === 0 && !loading ? (
              <div className="empty">
                <Search size={32} />
                <h2>Совпадений пока нет</h2>
                <p>
                  Попробуйте часть имени, другую фразу или уберите фильтры.
                  {incomplete ? " Индексация ещё не завершена." : ""}
                </p>
              </div>
            ) : (
              <div
                className="result-list"
                role="listbox"
                aria-label="Результаты поиска"
                onKeyDown={(e) => {
                  const i = hits.findIndex((h) => h.id === selected?.id);
                  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                    e.preventDefault();
                    const next =
                      hits[
                        Math.max(
                          0,
                          Math.min(
                            hits.length - 1,
                            i + (e.key === "ArrowDown" ? 1 : -1),
                          ),
                        )
                      ];
                    setSelected(next);
                    document.getElementById(`hit-${next.id}`)?.focus();
                  }
                  if (e.key === "Enter" && selected)
                    void action("open", { id: selected.id });
                }}
              >
                {hits.map((h) => (
                  <button
                    id={`hit-${h.id}`}
                    role="option"
                    aria-selected={selected?.id === h.id}
                    tabIndex={selected?.id === h.id ? 0 : -1}
                    className={`result ${selected?.id === h.id ? "selected" : ""}`}
                    key={h.id}
                    onClick={() => setSelected(h)}
                    onDoubleClick={() => action("open", { id: h.id })}
                  >
                    <div className={`file-icon ${h.name.split(".").pop()}`}>
                      <FileText size={21} />
                      <small>{h.name.split(".").pop()?.slice(0, 5)}</small>
                    </div>
                    <div className="result-copy">
                      <h3>
                        <Highlight text={h.name} query={query} />
                      </h3>
                      <p className="file-path" title={h.path}>
                        {h.path}
                      </p>
                      <p className="snippet">
                        <Highlight text={h.snippet} query={query} />
                      </p>
                      <div className="meta">
                        <span
                          className={`badge ${h.source === "ocr" ? "ocr" : ""}`}
                        >
                          {sourceLabel[h.source]}
                        </span>
                        {h.location && <span>{h.location}</span>}
                        <span>{formatSize(h.size)}</span>
                        {!h.online && (
                          <span className="warning">Диск недоступен</span>
                        )}
                      </div>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
          <aside className="preview">
            <div className="section-heading">
              ПРОСМОТР <span className="safe-tag">Только чтение</span>
            </div>
            {selected ? (
              <>
                <div className="preview-title">
                  <FileText size={28} />
                  <h2>{selected.name}</h2>
                  <p>
                    {formatSize(selected.size)} ·{" "}
                    {new Date(selected.modified / 1e6).toLocaleDateString("ru")}
                  </p>
                  <div className="file-actions">
                    <button
                      className="primary"
                      onClick={() => action("open", { id: selected.id })}
                    >
                      Открыть <ArrowUpRight size={15} />
                    </button>
                    <button
                      className="icon-button"
                      title="Показать в Проводнике"
                      aria-label="Показать в Проводнике"
                      onClick={() => action("reveal", { id: selected.id })}
                    >
                      <FolderOpen size={18} />
                    </button>
                    <button
                      className="icon-button"
                      aria-label="Скопировать путь"
                      onClick={async () => {
                        try {
                          await navigator.clipboard.writeText(selected.path);
                          setCopied(true);
                          setTimeout(() => setCopied(false), 1500);
                        } catch (e) {
                          setError(String(e));
                        }
                      }}
                    >
                      {copied ? <Check size={18} /> : <Copy size={18} />}
                    </button>
                  </div>
                </div>
                <div className="preview-content">
                  {previewLoading ? (
                    <p className="muted">Загружаем текст…</p>
                  ) : blocks.length ? (
                    blocks.map((b, i) => (
                      <article key={i}>
                        <div className="page-label">
                          {b.location || "Текст документа"}
                          {b.source === "ocr" && " · OCR"}
                        </div>
                        <pre>
                          <Highlight text={b.text} query={query} />
                        </pre>
                      </article>
                    ))
                  ) : (
                    <div className="preview-empty">
                      <FileText size={28} />
                      <p>
                        Текст ещё не извлечён
                        <br />
                        или формат не поддерживается.
                      </p>
                    </div>
                  )}
                </div>
                {blocks.length > 0 && (
                  <div className="preview-note">
                    Показан текстовый фрагмент · без исходного оформления
                  </div>
                )}
              </>
            ) : (
              <div className="preview-empty">
                <div className="document-placeholder">
                  <i />
                  <i />
                  <i />
                  <i />
                </div>
                <h3>Загляните внутрь</h3>
                <p>
                  Выберите файл, чтобы увидеть
                  <br />
                  текст и найденные совпадения.
                </p>
              </div>
            )}
          </aside>
        </section>
        <footer>
          <div className="index-state">
            <span
              className={`status-dot ${status?.scanning || status?.processing ? "pulse" : ""}`}
            />
            <div>
              <strong>
                {!native
                  ? "Настольный режим недоступен"
                  : !status
                    ? "Подключаемся…"
                    : status.cancelled
                      ? "Обработка остановлена"
                      : status.paused
                        ? "Индексация на паузе"
                        : status.scanning
                          ? "Индексируем имена файлов"
                          : status.processing
                            ? "Обрабатываем документы"
                            : status.pending
                              ? "Документы в очереди"
                              : "Готово к поиску"}
              </strong>
              <span>
                {status
                  ? `${status.total.toLocaleString("ru")} файлов · ${formatSize(status.bytes)} · самый низкий фоновый приоритет`
                  : "Данные хранятся только на вашем компьютере"}
              </span>
            </div>
          </div>
          {status &&
            (status.pending > 0 ||
              status.scanning ||
              status.paused ||
              status.processing > 0 ||
              status.cancelled) && (
              <button
                className="pause"
                onClick={() =>
                  action(status.paused || status.cancelled ? "resume" : "pause")
                }
              >
                {status.paused || status.cancelled ? (
                  <Play size={15} />
                ) : (
                  <Pause size={15} />
                )}{" "}
                {status.paused || status.cancelled ? "Продолжить" : "Пауза"}
              </button>
            )}
        </footer>
      </main>
      {panel && (
        <div className="modal-backdrop" onClick={() => setPanel(null)}>
          <section
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-label={
              panel === "folders"
                ? "Расположения"
                : panel === "errors"
                  ? "Ошибки обработки"
                  : "Настройки"
            }
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal-heading">
              <h2>
                {panel === "folders"
                  ? "Расположения"
                  : panel === "errors"
                    ? "Ошибки обработки"
                    : "Настройки"}
              </h2>
              <button
                autoFocus
                className="icon-button"
                aria-label="Закрыть"
                onClick={() => setPanel(null)}
              >
                <X size={20} />
              </button>
            </div>
            {panel === "folders" ? (
              <>
                <p className="muted">
                  Индексируем доступные файлы без скачивания облачных заглушек.
                  Удаление расположения отключает автоматическое добавление
                  дисков. Удаление расположения удаляет его поисковые данные, но
                  не сами файлы.
                </p>
                <label className="setting">
                  Добавлять новые локальные диски
                  <input
                    type="checkbox"
                    checked={settings.all_disks === "true"}
                    onChange={(e) =>
                      saveSettings({
                        ...settings,
                        all_disks: String(e.target.checked),
                      })
                    }
                  />
                </label>
                {roots.map((r) => (
                  <div className="folder-row" key={r.id}>
                    <HardDrive size={20} />
                    <div>
                      <b>{r.path}</b>
                      <small>
                        {r.error || (r.online ? "Доступно" : "Не подключено")}
                      </small>
                    </div>
                    <button
                      className="icon-button"
                      aria-label={`Убрать ${r.path}`}
                      onClick={async () => {
                        await action("remove_root", { id: r.id });
                        setSettings(await api("settings"));
                      }}
                    >
                      <X size={17} />
                    </button>
                  </div>
                ))}
                <button className="primary" onClick={() => choose()}>
                  <Plus size={16} /> Добавить папку
                </button>
                <h3>Исключения</h3>
                {exclusions.map((p) => (
                  <div className="folder-row" key={p}>
                    <span>{p}</span>
                    <button
                      aria-label={`Убрать исключение ${p}`}
                      onClick={async () => {
                        await action("unexclude", { path: p });
                        setExclusions(await api("exclusions"));
                      }}
                    >
                      <X size={16} />
                    </button>
                  </div>
                ))}
                <button onClick={() => choose(true)}>Исключить папку</button>
              </>
            ) : panel === "errors" ? (
              <>
                <p className="muted">
                  Показаны первые 200 ошибок. Неподдерживаемые форматы доступны
                  в поиске по имени.
                </p>
                {errors.length ? (
                  errors.map((e) => (
                    <div className="error-row" key={e.id}>
                      <b>{e.path}</b>
                      <p>{e.error}</p>
                      <button
                        onClick={async () => {
                          await action("retry", { id: e.id });
                          setErrors(await api("errors"));
                        }}
                      >
                        Повторить
                      </button>
                    </div>
                  ))
                ) : (
                  <p className="muted">Ошибок обработки нет.</p>
                )}
                <button
                  className="primary"
                  onClick={async () => {
                    await action("retry");
                    setErrors(await api("errors"));
                  }}
                >
                  <RefreshCw size={16} /> Повторить обработку
                </button>
              </>
            ) : (
              <>
                <label className="setting">
                  Распознавать текст на сканах
                  <input
                    type="checkbox"
                    checked={settings.ocr === "true"}
                    onChange={(e) =>
                      saveSettings({
                        ...settings,
                        ocr: String(e.target.checked),
                      })
                    }
                  />
                </label>
                <label className="setting">
                  Параллельные обработчики
                  <select
                    value={settings.workers}
                    onChange={(e) =>
                      saveSettings({ ...settings, workers: e.target.value })
                    }
                  >
                    <option value="auto">Авто: доступные ресурсы</option>
                    {[1, 2, 3, 4, 8, 16, 32, 64].map((n) => (
                      <option key={n}>{n}</option>
                    ))}
                  </select>
                </label>
                <p className="muted small">
                  Работают на самом низком приоритете, без искусственного
                  ограничения скорости. Число процессов не превышает число
                  доступных ядер и учитывает запас свободной памяти.
                </p>
                <details>
                  <summary>Компоненты распознавания</summary>
                  <p className="muted small">
                    Для обработки документов нужен Python, для OCR — Tesseract с
                    языками rus, eng и osd. Приложение не скачивает их
                    автоматически.
                  </p>
                  <label className="stack">
                    Путь к Tesseract
                    <input
                      value={settings.tesseract}
                      onChange={(e) =>
                        setSettings({ ...settings, tesseract: e.target.value })
                      }
                      onBlur={() => saveSettings(settings)}
                    />
                  </label>
                  <label className="stack">
                    Папка языковых моделей
                    <input
                      value={settings.tessdata}
                      onChange={(e) =>
                        setSettings({ ...settings, tessdata: e.target.value })
                      }
                      onBlur={() => saveSettings(settings)}
                    />
                  </label>
                </details>
                <hr />
                <div className="setting">
                  <span>Размер индекса и текста</span>
                  <b>{formatSize(status?.bytes ?? 0)}</b>
                </div>
                <div className="setting">
                  <span>Без обработчика содержимого</span>
                  <b>{status?.unsupported ?? 0}</b>
                </div>
                <button onClick={() => action("cancel")}>
                  Остановить обработку
                </button>
                <button
                  className="danger"
                  onClick={() => {
                    if (
                      window.confirm(
                        "Очистить поисковые данные и построить индекс заново? Исходные файлы останутся на месте.",
                      )
                    )
                      void action("rebuild");
                  }}
                >
                  Очистить и перестроить индекс
                </button>
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
