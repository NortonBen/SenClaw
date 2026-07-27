import { useEffect, useRef, useState } from "react";
import { api, type CloneConfig, type Presets, type Project } from "../lib/api";

interface Props {
  project: Project;
  presets: Presets | null;
  sceneCount: number;
  running: boolean;
  onAnalyze: (mode: "start" | "continue" | "regenerate", cfg: CloneConfig) => void;
  onChanged: () => void;
}

export function ControlsPanel({
  project,
  presets,
  sceneCount,
  running,
  onAnalyze,
  onChanged,
}: Props) {
  const styles = presets?.styles ?? [];
  const knownStyle = styles.includes(project.style);

  const [isCustomStyle, setIsCustomStyle] = useState(!knownStyle);
  const [customStyleText, setCustomStyleText] = useState(knownStyle ? "" : project.style);
  const [selectedStyle, setSelectedStyle] = useState(
    knownStyle ? project.style : styles[0] ?? project.style,
  );
  const [model, setModel] = useState(project.model);
  const [charDescription, setCharDescription] = useState(project.char_description);
  const [customDialogue, setCustomDialogue] = useState(project.custom_dialogue);
  const [bgDescription, setBgDescription] = useState(project.bg_description);
  const [autoMagic, setAutoMagic] = useState(project.auto_magic);
  const [visualSimilarity, setVisualSimilarity] = useState(project.visual_similarity);
  const [hasCharImage, setHasCharImage] = useState(project.has_char_image);

  // Reset the form when the user switches project.
  const projectId = project.id;
  useEffect(() => {
    const known = styles.includes(project.style);
    setIsCustomStyle(!known);
    setCustomStyleText(known ? "" : project.style);
    setSelectedStyle(known ? project.style : styles[0] ?? project.style);
    setModel(project.model);
    setCharDescription(project.char_description);
    setCustomDialogue(project.custom_dialogue);
    setBgDescription(project.bg_description);
    setAutoMagic(project.auto_magic);
    setVisualSimilarity(project.visual_similarity);
    setHasCharImage(project.has_char_image);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  const imageInput = useRef<HTMLInputElement>(null);
  const currentStyle = isCustomStyle ? customStyleText : selectedStyle;

  const config = (): CloneConfig => ({
    style: currentStyle,
    model,
    char_description: charDescription,
    custom_dialogue: customDialogue,
    bg_description: bgDescription,
    auto_magic: autoMagic,
    visual_similarity: visualSimilarity,
  });

  const uploadCharImage = async (file: File) => {
    const form = new FormData();
    form.append("char_image", file);
    await api.uploadCharImage(project.id, form);
    setHasCharImage(true);
    onChanged();
  };

  const removeCharImage = async () => {
    await api.clearCharImage(project.id);
    setHasCharImage(false);
    if (imageInput.current) imageInput.current.value = "";
    onChanged();
  };

  const busy = running;
  const startDisabled = busy || (isCustomStyle && !customStyleText.trim());

  return (
    <div className="bg-slate-900 p-6 rounded-3xl border border-slate-800 shadow-2xl space-y-6">
      <div>
        <h2 className="text-xs font-black text-slate-400 uppercase tracking-widest mb-4 flex items-center gap-2">
          <span className="w-1.5 h-1.5 rounded-full bg-indigo-500" />
          Cấu hình Sao chép
        </h2>

        <div className="space-y-4">
          <div className="space-y-2">
            <label className="text-[9px] font-bold text-slate-500 uppercase ml-1">AI Model</label>
            <select
              value={model}
              onChange={(e) => setModel(e.target.value)}
              className="w-full bg-slate-950 border border-slate-800 rounded-xl px-4 py-3 text-xs font-bold text-slate-300 focus:border-indigo-500 focus:outline-none transition-all"
            >
              {(presets?.models ?? []).map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </div>

          <div className="space-y-2">
            <label className="text-[9px] font-bold text-slate-500 uppercase ml-1">
              Phong cách đích
            </label>
            <select
              value={isCustomStyle ? "custom" : selectedStyle}
              onChange={(e) => {
                if (e.target.value === "custom") {
                  setIsCustomStyle(true);
                } else {
                  setIsCustomStyle(false);
                  setSelectedStyle(e.target.value);
                }
              }}
              className="w-full bg-slate-950 border border-slate-800 rounded-xl px-4 py-3 text-xs font-bold text-slate-300 focus:border-indigo-500 focus:outline-none transition-all"
            >
              {styles.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
              <option value="custom">Tùy chỉnh phong cách...</option>
            </select>
            {isCustomStyle && (
              <input
                type="text"
                placeholder="Ví dụ: Hoạt hình Ghibli, 3D Pixar..."
                value={customStyleText}
                onChange={(e) => setCustomStyleText(e.target.value)}
                className="w-full mt-2 bg-slate-950 border border-indigo-500/30 rounded-xl px-4 py-3 text-xs font-medium text-white focus:border-indigo-500 outline-none transition-all"
              />
            )}
          </div>

          <div className="space-y-4 pt-2">
            <button
              onClick={() => setAutoMagic(!autoMagic)}
              className={`w-full group relative overflow-hidden rounded-2xl p-4 transition-all duration-500 border-2 ${
                autoMagic
                  ? "bg-indigo-600 border-indigo-400 shadow-[0_0_20px_rgba(79,70,229,0.4)]"
                  : "bg-slate-900 border-slate-800 hover:border-slate-700 shadow-xl"
              }`}
            >
              <div className="relative z-10 flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div
                    className={`p-2 rounded-xl transition-colors ${
                      autoMagic
                        ? "bg-indigo-400/30 text-white"
                        : "bg-slate-800 text-indigo-400 group-hover:text-indigo-300"
                    }`}
                  >
                    <svg
                      className={`w-5 h-5 ${autoMagic ? "animate-pulse" : ""}`}
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth="2"
                        d="M13 10V3L4 14h7v7l9-11h-7z"
                      />
                    </svg>
                  </div>
                  <div className="text-left">
                    <p
                      className={`text-[10px] font-black uppercase tracking-widest ${
                        autoMagic ? "text-white" : "text-slate-300"
                      }`}
                    >
                      Chế độ "AI Tự Do Sáng Tạo"
                    </p>
                    <p className="text-[8px] font-medium text-indigo-200/60 leading-tight">
                      Tự thay đổi nhân vật, độ tuổi &amp; bối cảnh hoàn toàn mới
                    </p>
                  </div>
                </div>
                <div
                  className={`w-10 h-5 rounded-full p-1 transition-colors ${
                    autoMagic ? "bg-white/20" : "bg-slate-950"
                  }`}
                >
                  <div
                    className={`w-3 h-3 rounded-full bg-white transition-transform ${
                      autoMagic
                        ? "translate-x-5 shadow-[0_0_10px_rgba(255,255,255,0.8)]"
                        : "translate-x-0"
                    }`}
                  />
                </div>
              </div>
              {autoMagic && (
                <div className="absolute inset-0 bg-gradient-to-r from-transparent via-white/5 to-transparent animate-shimmer pointer-events-none" />
              )}
            </button>

            {!autoMagic && (
              <div className="bg-slate-950/50 border border-slate-800 rounded-2xl p-4 space-y-3">
                <div className="flex items-center justify-between">
                  <label className="text-[9px] font-black text-slate-500 uppercase tracking-widest">
                    Độ tương đồng hình ảnh ({visualSimilarity}%)
                  </label>
                  <span
                    className={`text-[10px] font-bold ${
                      visualSimilarity > 80
                        ? "text-emerald-400"
                        : visualSimilarity > 40
                          ? "text-indigo-400"
                          : "text-orange-400"
                    }`}
                  >
                    {visualSimilarity === 100
                      ? "Nguyên bản"
                      : visualSimilarity === 0
                        ? "Sáng tạo nhất"
                        : "Remix"}
                  </span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="100"
                  step="10"
                  value={visualSimilarity}
                  onChange={(e) => setVisualSimilarity(parseInt(e.target.value, 10))}
                  className="w-full h-1.5 bg-slate-800 rounded-lg appearance-none cursor-pointer accent-indigo-500"
                />
                <div className="flex justify-between text-[7px] font-black text-slate-600 uppercase tracking-tighter">
                  <span>Sáng tạo (0%)</span>
                  <span>Trung thực (100%)</span>
                </div>
              </div>
            )}
          </div>

          <div
            className={`space-y-4 transition-all duration-500 ${
              autoMagic
                ? "opacity-30 grayscale pointer-events-none scale-[0.98]"
                : "opacity-100 grayscale-0 scale-100"
            }`}
          >
            <div className="space-y-2">
              <label className="text-[9px] font-bold text-slate-500 uppercase ml-1">
                Thay thế nhân vật chính (Tùy chọn)
              </label>
              <div className="flex flex-wrap gap-2 mb-2">
                {(presets?.characters ?? []).map((p) => (
                  <button
                    key={p.name}
                    onClick={() => setCharDescription(p.desc)}
                    className="text-[8px] font-black uppercase px-2 py-1 bg-slate-800 border border-slate-700 rounded-md text-slate-400 hover:bg-indigo-600 hover:text-white hover:border-indigo-500 transition-all"
                  >
                    {p.name}
                  </button>
                ))}
              </div>

              <div className="relative group border border-slate-800 bg-slate-950 rounded-xl p-3 flex items-center gap-3 hover:border-indigo-500/50 transition-all">
                <input
                  ref={imageInput}
                  type="file"
                  accept="image/*"
                  onChange={(e) => {
                    const f = e.target.files?.[0];
                    if (f) void uploadCharImage(f);
                  }}
                  className="absolute inset-0 opacity-0 cursor-pointer z-10"
                />
                <div className="w-10 h-10 rounded-lg bg-slate-900 flex items-center justify-center border border-slate-800 overflow-hidden">
                  <svg
                    className="w-5 h-5 text-slate-600"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth="2"
                      d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"
                    />
                  </svg>
                </div>
                <div className="flex-grow">
                  <p className="text-[9px] font-black text-slate-400 uppercase tracking-widest">
                    {hasCharImage ? "Đã có ảnh mẫu" : "Tải ảnh nhân vật mẫu"}
                  </p>
                  <p className="text-[8px] text-slate-600 uppercase font-bold">JPG, PNG...</p>
                </div>
                {hasCharImage && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void removeCharImage();
                    }}
                    className="relative z-20 text-[8px] font-black text-red-400 hover:text-red-300 uppercase px-2 py-1"
                  >
                    Xoá
                  </button>
                )}
              </div>

              <textarea
                placeholder="Hoặc mô tả bằng chữ: Một chàng trai tóc vàng, mặc áo khoác đỏ..."
                value={charDescription}
                onChange={(e) => setCharDescription(e.target.value)}
                className="w-full bg-slate-950 border border-slate-800 rounded-xl px-4 py-3 text-xs font-medium text-slate-300 focus:border-indigo-500 focus:outline-none transition-all min-h-[80px] resize-none"
              />
            </div>

            <div className="space-y-2">
              <label className="text-[9px] font-bold text-slate-500 uppercase ml-1">
                Thay thế lời thoại/Viral Quote (Tùy chọn)
              </label>
              <textarea
                placeholder="Nhập câu nói viral bạn muốn nhân vật nói thay cho lời thoại gốc..."
                value={customDialogue}
                onChange={(e) => setCustomDialogue(e.target.value)}
                className="w-full bg-slate-950 border border-slate-800 rounded-xl px-4 py-3 text-xs font-medium text-slate-300 focus:border-indigo-500 focus:outline-none transition-all min-h-[80px] resize-none"
              />
            </div>

            <div className="space-y-2">
              <label className="text-[9px] font-bold text-slate-500 uppercase ml-1">
                Bối cảnh/Background (Để trống để tự động làm mới)
              </label>
              <div className="flex flex-wrap gap-2 mb-2">
                {(presets?.backgrounds ?? []).map((p) => (
                  <button
                    key={p.name}
                    onClick={() => setBgDescription(p.desc)}
                    className="text-[8px] font-black uppercase px-2 py-1 bg-slate-800 border border-slate-700 rounded-md text-slate-400 hover:bg-emerald-600 hover:text-white hover:border-emerald-500 transition-all"
                  >
                    {p.name}
                  </button>
                ))}
              </div>
              <textarea
                placeholder="Nếu để trống, AI sẽ tự động tạo một bối cảnh mới rực rỡ phù hợp với phong cách đã chọn..."
                value={bgDescription}
                onChange={(e) => setBgDescription(e.target.value)}
                className="w-full bg-slate-950 border border-slate-800 rounded-xl px-4 py-3 text-xs font-medium text-slate-300 focus:border-indigo-500 focus:outline-none transition-all min-h-[80px] resize-none"
              />
            </div>
          </div>
        </div>
      </div>

      <div className="rounded-2xl overflow-hidden border border-slate-800 aspect-video bg-black shadow-2xl">
        {/* keyed so switching project reloads the source instead of keeping the old buffer */}
        <video
          key={project.id}
          src={api.videoUrl(project.id)}
          controls
          className="w-full h-full object-contain"
        />
      </div>

      <div className="space-y-2">
        <button
          onClick={() => onAnalyze("start", config())}
          disabled={startDisabled}
          className="w-full py-4 bg-indigo-600 text-white rounded-2xl font-black text-xs uppercase tracking-widest hover:bg-indigo-500 disabled:bg-slate-800 disabled:text-slate-600 shadow-xl shadow-indigo-600/20 active:scale-[0.98] transition-all"
        >
          {busy ? (
            <span className="flex items-center justify-center gap-2">
              <span className="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin" />
              Đang trích xuất dữ liệu...
            </span>
          ) : sceneCount > 0 ? (
            "Sao chép lại từ đầu"
          ) : (
            "Bắt đầu Sao chép nội dung"
          )}
        </button>

        {sceneCount > 0 && (
          <div className="flex gap-2">
            <button
              onClick={() => onAnalyze("regenerate", config())}
              disabled={busy}
              className="flex-1 bg-slate-800 hover:bg-slate-700 text-indigo-400 border border-indigo-500/20 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all active:scale-95 disabled:opacity-40"
            >
              Làm lại đoạn cuối
            </button>
            <button
              onClick={() => onAnalyze("continue", config())}
              disabled={busy}
              className="flex-1 bg-indigo-600/20 hover:bg-indigo-600/30 text-indigo-300 border border-indigo-500/40 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all active:scale-95 disabled:opacity-40"
            >
              Tạo tiếp
            </button>
          </div>
        )}

        {sceneCount > 0 && (
          <p className="text-[9px] text-amber-500/70 font-bold uppercase tracking-wide text-center pt-1">
            "Sao chép lại từ đầu" sẽ xoá {sceneCount} đoạn đã tạo
          </p>
        )}
      </div>
    </div>
  );
}
