import { useEffect, useRef, useState } from "react";
import { api, type Character, type Job, type Project } from "../lib/api";

interface Props {
  project: Project;
  scenesText: string;
  sceneCount: number;
  characters: Character[];
  running: boolean;
  job: Job | null;
  onChanged: () => void;
  onContinue: () => void;
}

export function ResultsPanel({
  project,
  scenesText,
  sceneCount,
  characters,
  running,
  job,
  onChanged,
  onContinue,
}: Props) {
  const [findText, setFindText] = useState("");
  const [replaceText, setReplaceText] = useState("");
  const [onlyWithDialogue, setOnlyWithDialogue] = useState(false);
  const [voiceOverrides, setVoiceOverrides] = useState<Record<string, "male" | "female">>({});
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const textAreaRef = useRef<HTMLTextAreaElement>(null);

  // Character ids change when a project is switched or re-analysed; stale
  // overrides would silently target a character that no longer exists.
  const projectId = project.id;
  useEffect(() => {
    setVoiceOverrides({});
    setFindText("");
    setReplaceText("");
  }, [projectId]);

  const applyReplace = async () => {
    if (!findText.trim() && Object.keys(voiceOverrides).length === 0) return;
    setApplying(true);
    setError(null);
    setNotice(null);
    try {
      const res = await api.replace(project.id, {
        find: findText,
        replace: replaceText,
        only_with_dialogue: onlyWithDialogue,
        voice_overrides: voiceOverrides,
      });
      setFindText("");
      setReplaceText("");
      setVoiceOverrides({});
      onChanged();
      setNotice(
        `Đã đồng bộ ${sceneCount} đoạn` +
          (res.voices_applied > 0 ? ` · ${res.voices_applied} lượt đổi giọng` : ""),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setApplying(false);
    }
  };

  const copyToClipboard = async () => {
    try {
      await navigator.clipboard.writeText(scenesText);
      setNotice("Đã sao chép nội dung phân tích!");
    } catch {
      // clipboard API is blocked in some iframe contexts; fall back to select
      textAreaRef.current?.select();
      setNotice("Không tự sao chép được — đã bôi đen, nhấn Cmd/Ctrl+C.");
    }
  };

  const toggleVoice = (charId: string, gender: "male" | "female") =>
    setVoiceOverrides((prev) => ({ ...prev, [charId]: gender }));

  return (
    <div className="bg-slate-900 rounded-3xl border border-slate-800 shadow-2xl flex flex-col h-full min-h-[600px] overflow-hidden">
      <div className="px-6 py-4 border-b border-slate-800 flex items-center justify-between bg-slate-900/50 backdrop-blur-xl">
        <div className="flex items-center gap-3">
          <div
            className={`w-2 h-2 rounded-full ${
              sceneCount > 0
                ? "bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.6)] animate-pulse"
                : "bg-slate-700"
            }`}
          />
          <span className="text-[10px] font-black text-slate-400 uppercase tracking-widest">
            Dữ liệu nội dung (JSON) · {sceneCount} đoạn · {sceneCount * 8}s
          </span>
        </div>
        {sceneCount > 0 && (
          <div className="flex gap-2">
            <button
              onClick={copyToClipboard}
              className="text-[10px] font-bold text-slate-400 hover:text-white px-3 py-1.5 rounded-lg hover:bg-slate-800 transition-all border border-transparent hover:border-slate-700"
            >
              SAO CHÉP
            </button>
            <a
              href={api.downloadUrl(project.id)}
              className="text-[10px] font-bold text-emerald-500 hover:text-emerald-400 px-3 py-1.5 rounded-lg hover:bg-slate-800 transition-all border border-transparent hover:border-emerald-500/20"
            >
              TẢI VỀ .TXT
            </a>
          </div>
        )}
      </div>

      <div className="flex-grow p-6 flex flex-col min-h-0">
        {sceneCount > 0 ? (
          <>
            <div className="mb-4 p-5 bg-slate-900 border border-slate-800 rounded-3xl space-y-4 shadow-xl">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <h4 className="text-[10px] font-black text-indigo-400 uppercase tracking-[0.2em]">
                  Chỉnh sửa Prompt hàng loạt
                </h4>
                <div className="flex flex-wrap items-center gap-4">
                  {characters.length > 0 && (
                    <div className="flex flex-wrap gap-3 items-center">
                      <span className="text-[8px] font-bold text-slate-600 uppercase">
                        Nhân vật:
                      </span>
                      {characters.map((char) => (
                        <div
                          key={char.id}
                          className="flex items-center bg-slate-800 rounded-lg p-1 border border-slate-700 gap-2"
                          title={
                            char.has_dialogue ? `${char.id} · có thoại` : `${char.id} · không thoại`
                          }
                        >
                          <button
                            onClick={() => setFindText(char.name)}
                            className="text-[8px] font-black text-indigo-300 px-2 py-0.5 hover:text-white transition-all uppercase"
                          >
                            {char.name}
                          </button>
                          <div className="flex bg-slate-950 rounded-md p-0.5">
                            <button
                              onClick={() => toggleVoice(char.id, "male")}
                              className={`text-[7px] px-2 py-0.5 rounded uppercase font-black transition-all ${
                                voiceOverrides[char.id] === "male"
                                  ? "bg-indigo-600 text-white"
                                  : "text-slate-600 hover:text-slate-400"
                              }`}
                            >
                              Nam
                            </button>
                            <button
                              onClick={() => toggleVoice(char.id, "female")}
                              className={`text-[7px] px-2 py-0.5 rounded uppercase font-black transition-all ${
                                voiceOverrides[char.id] === "female"
                                  ? "bg-pink-600 text-white"
                                  : "text-slate-600 hover:text-slate-400"
                              }`}
                            >
                              Nữ
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                  <label className="flex items-center gap-2 cursor-pointer group">
                    <input
                      type="checkbox"
                      checked={onlyWithDialogue}
                      onChange={(e) => setOnlyWithDialogue(e.target.checked)}
                      className="w-3 h-3 rounded border-slate-700 bg-slate-950 text-indigo-600 focus:ring-indigo-500 focus:ring-offset-slate-900"
                    />
                    <span className="text-[9px] font-black text-slate-500 group-hover:text-slate-300 uppercase tracking-widest transition-colors">
                      Chỉ thay nhân vật có thoại
                    </span>
                  </label>
                </div>
              </div>

              <div className="flex flex-wrap sm:flex-nowrap gap-3">
                <div className="flex-grow flex items-center gap-2 bg-slate-950 border border-slate-800 rounded-xl px-4 py-1.5 focus-within:border-indigo-500/50 transition-all">
                  <span className="text-[8px] font-bold text-slate-600 uppercase">Tìm</span>
                  <input
                    type="text"
                    placeholder="Tên cũ hoặc nội dung cần đổi..."
                    value={findText}
                    onChange={(e) => setFindText(e.target.value)}
                    className="w-full bg-transparent text-[10px] text-white outline-none py-1"
                  />
                </div>
                <div className="hidden sm:flex items-center text-slate-700 px-1">
                  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth="2"
                      d="M14 5l7 7-7 7"
                    />
                  </svg>
                </div>
                <div className="flex-grow flex items-center gap-2 bg-slate-950 border border-slate-800 rounded-xl px-4 py-1.5 focus-within:border-indigo-500/50 transition-all">
                  <span className="text-[8px] font-bold text-slate-600 uppercase">Đổi thành</span>
                  <input
                    type="text"
                    placeholder="Tên mới..."
                    value={replaceText}
                    onChange={(e) => setReplaceText(e.target.value)}
                    className="w-full bg-transparent text-[10px] text-white outline-none py-1"
                  />
                </div>
                <button
                  onClick={applyReplace}
                  disabled={
                    applying || (!findText.trim() && Object.keys(voiceOverrides).length === 0)
                  }
                  className="w-full sm:w-auto bg-indigo-600 hover:bg-indigo-500 text-white px-6 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all active:scale-95 disabled:opacity-50 disabled:cursor-not-allowed shadow-lg shadow-indigo-600/20"
                >
                  {applying ? "..." : "THỰC THI"}
                </button>
              </div>

              {notice && (
                <p className="text-[9px] font-bold text-emerald-400 uppercase tracking-widest">
                  {notice}
                </p>
              )}
              {error && (
                <p className="text-[9px] font-bold text-red-400 uppercase tracking-widest">
                  {error}
                </p>
              )}
            </div>

            <div className="flex-grow relative min-h-[240px]">
              <textarea
                ref={textAreaRef}
                readOnly
                value={scenesText}
                className="absolute inset-0 w-full h-full bg-slate-950 border border-slate-800 rounded-2xl p-6 font-mono text-[11px] text-indigo-300 focus:outline-none resize-none leading-relaxed shadow-inner scrollbar-slim"
              />
            </div>

            <div className="mt-6 flex justify-center">
              <button
                onClick={onContinue}
                disabled={running}
                className="group flex items-center gap-3 bg-slate-800 hover:bg-slate-700 text-white px-8 py-3 rounded-2xl font-black text-[10px] uppercase tracking-widest transition-all active:scale-95 disabled:opacity-50"
              >
                {running ? (
                  <span className="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                ) : (
                  <svg
                    className="w-4 h-4 text-indigo-400 group-hover:translate-x-1 transition-transform"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth="2"
                      d="M13 5l7 7-7 7M5 5l7 7-7 7"
                    />
                  </svg>
                )}
                Phân tích đoạn tiếp theo
              </button>
            </div>
          </>
        ) : running ? (
          <div className="flex-grow flex flex-col items-center justify-center space-y-6 text-center">
            <div className="relative">
              <div className="w-20 h-20 border-4 border-slate-800 border-t-indigo-500 rounded-full animate-spin" />
              <div className="absolute inset-0 flex items-center justify-center">
                <div className="w-10 h-10 border-2 border-indigo-500/20 rounded-full" />
              </div>
            </div>
            <div className="space-y-2">
              <p className="text-[10px] font-black text-slate-300 uppercase tracking-widest animate-pulse">
                Đang giải mã cấu trúc video...
              </p>
              <p className="text-[9px] text-slate-500 max-w-[240px] leading-relaxed">
                AI đang quan sát và trích xuất từng chi tiết nhỏ trong video của bạn. Video lớn phải
                tải lên Gemini trước nên lần đầu sẽ lâu hơn.
              </p>
            </div>
          </div>
        ) : (
          <div className="flex-grow flex flex-col items-center justify-center text-slate-700 space-y-4">
            <div className="p-8 bg-slate-950 rounded-full border border-slate-900">
              <svg
                className="w-16 h-16 opacity-10"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="1"
                  d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"
                />
              </svg>
            </div>
            <p className="text-[10px] font-black uppercase tracking-widest opacity-20">
              Sẵn sàng phân tích dữ liệu
            </p>
          </div>
        )}
      </div>

      {job?.status === "failed" && job.error && (
        <div className="mx-6 mb-6 p-4 bg-red-950/30 border border-red-500/30 rounded-2xl text-red-400 text-[10px] font-bold flex items-start gap-3">
          <svg
            className="w-4 h-4 shrink-0 mt-0.5"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
              d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
            />
          </svg>
          <span className="normal-case leading-relaxed">{job.error}</span>
        </div>
      )}
    </div>
  );
}
