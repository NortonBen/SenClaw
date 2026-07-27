import { create } from 'zustand';
import api from '@/lib/api';

export interface Overview {
    dueNow: number;
    newAvailable: number;
    snoozedUntil: string | null;
    currentStreak: number;
    totalXP: number;
    dailyWordGoal: number;
    today: { newWordsToday: number; reviewedWordsToday: number };
    levels: {
        totalWords: number;
        totalLearned: number;
        newWords: number;
        byLevel: {
            level0: number; level1: number; level2: number; level3: number;
            level4: number; level5: number; level6Plus: number;
        };
    };
    learnedWords: number;
    library: {
        lessons: number;
        cards: number;
        grammars: number;
        grammarDue: number;
        stories: number;
        dictationLessons: number;
        dictationInProgress: number;
    };
    timezone: string;
    studySlots: string[];
    nextSlot: string | null;
}

interface OverviewState {
    data: Overview | null;
    loading: boolean;
    error: string | null;
    /** Fetch once per mount cycle; the sidebar badges and the dashboard share it. */
    load: (force?: boolean) => Promise<void>;
}

let inFlight: Promise<void> | null = null;

export const useOverviewStore = create<OverviewState>((set, get) => ({
    data: null,
    loading: false,
    error: null,
    load: async (force = false) => {
        if (!force && (get().data || inFlight)) {
            await inFlight;
            return;
        }
        set({ loading: true, error: null });
        inFlight = api
            .get('/study/overview')
            .then(({ data }) => set({ data, loading: false }))
            .catch((e: unknown) => {
                console.error('overview failed', e);
                set({ loading: false, error: 'Không tải được dữ liệu học tập' });
            })
            .finally(() => {
                inFlight = null;
            });
        await inFlight;
    },
}));
