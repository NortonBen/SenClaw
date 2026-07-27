import { useState } from "react";
import { api, type Presets } from "../lib/api";

interface Props {
  presets: Presets | null;
  hasApiKey: boolean;
  apiKeyFromEnv: boolean;
  defaultModel: string;
  onClose: () => void;
  onSaved: () => void;
}

export function SettingsModal({
  presets,
  hasApiKey,
  apiKeyFromEnv,
  defaultModel,
  onClose,
  onSaved,
}: Props) {
  const [key, setKey] = useState("");
  const [model, setModel] = useState(defaultModel);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.saveSettings({
        // An untouched field must not wipe the stored key.
        ...(key.trim() ? { gemini_api_key: key.trim() } : {}),
        default_model: model,
      });
      onSaved();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[100] bg-slate-950/80 backdrop-blur-sm flex items-center justify-center p-6">
      <div className="bg-slate-900 border border-slate-800 rounded-3xl shadow-2xl w-full max-w-lg p-6 space-y-6">
        <div className="flex items-center justify-between">
          <h2 className="text-xs font-black text-slate-300 uppercase tracking-widest">
            Cài đặt Video Cloner
          </h2>
          <button
            onClick={onClose}
            className="text-slate-600 hover:text-white transition-colors"
            aria-label="Đóng"
          >
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth="2"
                d="M6 18L18 6M6 6l12 12"
              />
            </svg>
          </button>
        </div>

        <div className="space-y-2">
          <label className="text-[9px] font-bold text-slate-500 uppercase ml-1">
            Gemini API Key
          </label>
          <input
            type="password"
            autoComplete="off"
            placeholder={
              hasApiKey ? "Đã lưu — nhập key mới để thay" : "Dán API key của Google AI Studio"
            }
            value={key}
            onChange={(e) => setKey(e.target.value)}
            className="w-full bg-slate-950 border border-slate-800 rounded-xl px-4 py-3 text-xs font-medium text-white focus:border-indigo-500 focus:outline-none transition-all"
          />
          <p className="text-[10px] text-slate-600 leading-relaxed px-1">
            Video Cloner gọi thẳng Gemini vì phải gửi kèm file video — bridge LLM của SenClaw
            không truyền được video và cũng không có temperature.
            {apiKeyFromEnv && (
              <>
                {" "}
                <span className="text-emerald-500 font-bold">
                  Đang dùng key từ biến môi trường.
                </span>
              </>
            )}
          </p>
        </div>

        <div className="space-y-2">
          <label className="text-[9px] font-bold text-slate-500 uppercase ml-1">
            Model mặc định cho dự án mới
          </label>
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

        {error && (
          <div className="p-3 bg-red-950/30 border border-red-500/30 rounded-xl text-red-400 text-[10px] font-bold">
            {error}
          </div>
        )}

        <button
          onClick={save}
          disabled={saving}
          className="w-full py-3 bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-800 disabled:text-slate-600 text-white rounded-2xl font-black text-[10px] uppercase tracking-widest transition-all active:scale-[0.98]"
        >
          {saving ? "Đang lưu..." : "Lưu cài đặt"}
        </button>
      </div>
    </div>
  );
}
