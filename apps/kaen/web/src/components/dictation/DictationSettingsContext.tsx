import { useState, useEffect, createContext, useContext, ReactNode } from 'react';

export interface DictationSettings {
    replayKey: string;
    playPauseKey: string;
    autoReplay: number; // 0 = No, n = n times
    secondsBetweenReplays: number;
    wordSuggestions: boolean;
    showShortcutTips: boolean;
}

const DEFAULT_SETTINGS: DictationSettings = {
    replayKey: 'Ctrl',
    playPauseKey: '`',
    autoReplay: 0,
    secondsBetweenReplays: 0.5,
    wordSuggestions: true,
    showShortcutTips: true,
};

const STORAGE_KEY = 'dictation-settings';

interface DictationSettingsContextType {
    settings: DictationSettings;
    updateSettings: (updates: Partial<DictationSettings>) => void;
    resetSettings: () => void;
}

const DictationSettingsContext = createContext<DictationSettingsContextType | null>(null);

export function useDictationSettings() {
    const context = useContext(DictationSettingsContext);
    if (!context) {
        throw new Error('useDictationSettings must be used within a DictationSettingsProvider');
    }
    return context;
}

interface DictationSettingsProviderProps {
    children: ReactNode;
}

export function DictationSettingsProvider({ children }: DictationSettingsProviderProps) {
    const [settings, setSettings] = useState<DictationSettings>(() => {
        const stored = localStorage.getItem(STORAGE_KEY);
        if (stored) {
            try {
                return { ...DEFAULT_SETTINGS, ...JSON.parse(stored) };
            } catch {
                return DEFAULT_SETTINGS;
            }
        }
        return DEFAULT_SETTINGS;
    });

    useEffect(() => {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    }, [settings]);

    const updateSettings = (updates: Partial<DictationSettings>) => {
        setSettings(prev => ({ ...prev, ...updates }));
    };

    const resetSettings = () => {
        setSettings(DEFAULT_SETTINGS);
    };

    return (
        <DictationSettingsContext.Provider value={{ settings, updateSettings, resetSettings }}>
            {children}
        </DictationSettingsContext.Provider>
    );
}
