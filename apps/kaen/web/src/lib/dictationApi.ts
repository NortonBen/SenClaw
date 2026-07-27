import api from './api';

export interface DictationTopic {
    id: number;
    name: string;
    slug: string;
    description?: string;
    level?: string;
    lessonCount?: number;
}

export interface DictationLesson {
    id: number;
    title: string;
    topic: string;
    description?: string;
    level: string;
    /** Full lesson audio; segments are played by seeking within this file. */
    audioUrl: string;
    youtubeVideoId?: string;
    mode: 'dictation' | 'pronunciation';
    dictationTopic?: DictationTopic | null;
    userProgress?: {
        percentage: number;
        hasMark: boolean;
    } | null;
}

export interface DictationLessonSegment {
    id: number;
    content: string;
    solutions: string[][];
    startTime: number;
    endTime: number;
    orderIndex: number;
}

export interface DictationLessonDetail extends DictationLesson {
    segments: DictationLessonSegment[];
}

/** History entry: the lesson fields flattened + completionPercentage/lastPracticedAt. */
export interface UserDictationProgress extends DictationLesson {
    completionPercentage?: number;
    lastPracticedAt: string;
}

export const dictationApi = {
    getTopics: async (): Promise<DictationTopic[]> => {
        const response = await api.get('/dictation-lessons/topics');
        return response.data;
    },

    getLessons: async (topic: string, page = 1, limit = 20): Promise<{ data: DictationLesson[], total: number }> => {
        const response = await api.get('/dictation-lessons', {
            params: { topic, page, limit },
        });
        return response.data;
    },

    getLesson: async (id: number): Promise<DictationLessonDetail> => {
        const response = await api.get(`/dictation-lessons/${id}`);
        return response.data;
    },

    saveProgress: async (lessonId: number, currentIndex: number, segmentStatus: Record<number, string>): Promise<unknown> => {
        const response = await api.post(`/dictation-lessons/${lessonId}/progress`, {
            currentIndex,
            segmentStatus,
        });
        return response.data;
    },

    getProgress: async (lessonId: number): Promise<{ currentIndex: number, segmentStatus: Record<number, string> } | null> => {
        const response = await api.get(`/dictation-lessons/${lessonId}/progress`);
        return response.data;
    },

    getHistory: async (): Promise<UserDictationProgress[]> => {
        const response = await api.get('/dictation-lessons/history/me');
        return response.data;
    },

    lookupWord: async (word: string, targetLang = 'vi'): Promise<Record<string, unknown>> => {
        const response = await api.get('/dictionary/lookup', {
            params: { word, targetLang },
        });
        return response.data;
    },

    getAudioUrl: async (word: string): Promise<string | null> => {
        const response = await api.get('/dictionary/audio', {
            params: { word },
        });
        return response.data.audioUrl;
    },
};
