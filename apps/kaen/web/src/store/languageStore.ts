import { create } from 'zustand';

export interface Language {
    id: number;
    code: string;
    name: string;
    flag: string;
    isActive: boolean;
}

// Static list — single-user local app, no /api/languages endpoint.
const STATIC_LANGUAGES: Language[] = [
    { id: 1, code: 'vi', name: 'Tiếng Việt', flag: '🇻🇳', isActive: true },
    { id: 2, code: 'vn', name: 'Tiếng Việt', flag: '🇻🇳', isActive: true },
    { id: 3, code: 'en', name: 'English', flag: '🇺🇸', isActive: true },
    { id: 4, code: 'ja', name: '日本語', flag: '🇯🇵', isActive: true },
    { id: 5, code: 'jp', name: '日本語', flag: '🇯🇵', isActive: true },
    { id: 6, code: 'ko', name: '한국어', flag: '🇰🇷', isActive: true },
    { id: 7, code: 'zh', name: '中文', flag: '🇨🇳', isActive: true },
    { id: 8, code: 'fr', name: 'Français', flag: '🇫🇷', isActive: true },
    { id: 9, code: 'de', name: 'Deutsch', flag: '🇩🇪', isActive: true },
    { id: 10, code: 'es', name: 'Español', flag: '🇪🇸', isActive: true },
];

interface LanguageState {
    languages: Language[];
    loading: boolean;
    error: string | null;
    fetchLanguages: () => Promise<void>;
    getLanguageByCode: (code: string) => Language | undefined;
}

export const useLanguageStore = create<LanguageState>()((set, get) => ({
    languages: STATIC_LANGUAGES,
    loading: false,
    error: null,

    fetchLanguages: async () => {
        set({ languages: STATIC_LANGUAGES, loading: false, error: null });
    },

    getLanguageByCode: (code: string) => {
        return get().languages.find((l) => l.code === code);
    },
}));
