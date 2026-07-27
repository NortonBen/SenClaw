import { create } from 'zustand';
import api from '@/lib/api';

interface User {
  id: string;
  email: string;
  username: string;
  fullName?: string;
  avatarUrl?: string;
  bio?: string;
  nativeLanguage: string;
  studySlots: string[];
  currentStreak: number;
  totalXP: number;
  snoozeUntil?: string;
  timezone?: string;
  dailyWordGoal: number;
  totalDictationsCompleted: number;
  totalAiSentencesCreated: number;
}

interface AuthState {
  user: User | null;
  /** Single-user local app: always "logged in". Kept for compatibility with old checks. */
  token: string;
  isLoading: boolean;
  fetchProfile: () => Promise<void>;
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  token: 'local',
  isLoading: false,

  fetchProfile: async () => {
    try {
      const { data } = await api.get('/users/profile');
      set({ user: data });
    } catch (error) {
      console.error('Failed to fetch profile:', error);
    }
  },
}));
