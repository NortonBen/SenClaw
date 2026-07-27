import { useState } from "react";
import { api } from "../lib/api";

interface Props {
  projectId: number;
  sceneCount: number;
}

type Busy = null | "file" | "wiki" | "dry" | "push";

export function ExportPanel({ projectId, sceneCount }: Props) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<Busy>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [orientation, setOrientation] = useState("HORIZONTAL");
  const [translate, setTranslate] = useState(true);
  const [preview, setPreview] = useState<string | null>(null);

  const empty = sceneCount === 0;

  const run = async (kind: Exclude<Busy, null>, fn: () => Promise<string>) => {
    setBusy(kind);
    setError(null);
    setNotice(null);
    try {
      setNotice(await fn());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const toFile = () =>
    run("file", async () => {
      const r = await api.exportToFile(projectId);
      return `Đã ghi ${r.scene_count} đoạn vào ${r.dir}`;
    });

  const toWiki = () =>
    run("wiki", async () => {
      const r = await api.exportToWiki(projectId);
      return `Đã đăng lên wiki: ${r.path}`;
    });

  const dryRun = () =>
    run("dry", async () => {
      const r = await api.handoffVideoFlow(projectId, {
        orientation,
        translate,
        dry_run: true,
      });
      const scenes = r.plan?.scenes?.length ?? 0;
      const entities = r.plan?.entities?.length ?? 0;
      setPreview(JSON.stringify(r.plan?.scenes?.[0] ?? {}, null, 2));
      return `Sẽ tạo bên video-flow: ${scenes} đoạn, ${entities} nhân vật${
        r.translated_scenes ? ` · đã dịch ${r.translated_scenes} đoạn` : ""
      }`;
    });

  const push = () =>
    run("push", async () => {
      const r = await api.handoffVideoFlow(projectId, {
        orientation,
        translate,
        dry_run: false,
      });
      setPreview(null);
      return `Đã bàn giao: ${r.scenes_created} đoạn, ${r.entities_created} nhân vật. Project video-flow: ${r.project_id}`;
    });

  return (
    <div className="bg-slate-900 border border-slate-800 rounded-3xl shadow-xl overflow-hidden">
      <button
        onClick={() => setOpen(!open)}
        className="w-full px-6 py-4 flex items-center justify-between hover:bg-slate-800/40 transition-colors"
      >
        <h2 className="text-xs font-black text-slate-400 uppercase tracking-widest flex items-center gap-2">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
          Xuất &amp; Bàn giao
        </h2>
        <svg
          className={`w-4 h-4 text-slate-600 transition-transform ${open ? "rotate-180" : ""}`}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      {open && (
        <div className="px-6 pb-6 space-y-5">
          {empty ? (
            <p className="text-[10px] text-slate-600 leading-relaxed">
              Chưa có đoạn nào để xuất. Chạy phân tích trước.
            </p>
          ) : (
            <>
              {notice && (
                <p className="text-[9px] font-bold text-emerald-400 uppercase tracking-widest leading-relaxed">
                  {notice}
                </p>
              )}
              {error && (
                <p className="text-[9px] font-bold text-red-400 tracking-wide leading-relaxed">
                  {error}
                </p>
              )}

              <div className="space-y-2">
                <p className="text-[9px] font-black text-slate-600 uppercase tracking-widest">
                  Tải về
                </p>
                <div className="flex gap-2">
                  <a
                    href={api.bundleUrl(projectId)}
                    className="flex-1 text-center bg-slate-950 border border-slate-800 hover:border-slate-700 text-slate-300 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all"
                  >
                    Bundle .json
                  </a>
                  <a
                    href={api.markdownUrl(projectId)}
                    className="flex-1 text-center bg-slate-950 border border-slate-800 hover:border-slate-700 text-slate-300 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all"
                  >
                    Kịch bản .md
                  </a>
                </div>
                <p className="text-[9px] text-slate-600 leading-relaxed">
                  Bundle chứa cả prompt đã làm phẳng (khung hình / diễn biến) lẫn JSON Veo 3 gốc.
                </p>
              </div>

              <div className="space-y-2">
                <p className="text-[9px] font-black text-slate-600 uppercase tracking-widest">
                  Lưu vào SenClaw
                </p>
                <div className="flex gap-2">
                  <button
                    onClick={toFile}
                    disabled={busy !== null}
                    className="flex-1 bg-slate-950 border border-slate-800 hover:border-slate-700 text-slate-300 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all disabled:opacity-40"
                  >
                    {busy === "file" ? "..." : "Thư mục chia sẻ"}
                  </button>
                  <button
                    onClick={toWiki}
                    disabled={busy !== null}
                    className="flex-1 bg-slate-950 border border-slate-800 hover:border-slate-700 text-slate-300 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all disabled:opacity-40"
                  >
                    {busy === "wiki" ? "..." : "Wiki"}
                  </button>
                </div>
              </div>

              <div className="space-y-3 pt-1 border-t border-slate-800">
                <p className="text-[9px] font-black text-slate-600 uppercase tracking-widest pt-3">
                  Bàn giao sang video-flow
                </p>

                <div className="flex gap-2">
                  {(["HORIZONTAL", "VERTICAL"] as const).map((o) => (
                    <button
                      key={o}
                      onClick={() => setOrientation(o)}
                      className={`flex-1 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all border ${
                        orientation === o
                          ? "bg-indigo-600/20 border-indigo-500/40 text-indigo-300"
                          : "bg-slate-950 border-slate-800 text-slate-500 hover:border-slate-700"
                      }`}
                    >
                      {o === "HORIZONTAL" ? "Ngang 16:9" : "Dọc 9:16"}
                    </button>
                  ))}
                </div>

                <label className="flex items-center gap-2 cursor-pointer group">
                  <input
                    type="checkbox"
                    checked={translate}
                    onChange={(e) => setTranslate(e.target.checked)}
                    className="w-3 h-3 rounded border-slate-700 bg-slate-950 text-indigo-600 focus:ring-indigo-500 focus:ring-offset-slate-900"
                  />
                  <span className="text-[9px] font-bold text-slate-500 group-hover:text-slate-300 uppercase tracking-widest transition-colors">
                    Dịch prompt sang tiếng Anh
                  </span>
                </label>
                <p className="text-[9px] text-slate-600 leading-relaxed">
                  video-flow đưa thẳng prompt cho Veo 3 nên tiếng Anh cho kết quả tốt hơn. Lời
                  thoại vẫn giữ nguyên tiếng Việt vì nó dùng để lồng tiếng.
                </p>

                <div className="flex gap-2">
                  <button
                    onClick={dryRun}
                    disabled={busy !== null}
                    className="flex-1 bg-slate-800 hover:bg-slate-700 text-slate-300 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all disabled:opacity-40"
                  >
                    {busy === "dry" ? "..." : "Xem trước"}
                  </button>
                  <button
                    onClick={push}
                    disabled={busy !== null}
                    className="flex-1 bg-emerald-600 hover:bg-emerald-500 text-white py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all disabled:opacity-40"
                  >
                    {busy === "push" ? "Đang gửi..." : "Bàn giao"}
                  </button>
                </div>

                {preview && (
                  <pre className="p-3 bg-slate-950 border border-slate-800 rounded-xl text-[9px] font-mono text-slate-500 max-h-52 overflow-auto scrollbar-slim whitespace-pre-wrap break-all">
                    {preview}
                  </pre>
                )}

                <p className="text-[9px] text-amber-500/70 leading-relaxed">
                  Cần video-flow đang chạy. Sau khi bàn giao, bên đó hãy chạy workflow — đừng chạy
                  “pipeline create”, nó sẽ xoá sạch các đoạn vừa nhận.
                </p>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
