interface Props {
  hasApiKey: boolean;
  onOpenSettings: () => void;
}

export function Header({ hasApiKey, onOpenSettings }: Props) {
  return (
    <header className="bg-slate-900 border-b border-slate-800 sticky top-0 z-50 py-4 shadow-xl">
      <div className="max-w-7xl mx-auto px-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="bg-indigo-500 p-2 rounded-lg shadow-indigo-500/20 shadow-lg">
            <svg
              className="w-6 h-6 text-white"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth="2"
                d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z"
              />
            </svg>
          </div>
          <div>
            <h1 className="text-xl font-black text-white tracking-tighter uppercase leading-none">
              Video <span className="text-indigo-400">Cloner</span>
            </h1>
            <p className="text-[10px] text-slate-500 font-bold uppercase tracking-[0.2em] mt-1">
              AI Transcription &amp; Analysis Engine
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div
            className={`hidden sm:flex items-center gap-2 text-[10px] font-black px-4 py-2 rounded-full tracking-widest uppercase border ${
              hasApiKey
                ? "text-emerald-400 bg-emerald-500/5 border-emerald-500/20"
                : "text-amber-400 bg-amber-500/5 border-amber-500/20"
            }`}
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                hasApiKey ? "bg-emerald-500 animate-pulse" : "bg-amber-500"
              }`}
            />
            {hasApiKey ? "Hệ thống sẵn sàng" : "Chưa có API key"}
          </div>
          <button
            onClick={onOpenSettings}
            className="text-[10px] font-black text-slate-400 hover:text-white uppercase tracking-widest px-4 py-2 rounded-full border border-slate-800 hover:border-slate-700 hover:bg-slate-800 transition-all"
          >
            Cài đặt
          </button>
        </div>
      </div>
    </header>
  );
}
