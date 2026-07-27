import { useCallback, useEffect, useState } from "react";
import { api, type Job, type Snapshot } from "../lib/api";

interface Props {
  projectId: number;
  running: boolean;
  /** Bumped by the parent whenever scenes change, to refetch history. */
  version: number;
  onRestored: () => void;
}

const REASON_LABEL: Record<Snapshot["reason"], string> = {
  analyze_start: "Phân tích lại từ đầu",
  analyze_regenerate: "Làm lại đoạn cuối",
  replace: "Sửa hàng loạt",
  restore: "Khôi phục",
};

const KIND_LABEL: Record<string, string> = {
  start: "Từ đầu",
  continue: "Tạo tiếp",
  regenerate: "Làm lại đoạn cuối",
};

function when(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleString("vi-VN", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
}

export function HistoryPanel({ projectId, running, version, onRestored }: Props) {
  const [open, setOpen] = useState(false);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rawFor, setRawFor] = useState<{ id: number; text: string } | null>(null);

  const load = useCallback(async () => {
    try {
      const [j, s] = await Promise.all([api.jobs(projectId), api.snapshots(projectId)]);
      setJobs(j.jobs);
      setSnapshots(s.snapshots);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [projectId]);

  useEffect(() => {
    setConfirming(null);
    setNotice(null);
    setRawFor(null);
    if (open) void load();
  }, [open, load, version]);

  const restore = async (snapshotId: number) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const res = await api.restore(projectId, snapshotId);
      setConfirming(null);
      setNotice(`Đã khôi phục ${res.restored_scenes} đoạn. Bản vừa thay cũng đã được lưu lại.`);
      onRestored();
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const showRaw = async (jobId: number) => {
    if (rawFor?.id === jobId) {
      setRawFor(null);
      return;
    }
    try {
      const res = await api.jobRaw(jobId);
      setRawFor({ id: jobId, text: res.raw || "(trống)" });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="bg-slate-900 border border-slate-800 rounded-3xl shadow-xl overflow-hidden">
      <button
        onClick={() => setOpen(!open)}
        className="w-full px-6 py-4 flex items-center justify-between hover:bg-slate-800/40 transition-colors"
      >
        <h2 className="text-xs font-black text-slate-400 uppercase tracking-widest flex items-center gap-2">
          <span className="w-1.5 h-1.5 rounded-full bg-indigo-500" />
          Lịch sử &amp; Khôi phục
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
          {notice && (
            <p className="text-[9px] font-bold text-emerald-400 uppercase tracking-widest">
              {notice}
            </p>
          )}
          {error && (
            <p className="text-[9px] font-bold text-red-400 uppercase tracking-widest">{error}</p>
          )}

          <div className="space-y-2">
            <p className="text-[9px] font-black text-slate-600 uppercase tracking-widest">
              Điểm khôi phục ({snapshots.length})
            </p>
            {snapshots.length === 0 ? (
              <p className="text-[10px] text-slate-600 leading-relaxed">
                Chưa có. App tự lưu một bản ngay trước mỗi thao tác ghi đè — phân tích lại từ đầu,
                làm lại đoạn cuối, hoặc sửa hàng loạt.
              </p>
            ) : (
              <div className="space-y-1.5 max-h-64 overflow-y-auto scrollbar-slim pr-1">
                {snapshots.map((s) => (
                  <div
                    key={s.id}
                    className="bg-slate-950 border border-slate-800 rounded-xl px-3 py-2 flex items-center gap-3"
                  >
                    <div className="flex-grow min-w-0">
                      <p className="text-[10px] font-black text-slate-400 uppercase tracking-wide truncate">
                        {REASON_LABEL[s.reason] ?? s.reason}
                      </p>
                      <p className="text-[8px] font-bold text-slate-600 truncate">
                        {s.scene_count} đoạn · {when(s.created_at)}
                        {s.label && ` · ${s.label}`}
                      </p>
                    </div>
                    {confirming === s.id ? (
                      <div className="flex gap-1 shrink-0">
                        <button
                          onClick={() => void restore(s.id)}
                          disabled={busy}
                          className="text-[8px] font-black text-emerald-400 uppercase px-2 py-1 rounded hover:bg-emerald-500/10 disabled:opacity-40"
                        >
                          Xác nhận
                        </button>
                        <button
                          onClick={() => setConfirming(null)}
                          className="text-[8px] font-black text-slate-500 uppercase px-2 py-1 rounded hover:bg-slate-800"
                        >
                          Huỷ
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={() => setConfirming(s.id)}
                        disabled={running}
                        title={running ? "Đang chạy phân tích" : undefined}
                        className="shrink-0 text-[8px] font-black text-indigo-400 hover:text-indigo-300 uppercase px-2 py-1 rounded hover:bg-indigo-500/10 disabled:opacity-30"
                      >
                        Khôi phục
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="space-y-2">
            <p className="text-[9px] font-black text-slate-600 uppercase tracking-widest">
              Các lượt bóc tách ({jobs.length})
            </p>
            {jobs.length === 0 ? (
              <p className="text-[10px] text-slate-600">Chưa chạy lượt nào.</p>
            ) : (
              <div className="space-y-1.5 max-h-64 overflow-y-auto scrollbar-slim pr-1">
                {jobs.map((j) => (
                  <div key={j.id} className="bg-slate-950 border border-slate-800 rounded-xl">
                    <div className="px-3 py-2 flex items-center gap-3">
                      <span
                        className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                          j.status === "completed"
                            ? "bg-emerald-500"
                            : j.status === "failed"
                              ? "bg-red-500"
                              : "bg-amber-500 animate-pulse"
                        }`}
                      />
                      <div className="flex-grow min-w-0">
                        <p className="text-[10px] font-black text-slate-400 uppercase tracking-wide truncate">
                          #{j.id} · {KIND_LABEL[j.kind] ?? j.kind}
                          {j.status === "completed" && ` · +${j.scenes_added} đoạn`}
                        </p>
                        <p className="text-[8px] font-bold text-slate-600 truncate">
                          {j.status === "failed" ? j.error : `${j.status}`}
                        </p>
                      </div>
                      <button
                        onClick={() => void showRaw(j.id)}
                        className="shrink-0 text-[8px] font-black text-slate-500 hover:text-slate-300 uppercase px-2 py-1 rounded hover:bg-slate-800"
                      >
                        {rawFor?.id === j.id ? "Ẩn" : "Raw"}
                      </button>
                    </div>
                    {rawFor?.id === j.id && (
                      <pre className="mx-3 mb-3 p-3 bg-slate-900 border border-slate-800 rounded-lg text-[9px] font-mono text-slate-500 max-h-40 overflow-auto scrollbar-slim whitespace-pre-wrap break-all">
                        {rawFor.text}
                      </pre>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
