import api from './api';

export interface Language {
    code: string;
    name: string;
    flag: string;
}

export const SUPPORTED_LANGUAGES: Language[] = [
    { code: 'vi', name: 'Vietnamese', flag: '🇻🇳' },
    { code: 'es', name: 'Spanish', flag: '🇪🇸' },
    { code: 'fr', name: 'French', flag: '🇫🇷' },
    { code: 'de', name: 'German', flag: '🇩🇪' },
    { code: 'ja', name: 'Japanese', flag: '🇯🇵' },
    { code: 'ko', name: 'Korean', flag: '🇰🇷' },
    { code: 'zh-CN', name: 'Chinese (Simplified)', flag: '🇨🇳' },
    { code: 'ru', name: 'Russian', flag: '🇷🇺' },
    { code: 'it', name: 'Italian', flag: '🇮🇹' },
    { code: 'pt', name: 'Portuguese', flag: '🇵🇹' },
    { code: 'id', name: 'Indonesian', flag: '🇮🇩' },
    { code: 'th', name: 'Thai', flag: '🇹🇭' },
];

const CACHE_KEY = 'kaen_translation_cache';

interface CacheData {
    [key: string]: string;
}

export interface DictionaryData {
    word: string;
    ipa: string;
    partOfSpeech?: string;
    definition: string;
    translatedDefinition?: string;
    examples: string[];
    translatedExamples?: string[];
    translation: string;
    audioUs: string;
    audioUk: string;
}

export const translationService = {
    translate: async (text: string, targetLang: string): Promise<string | null> => {
        if (!text || !targetLang || targetLang === 'en') return null;

        const cacheKey = `${targetLang}:${text.trim()}`;
        const cached = getCache();

        if (cached[cacheKey]) {
            return cached[cacheKey];
        }

        try {
            // Backend dictionary lookup first (server-side cached).
            const { data } = await api.get('/dictionary/lookup', {
                params: { word: text.trim(), targetLang },
            });

            if (data && data.translation) {
                updateCache(cacheKey, data.translation);
                return data.translation;
            }
        } catch (e) {
            console.warn('Backend translation failed, falling back to Google Translate', e);
        }

        try {
            // Fallback: public Google Translate endpoint.
            const sourceLang = 'en';
            const url = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=${sourceLang}&tl=${targetLang}&dt=t&q=${encodeURIComponent(text)}`;

            const response = await fetch(url);
            if (!response.ok) {
                console.error('External translation request failed');
                return null;
            }

            const data = await response.json();
            if (data && data[0] && data[0][0] && data[0][0][0]) {
                const translatedText = data[0][0][0];
                updateCache(cacheKey, translatedText);
                return translatedText;
            }

            return null;
        } catch (error) {
            console.error('Translation error:', error);
            return null;
        }
    },

    getDictionaryData: async (text: string, targetLang: string): Promise<DictionaryData | null> => {
        if (!text) return null;

        try {
            const { data } = await api.get('/dictionary/lookup', {
                params: { word: text, targetLang },
            });

            if (data) {
                return {
                    word: data.word,
                    ipa: data.ipa || '',
                    partOfSpeech: data.partOfSpeech || '',
                    definition: data.definition || '',
                    translatedDefinition: data.translatedDefinition || '',
                    examples: data.examples || [],
                    translatedExamples: data.translatedExamples || [],
                    translation: data.translation || '',
                    audioUs: data.audioUs || '',
                    audioUk: data.audioUk || '',
                };
            }
        } catch (e) {
            console.error('Failed to fetch dictionary data from backend', e);
        }

        // Fallback: minimal card, pronunciation handled by speechSynthesis.
        return {
            word: text,
            ipa: '',
            translation: '',
            audioUs: '',
            audioUk: '',
            definition: '',
            examples: [],
        };
    },
};

function getCache(): CacheData {
    try {
        const stored = localStorage.getItem(CACHE_KEY);
        return stored ? JSON.parse(stored) : {};
    } catch {
        return {};
    }
}

function updateCache(key: string, value: string) {
    try {
        const cache = getCache();
        cache[key] = value;
        localStorage.setItem(CACHE_KEY, JSON.stringify(cache));
    } catch (e) {
        console.error('Failed to update translation cache', e);
    }
}
