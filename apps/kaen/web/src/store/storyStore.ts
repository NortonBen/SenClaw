import { create } from 'zustand';
import api from '@/lib/api';

export interface Story {
  id: string;
  title: string;
  topic?: string;
  description?: string;
  lessonId: string;
  createdAt: string;
  lesson?: {
    id: string;
    title: string;
  };
  progress?: {
    currentStep: number;
    completedSteps: number[];
    lastAccessedAt?: string;
  };
}

interface StoryStoreState {
  stories: Story[];
  loading: boolean;
  error: string | null;
  fetchStories: () => Promise<void>;
  deleteStory: (id: string) => Promise<void>;
}

export const useStoryStore = create<StoryStoreState>((set, get) => ({
  stories: [],
  loading: false,
  error: null,
  fetchStories: async () => {
    set({ loading: true, error: null });
    try {
      const { data } = await api.get<Story[]>('/stories');
      set({ stories: data || [], loading: false });
    } catch (error: any) {
      console.error('Failed to load stories:', error);
      set({
        error: error.response?.data?.message || 'Không thể tải danh sách story.',
        loading: false,
      });
    }
  },
  deleteStory: async (id: string) => {
    try {
      await api.delete(`/stories/${id}`);
      const { stories } = get();
      set({ stories: stories.filter((story) => story.id !== id) });
    } catch (error: any) {
      console.error('Failed to delete story:', error);
      throw error;
    }
  },
}));
