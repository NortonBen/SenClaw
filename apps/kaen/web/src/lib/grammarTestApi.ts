import api from './api';

// Kaen backend routes (Rust, same-origin /api):
//   GET  /grammar-topics?level
//   GET  /grammar-topics/for-lesson/:grammarSlug
//   GET  /grammar-test/:topicId
//   POST /grammar-test/generate
//   POST /grammar-test/submit
//   GET  /grammar-test/results/:sessionId
// (kaizen used /grammar/topics + /grammar/tests/* — paths adapted here.)

export interface GrammarTopic {
  id: string;
  name: string;
  level: string;
  description?: string;
  /** Khi đề được sinh/gắn từ trang bài Grammar */
  grammarId?: string | null;
  grammarSlug?: string | null;
  /** Có khi backend map relation count hoặc khi lọc theo bài grammar */
  questionCount?: number;
}

export interface GrammarQuestion {
  id: string;
  content: string;
  options: { id: string; text: string }[];
  explanation?: string;
}

export interface TestSubmission {
  questionId: string;
  selectedAnswerId: string;
}

/** Một dòng kết quả sau khi chấm (options = JSON từ DB: { id, text }[]) */
export interface TestResultItem {
  questionId: string;
  content?: string | null;
  options?: unknown;
  selectedAnswerId: string;
  isCorrect: boolean;
  correctAnswerId?: string | null;
  explanation?: string | null;
}

export interface TestResult {
  sessionId: string;
  score: number;
  total: number;
  results: TestResultItem[];
}

export interface GrammarLessonTestMatch {
  topicId: string;
  name: string;
  level: string;
  questionCount: number;
}

export const grammarTestApi = {
  getTopics: async (level?: string) => {
    const response = await api.get('/grammar-topics', { params: { level } });
    return response.data as GrammarTopic[];
  },

  /** Chủ đề test khớp với bài học grammar (slug), hoặc null */
  getTopicForGrammarLesson: async (grammarSlug: string) => {
    const response = await api.get<GrammarLessonTestMatch | null>(
      `/grammar-topics/for-lesson/${encodeURIComponent(grammarSlug)}`,
    );
    return response.data;
  },

  getQuestions: async (topicId: string) => {
    const response = await api.get(`/grammar-test/${topicId}`);
    return response.data as GrammarQuestion[];
  },

  /** Gọi AI — có thể mất 30-120 giây; caller phải hiển thị loading rõ ràng. */
  generateAiTest: async (
    topicName: string,
    level: string,
    count: number,
    opts?: { grammarSlug?: string; grammarId?: string },
  ) => {
    const body: Record<string, unknown> = {
      topic: topicName,
      level,
      count,
    };
    if (opts?.grammarSlug?.trim()) body.grammarSlug = opts.grammarSlug.trim();
    if (opts?.grammarId?.trim()) body.grammarId = opts.grammarId.trim();
    const response = await api.post('/grammar-test/generate', body, {
      // AI generation is slow; don't let a default timeout kill it.
      timeout: 180_000,
    });
    return response.data as GrammarQuestion[];
  },

  submitTest: async (topicId: string, answers: TestSubmission[]) => {
    const response = await api.post('/grammar-test/submit', { topicId, answers });
    return response.data as TestResult;
  },

  getSessionResult: async (sessionId: string) => {
    const response = await api.get(`/grammar-test/results/${sessionId}`);
    return response.data as TestResult;
  },
};
