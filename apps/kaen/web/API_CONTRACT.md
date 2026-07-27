# Kaen Web — API Contract

Every endpoint the trimmed frontend actually calls. Base URL is same-origin **`/api`** (axios `baseURL: '/api'`, no auth headers — single-user local app). All field names are **camelCase**, mirroring kaizen.

`Card` shape used throughout (fields the UI reads):

```ts
interface Card {
  id: string;
  lessonId?: string;
  word: string;
  explain: string;                       // English definition
  meanings?: Record<string, string>;     // e.g. { "vi": "Quả táo" } — UI reads meanings['vi'] (fallback 'vn')
  examples?: string[];
  ipa?: string;
  partOfSpeech?: string;
  imageUrl?: string;
  progress?: {                           // per-card SRS progress (study/review/learned endpoints)
    level: number;                       // 0 = new; Study page filters on progress.level
    lastReviewed?: string | null;        // ISO datetime
    nextReview?: string | null;          // ISO datetime
  };
}
```

Paginated list envelope (lessons + learned-cards):

```ts
{ total: number, totalPages: number, hasNext: boolean, hasPrevious: boolean, page?: number, limit?: number }
```

---

## Users

### GET `/api/users/profile`
Called on app start (`authStore.fetchProfile`) and after snooze.
Response fields the UI reads:
```ts
{
  id: string, email: string, username: string,
  fullName?: string, bio?: string,
  nativeLanguage: string,          // 'vi' | 'vn' | 'en' | ... (used as meanings key + display)
  studySlots: string[],            // e.g. ["08:00", "20:00"]
  currentStreak: number, totalXP: number,
  snoozeUntil?: string,            // ISO UTC datetime; UI compares vs now + formats HH:mm local
  dailyWordGoal: number
}
```

### POST `/api/users/snooze`
Body: `{ durationHours: number }` (1 | 3 | 24).
Response body unused (status only); UI re-fetches profile afterwards (expects `snoozeUntil` updated).

---

## Study

### GET `/api/study/session` and GET `/api/study/session?lessonId=<id>`
Response:
```ts
{ cards: Card[], lesson?: { title: string } }   // lesson only meaningful with ?lessonId
```
Cards must carry `progress` (Study splits new words `!progress || progress.level === 0` vs review words `progress.level > 0`).

### POST `/api/study/review/:cardId`
Body: `{ result: 'REMEMBER' | 'FORGOT', mode: 'FLIP' }`. Response unused.

### POST `/api/study/review-batch`
Body:
```ts
{
  reviews: Array<{ cardId: string, result: 'REMEMBER' | 'FORGOT', mode: 'FLIP' }>,
  durationSeconds: number,
  newWordsLearned: number,
  cardsReviewed: number,
  gameScore: number
}
```
Response unused.

### GET `/api/study/spaced-repetition/:reviewNotificationId`
Response: `{ cards: Card[] }` (UI counts `!card.progress` as new words).
Note: reachable via route `/spaced-repetition/:reviewNotificationId` (deep link; the in-app notification bell was removed).

### POST `/api/study/spaced-repetition/review/:cardId`
Body: `{ result: 'REMEMBER' | 'FORGOT', mode: 'FLIP' | 'TYPING' }`. Response unused.

### POST `/api/study/log`
Body: `{ durationSeconds: number, newWordsLearned: number, cardsReviewed: number, gameScore: number }`. Response unused.

### GET `/api/study/learned-cards`
Query: `page`, `limit` (numbers).
Response:
```ts
{
  cards: Card[],                  // incl. progress { level, lastReviewed, nextReview }
  page: number, limit: number, total: number,
  totalPages: number, hasNext: boolean, hasPrevious: boolean
}
```

### GET `/api/study/statistics/level`
Response (read by Home + Profile):
```ts
{
  totalWords: number,
  totalLearned: number,
  newWords: number,
  byLevel: {
    level0: number, level1: number, level2: number, level3: number,
    level4: number, level5: number, level6Plus: number
  }
}
```

### GET `/api/study/statistics/today`
Response: `{ newWordsToday: number, reviewedWordsToday: number }`

---

## Lessons

### GET `/api/lessons`
Query: `search?` (string), `page`, `limit`.
Response:
```ts
{
  lessons: Array<{ id: string, title: string, description?: string, createdAt: string /* ISO UTC */, cardCount: number }>,
  total: number, totalPages: number, hasNext: boolean, hasPrevious: boolean
}
```

### GET `/api/lessons/my-and-marked`
Same query params and response shape as GET `/api/lessons` (single-user: effectively "all my lessons"; ManageLessons page uses this one).

### POST `/api/lessons`
Body: `{ title: string, description?: string }` (visibility/tags no longer sent).
Response: created lesson — UI reads `id` only.

### GET `/api/lessons/:id`
Response:
```ts
{
  id: string, title: string, description?: string, createdAt: string,
  cards: Array<{ id: string, word: string, meanings?: Record<string,string>,
                 examples?: string[], ipa?: string, partOfSpeech?: string,
                 explain?: string, imageUrl?: string }>
}
```
404 → UI alerts "not found" and navigates away.

### GET `/api/lessons/:id/cards`
Response: `{ cards: Card[], lesson?: { title: string } }` (StudyLesson/ReviewLesson read `lesson.title`; cards should carry `progress` where available so flip-study can show state).

### PATCH `/api/lessons/:id`
Body: `{ title: string, description?: string }`. Response unused.

### DELETE `/api/lessons/:id`
Response unused (status only).

### POST `/api/lessons/:id/cards`
Body:
```ts
{
  word: string,
  meanings: Record<string, string>,   // may be {} (Study quick-import sends only word + examples)
  examples?: string[],
  ipa?: string, partOfSpeech?: string, explain?: string
}
```
Response unused.

### PATCH `/api/lessons/:lessonId/cards/:cardId`
Body: same shape as POST card. Response unused.

### DELETE `/api/lessons/:lessonId/cards/:cardId`
Response unused.

---

## Review practice

### GET `/api/review/session`
Query: `allowRepeat` — `'true'` or omitted (also sent literally as `false` once from Review mount; treat any value other than `'true'` as false).
Response: `{ cards: Card[] }`.

### GET `/api/review/session/lesson/:lessonId`
Query: `allowRepeat: 'true'`.
Response: `{ cards: Card[], lesson?: { title: string } }`.

### POST `/api/review/submit/batch`
Body: `{ results: Array<{ cardId: string, isCorrect: boolean }> }`.
Response: logged only; suggested shape `{ success: boolean, submitted: number, total: number }`.

---

## Listening / Writing / Matching practice

Identical pattern for the three domains:

### GET `/api/listening/session`, GET `/api/writing/session`, GET `/api/matching/session`
Response: `{ cards: Card[] }` — cards need `word` and `meanings['vi']` (question/answer generation), plus `ipa`/`explain` optionally.

### POST `/api/listening/submit/:cardId`, POST `/api/writing/submit/:cardId`, POST `/api/matching/submit/:cardId`
Body: `{ isCorrect: boolean }`. Response unused.

---

## Grammar

Pages: `/grammar` (list + "New Grammar" form), `/grammar/:slug` (detail + inline quiz), `/grammar-tests` (topics), `/grammar-tests/generate` (AI test), `/grammar-tests/:topicId` (test session), `/grammar-tests/results/:sessionId` (result).

Level enum everywhere: `'A1' | 'A2' | 'B1' | 'B1-B2' | 'B2' | 'C1' | 'OTHER'`.

### GET `/api/grammar/public`
Query: `page`, `limit` (15), `level?`, `search?`, `study?` (`completed` | `pending`; omitted for "all").
Response:
```ts
{
  items: Array<{
    id: string, title: string, slug: string, description: string, level: Level,
    viewCount: number, createdAt: string,
    studyProgress: null | {
      firstPassedAt: string | null, lastTestAt: string | null,
      nextReminderAt: string | null, dueForReview: boolean
    }
  }>,
  total: number, page: number, limit: number, totalPages: number
}
```

### GET `/api/grammar/:idOrSlug`
Same item shape plus `content` (markdown or legacy HTML — rendered as-is, `<img>` tags incl. `/uploads` URLs pass through), `prevSlug`, `nextSlug`. Auto-increments `viewCount`. 404 → "not found" screen.
Called from detail page, GenerateAITestPage (reads `title`, `slug`, `level`) and GrammarTestTopicsPage level-fallback (reads `level`).

### POST `/api/grammar`
Body: `{ title: string, content: string, level: string }` (from the "New Grammar" form — kaen extra; kaizen created lessons via admin CMS). Response: created grammar — UI reads `slug`/`id` to navigate.

### DELETE `/api/grammar/:idOrSlug`
From the detail-page delete button. Response unused (status only).
(`PATCH /api/grammar/:idOrSlug` exists in the backend but the UI has no edit form yet.)

### GET `/api/grammar-topics`
Query: `level?`.
Response: `Array<{ id: string, name: string, level: string, description?: string, questionCount?: number, grammarId?: string | null, grammarSlug?: string | null }>`.

### GET `/api/grammar-topics/for-lesson/:grammarSlug`
Response: `{ topicId: string, name: string, level: string, questionCount: number }` or `null` (no matching topic).

### GET `/api/grammar-test/:topicId`
Response: `Array<{ id: string, topicId?: string, content: string, options: Array<{ id: 'A'|'B'|'C'|'D', text: string }> }>` — max 10 questions, **no answers**.

### POST `/api/grammar-test/generate`
Body: `{ topic: string, level: string, count: number, grammarSlug?: string }`.
Response: freshly generated questions (same shape as above, no answers). **Slow — calls the LLM (30-120s)**; the UI shows an elapsed-seconds waiting banner and sends a 180s axios timeout. Errors: 400 `{ error, message }` (e.g. LLM bridge disabled) — UI shows `message` (fallback `error`).

### POST `/api/grammar-test/submit`
Body: `{ topicId: string, answers: Array<{ questionId: string, selectedAnswerId: string }> }`.
Response:
```ts
{
  sessionId: string, score: number, total: number,
  results: Array<{
    questionId: string, content: string, options: Array<{ id: string, text: string }>,
    selectedAnswerId: string, isCorrect: boolean,
    correctAnswerId: string, explanation?: string | null
  }>
}
```

### GET `/api/grammar-test/results/:sessionId`
Same shape as the submit response.

---

## Stories

Pages: `/stories` (list + AI-generate dialog), `/stories/create`, `/stories/:id/edit`, `/stories/:id` (reader: 3 step tabs, client-side vocab highlighting, progress tracking).

Step enum from the backend: `stepType: 'STEP1' | 'STEP2' | 'STEP3'` (the UI normalizes to lowercase for display and sends uppercase on create/update).

### GET `/api/stories`
Response: array of
```ts
{
  id: string, title: string, topic?: string, description?: string,
  lessonId: string, createdAt: string,
  lesson?: { id: string, title: string }
}
```
(`/api/stories/public` returns the same result; the trimmed UI only calls `/stories`.)

### GET `/api/stories/:id`
Same fields plus:
```ts
{
  steps: Array<{ id: string, stepType: 'STEP1'|'STEP2'|'STEP3', primaryLanguage?: string,
                 content: string /* HTML */, order: number, audioUrl: null }>,
  lesson: { id: string, title: string, cards: Card[] },   // full cards for vocab highlighting
  progress?: {
    currentStep: number, completedSteps: number[],
    viewedVocabIds: string[], listenedVocabIds: string[],
    totalReadingTime?: number, ttsSessionsCount?: number
  }
}
```
`audioUrl` is always `null` — read-aloud and per-word pronunciation use the browser `speechSynthesis` API (en-US).

### POST `/api/stories`
Body: `{ title, lessonId, topic?, description?, steps: [{ stepType: 'STEP1'|'STEP2'|'STEP3', content, order }] }`.
Response: created story (UI navigates to `/stories` and re-fetches).

### PATCH `/api/stories/:id`
Same body shape as POST (steps replaced). Response unused.

### DELETE `/api/stories/:id`
Response unused (status only).

### GET `/api/stories/:id/progress`
Response: same `progress` shape as embedded in GET `/stories/:id`.

### POST `/api/stories/:id/progress`
Body (all optional):
```ts
{
  currentStep?: number,
  completedSteps?: number[],
  viewedVocabIds?: string[],      // MERGED server-side (send the client's current set)
  listenedVocabIds?: string[],    // MERGED server-side
  additionalReadingTime?: number,
  incrementTtsCount?: boolean     // UI sends true each time read-aloud starts
}
```
Response unused.

### POST `/api/stories/generate`
Body: `{ lessonId: string, title?: string, description?: string }`.
Response: the full created story (UI reads `id` to navigate to `/stories/:id`).
**Slow — calls the LLM (30-120s)**; the UI shows an elapsed-seconds waiting dialog and sends a 180s axios timeout. Errors: 400 `{ error, message }` — UI shows `message` (fallback `error`).

---

## Dictation

Pages: `/dictation` (topics), `/dictation/:topic` (lesson list), `/dictation/practice/:id` (typing practice), `/dictation/listen/:id` (listen mode), `/dictation-history`.

Audio: the UI plays `lesson.audioUrl` with an HTML5 `<audio>` element and seeks per segment via `currentTime` (`startTime`..`endTime`, stop enforced on `timeupdate` + a 60ms poll). It does NOT call `/api/dictation-lessons/:id/audio/segment` (that endpoint only redirects to `audioUrl`). kaizen's wavesurfer.js waveform was replaced by a plain segment progress bar. YouTube video mode and pronunciation-challenge mode were not ported — every lesson renders the segment-based dictation player.

### GET `/api/dictation-lessons`
Query: `topic?` (slug or name), `level?`, `page`, `limit`.
Response:
```ts
{
  data: Array<{
    id: number, title: string, topic: string, description?: string, level: string,
    audioUrl: string, youtubeVideoId?: string,
    mode: 'dictation' | 'pronunciation',
    dictationTopic: { id: number, name: string, slug: string, level?: string } | null,
    userProgress: { percentage: number, hasMark: boolean } | null
  }>,
  total: number, page: number, limit: number
}
```

### GET `/api/dictation-lessons/topics`
Response: `Array<{ id: number, name: string, slug: string, description?: string, level?: string, lessonCount?: number }>`.

### GET `/api/dictation-lessons/history/me`
Response: array of **flattened lesson objects** (same fields as the list) plus:
```ts
{ ...lesson, completionPercentage: number, lastPracticedAt: string }
```
Ordered by `lastPracticedAt` DESC. The UI reads `id` (lesson id), `title`, `dictationTopic?.name`/`topic`, `completionPercentage`, `lastPracticedAt`.

### GET `/api/dictation-lessons/:id`
Lesson shape plus:
```ts
{
  segments: Array<{
    id: number, content: string, solutions: string[][],   // groups of accepted variants, in order
    startTime: number, endTime: number, orderIndex: number
  }>
}
```
(`pronunciationChallenges` may also be present; the trimmed UI ignores it.)

### GET `/api/dictation-lessons/:id/progress`
Response: `{ currentIndex: number, segmentStatus: Record<number, 'learned'|'skipped'|'marked'> } | null`.

### POST `/api/dictation-lessons/:id/progress`
Body: `{ currentIndex: number, segmentStatus: Record<number, 'learned'|'skipped'|'marked'> }`. Response unused. Saved debounced (1s) while practicing.

---

## Dictionary

Used by the word-lookup popup (PronunciationCard — click a word in dictation feedback/listen mode) and by the listen-mode translation dropdown (with an external Google Translate fallback when the backend has no translation).

### GET `/api/dictionary/lookup`
Query: `word` (string), `targetLang` (default `vi`).
Response:
```ts
{
  word: string, ipa: string, partOfSpeech: string,
  definition: string, examples: string[],
  audioUrl: string | null, audioUs: string | null, audioUk: string | null,
  translation: string                       // targetLang translation
}
```
UI reads `translation`, `ipa`, `partOfSpeech`, `definition`, `examples` (audio playback uses browser `speechSynthesis`, not the audio URLs).

### GET `/api/dictionary/audio`
Query: `word`.
Response: `{ word: string, audioUrl: string | null }`. (Available in `dictationApi.getAudioUrl`; current UI pronounces via `speechSynthesis` instead.)

---

## Spec'd in the task but NOT called by the trimmed UI

Backend may skip these (or keep for future):

- `PATCH /api/users/profile` — UpdateProfile page was removed.
- `POST /api/study/snooze` — UI only calls `POST /api/users/snooze`.
- `GET /api/lessons/my` — UI uses `/api/lessons` (Bank) and `/api/lessons/my-and-marked` (ManageLessons).
- `POST /api/lessons/import` — import is done client-side (parse text → `POST /lessons` + per-card `POST /lessons/:id/cards`).
- `/api/lessons/public`, mark/unmark, `/api/tags*`, `/api/languages`, `/api/countries`, `/api/dictionary/*`, `/api/lessons/pronunciation/:word` — all removed; language/country lists are static client-side and pronunciation uses the browser `speechSynthesis` API (en-US).
