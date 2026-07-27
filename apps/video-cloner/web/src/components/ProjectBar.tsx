import { useRef, useState } from "react";
import { api, type Project } from "../lib/api";

interface Props {
  projects: Project[];
  activeId: number | null;
  uploading: boolean;
  onSelect: (id: number) => void;
  onUpload: (file: File) => void;
  onDeleted: () => void;
}

export function ProjectBar({
  projects,
  activeId,
  uploading,
  onSelect,
  onUpload,
  onDeleted,
}: Props) {
  const input = useRef<HTMLInputElement>(null);
  const [confirming, setConfirming] = useState<number | null>(null);

  const remove = async (id: number) => {
    await api.deleteProject(id);
    setConfirming(null);
    onDeleted();
  };

  return (
    <div className="bg-slate-900 border border-slate-800 rounded-3xl p-4 shadow-xl space-y-3">
      <div className="flex items-center justify-between">
        <h2 className="text-xs font-black text-slate-400 uppercase tracking-widest flex items-center gap-2">
          <span className="w-1.5 h-1.5 rounded-full bg-indigo-500" />
          Nguồn Video
        </h2>
        <span className="text-[9px] font-bold text-slate-600 uppercase">
          {projects.length} dự án
        </span>
      </div>

      <div className="relative group border-2 border-dashed border-slate-800 rounded-2xl p-4 text-center hover:border-indigo-500/50 hover:bg-indigo-500/5 transition-all">
        <input
          ref={input}
          type="file"
          accept="video/*"
          disabled={uploading}
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) {
              onUpload(f);
              e.target.value = "";
            }
          }}
          className="absolute inset-0 opacity-0 cursor-pointer z-10 disabled:cursor-wait"
        />
        <div className="space-y-2">
          {uploading ? (
            <div className="w-6 h-6 mx-auto border-2 border-slate-700 border-t-indigo-500 rounded-full animate-spin" />
          ) : (
            <svg
              className="w-7 h-7 mx-auto text-slate-600 group-hover:text-indigo-400 transition-colors"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth="2"
                d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12"
              />
            </svg>
          )}
          <p className="text-[10px] font-bold text-slate-500 uppercase">
            {uploading ? "Đang tải video lên..." : "Kéo thả hoặc nhấp để chọn video"}
          </p>
        </div>
      </div>

      {projects.length > 0 && (
        <div className="space-y-1.5 max-h-52 overflow-y-auto scrollbar-slim pr-1">
          {projects.map((p) => {
            const active = p.id === activeId;
            return (
              <div
                key={p.id}
                className={`w-full rounded-xl border px-3 py-2 transition-all flex items-center gap-2 ${
                  active
                    ? "bg-indigo-600/10 border-indigo-500/40"
                    : "bg-slate-950 border-slate-800 hover:border-slate-700"
                }`}
              >
                <button
                  onClick={() => onSelect(p.id)}
                  className="flex-grow text-left min-w-0"
                  title={p.video_filename}
                >
                  <p
                    className={`text-[10px] font-black uppercase tracking-wide truncate ${
                      active ? "text-indigo-300" : "text-slate-400"
                    }`}
                  >
                    {p.name}
                  </p>
                  <p className="text-[8px] font-bold text-slate-600 uppercase">
                    {p.scene_count ?? 0} đoạn
                    {p.running && <span className="text-amber-500"> · đang chạy</span>}
                  </p>
                </button>

                {confirming === p.id ? (
                  <div className="flex gap-1 shrink-0">
                    <button
                      onClick={() => void remove(p.id)}
                      className="text-[8px] font-black text-red-400 uppercase px-1.5 py-1 rounded hover:bg-red-500/10"
                    >
                      Xoá
                    </button>
                    <button
                      onClick={() => setConfirming(null)}
                      className="text-[8px] font-black text-slate-500 uppercase px-1.5 py-1 rounded hover:bg-slate-800"
                    >
                      Huỷ
                    </button>
                  </div>
                ) : (
                  <button
                    onClick={() => setConfirming(p.id)}
                    className="shrink-0 text-slate-700 hover:text-red-400 transition-colors p-1"
                    aria-label={`Xoá ${p.name}`}
                  >
                    <svg
                      className="w-3.5 h-3.5"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth="2"
                        d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                      />
                    </svg>
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
