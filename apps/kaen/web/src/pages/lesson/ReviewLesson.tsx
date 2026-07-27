
import { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import api from '@/lib/api';
import Modal from '@/components/common/Modal';
import SEO from '@/components/common/SEO';
import '../practice/Review.css';
import ReviewCard from '@/components/study/ReviewCard';
import { useAuthStore } from '@/store/authStore';
import { Card } from '@/types';
import styles from './ReviewLesson.module.css';
import { playPronunciation } from '@/lib/audioUtils';

interface Question {
  card: Card;
  questionType: 'word' | 'explain';
  options: string[];
  correctAnswer: string;
}

export default function ReviewLesson() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id: lessonId } = useParams<{ id: string }>();
  const [cards, setCards] = useState<Card[]>([]);
  const [currentQuestionIndex, setCurrentQuestionIndex] = useState(0);
  const [currentQuestion, setCurrentQuestion] = useState<Question | null>(null);
  const [selectedAnswer, setSelectedAnswer] = useState<string | null>(null);
  const [showResult, setShowResult] = useState(false);
  const [isCorrect, setIsCorrect] = useState(false);
  const [score, setScore] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCompleteModal, setShowCompleteModal] = useState(false);
  const [showStopModal, setShowStopModal] = useState(false);
  const [completeMessage, setCompleteMessage] = useState('');
  const [lessonTitle, setLessonTitle] = useState<string | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [reviewResults, setReviewResults] = useState<Map<string, boolean>>(new Map());

  useEffect(() => {
    if (lessonId) {
      loadReviewSession();
    }
  }, [lessonId]);

  const loadReviewSession = async () => {
    if (!lessonId) return;

    try {
      setLoading(true);
      const { token } = useAuthStore.getState();

      console.log(`[ReviewLesson] Loading review session for lesson ${lessonId}(Guest: ${!token})`);

      let sessionCards = [];
      if (token) {
        // Logged in: get actual review session (limited to learned cards)
        const { data } = await api.get(`/review/session/lesson/${lessonId}`, {
          params: { allowRepeat: 'true' },
        });
        sessionCards = data.cards || [];
        if (data.lesson) setLessonTitle(data.lesson.title);
      } else {
        // Guest: get all cards from public lesson endpoint
        const { data } = await api.get(`/lessons/${lessonId}/cards`);
        sessionCards = data.cards || [];
        if (data.lesson) setLessonTitle(data.lesson.title);
      }

      console.log(`[ReviewLesson] Received ${sessionCards.length} cards`);

      if (sessionCards.length === 0) {
        setError(t('review.noCardsToReview'));
        setLoading(false);
        return;
      }

      setCards(sessionCards);
      setCurrentQuestionIndex(0);
      setScore(0);
      setSelectedAnswer(null);
      setShowResult(false);
      setReviewResults(new Map()); // Reset kết quả review
      generateQuestion(sessionCards, 0);
      setLoading(false);
    } catch (err: any) {
      console.error('Failed to load review session:', err);
      const errorMsg = err.response?.data?.message || err.message || t('review.loadFailed');
      setError(errorMsg);
      setLoading(false);
    }
  };

  const generateQuestion = (cardList: Card[], index: number) => {
    if (index >= cardList.length) {
      // Đã hết câu hỏi
      return;
    }

    const card = cardList[index];

    // Random chọn kiểu câu hỏi: 'word' hoặc 'explain'
    const isExplainType = Math.random() > 0.5;
    const questionType: 'word' | 'explain' = isExplainType ? 'explain' : 'word';

    let correctAnswer: string = '';
    const wrongOptions: string[] = [];

    const otherCards = cardList.filter((c) => c.id !== card.id);
    const shuffledOthers = [...otherCards].sort(() => Math.random() - 0.5);

    if (questionType === 'word') {
      // Hiện từ (Word) -> Chọn giải thích (Explain)
      correctAnswer = (card.explain || card.meanings?.['vi'] || '').trim();

      if (!correctAnswer) {
        console.warn(`[ReviewLesson] Card ${card.word} has no explain/meaning, using '...'`);
        correctAnswer = '...';
      }

      for (let i = 0; i < Math.min(3, shuffledOthers.length); i++) {
        const wrongCard = shuffledOthers[i];
        const wrongAnswer = (wrongCard.explain || wrongCard.meanings?.['vi'] || '').trim();
        if (wrongAnswer && wrongAnswer !== correctAnswer && !wrongOptions.includes(wrongAnswer)) {
          wrongOptions.push(wrongAnswer);
        }
      }
    } else {
      // Hiện giải thích (Explain) -> Chọn từ (Word)
      correctAnswer = card.word;

      for (let i = 0; i < Math.min(3, shuffledOthers.length); i++) {
        const wrongCard = shuffledOthers[i];
        const wrongAnswer = wrongCard.word;
        if (wrongAnswer && wrongAnswer !== correctAnswer && !wrongOptions.includes(wrongAnswer)) {
          wrongOptions.push(wrongAnswer);
        }
      }
    }

    // Nếu chưa đủ 3 đáp án sai, thêm các giá trị mặc định
    while (wrongOptions.length < 3) {
      if (questionType === 'word') {
        const fake = t('review.different') + ' ' + (wrongOptions.length + 1);
        if (!wrongOptions.includes(fake)) wrongOptions.push(fake);
      } else {
        break;
      }
    }

    // Trộn đáp án
    const allOptions = [correctAnswer, ...wrongOptions];
    const shuffledOptions = allOptions.sort(() => Math.random() - 0.5);

    setCurrentQuestion({
      card,
      questionType,
      options: shuffledOptions,
      correctAnswer,
    });
    setSelectedAnswer(null);
    setShowResult(false);
  };

  const handleSelectAnswer = (answer: string) => {
    if (showResult) return; // Đã chọn rồi, không cho chọn lại

    setSelectedAnswer(answer);
    const correct = answer === currentQuestion?.correctAnswer;
    setIsCorrect(correct);
    setShowResult(true);

    if (correct) {
      setScore((prev) => prev + 1);
    }

    // Lưu kết quả vào state thay vì submit ngay (tối ưu hóa)
    if (currentQuestion) {
      setReviewResults((prev) => {
        const newMap = new Map(prev);
        newMap.set(currentQuestion.card.id, correct);
        return newMap;
      });
      console.log(`[ReviewLesson] Stored review result for card ${currentQuestion.card.id}, isCorrect: ${correct}`);
    }

    // Sau 1.5 giây chuyển câu tiếp theo
    setTimeout(() => {
      const nextIndex = currentQuestionIndex + 1;
      if (nextIndex < cards.length) {
        setCurrentQuestionIndex(nextIndex);
        generateQuestion(cards, nextIndex);
      } else {
        // Đã hết câu hỏi
        handleComplete();
      }
    }, 1500);
  };

  const submitBatchReview = async (results: Array<{ cardId: string; isCorrect: boolean }>) => {
    const { token } = useAuthStore.getState();
    if (!token) {
      console.log('[ReviewLesson] Guest mode: skipping batch submit');
      return { success: true, submitted: 0, total: 0 };
    }

    if (!results || results.length === 0) {
      console.log('[ReviewLesson] No results to submit');
      return { success: true, submitted: 0, total: 0 };
    }

    try {
      console.log(`[ReviewLesson] Submitting batch review for ${results.length} cards...`);
      const response = await api.post('/review/submit/batch', { results });
      console.log(`[ReviewLesson] Batch review submitted successfully:`, response.data);
      return response.data;
    } catch (error: any) {
      console.error(`[ReviewLesson] Failed to submit batch review:`, error);
      // Ném lỗi để caller có thể xử lý
      throw error;
    }
  };

  const handlePlayPronunciation = async () => {
    if (!currentQuestion || !currentQuestion.card.word) return;
    await playPronunciation(currentQuestion.card.word, () => setIsPlaying(true), () => setIsPlaying(false));
  };

  const handleComplete = async () => {
    const results = Array.from(reviewResults.entries()).map(([cardId, isCorrect]) => ({
      cardId,
      isCorrect,
    }));

    if (results.length > 0) {
      try {
        await submitBatchReview(results);
        console.log(`[ReviewLesson] Submitted batch review for ${results.length} cards`);
        setReviewResults(new Map());
      } catch (error) {
        console.error('[ReviewLesson] Failed to submit batch review:', error);
      }
    }

    const percentage = Math.round((score / cards.length) * 100);
    const message = t('review.completeMessage', { score, total: cards.length, percent: percentage });

    setCompleteMessage(message);
    setShowCompleteModal(true);
  };

  const handleContinueReview = () => {
    setShowCompleteModal(false);
    loadReviewSession();
  };

  const handleFinishReview = () => {
    setShowCompleteModal(false);
    navigate('/lessons');
  };

  if (loading) {
    return (
      <div className={styles.container}>
        <div className={styles.loading}>
          <p>{t('review.loading')}</p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className={styles.container}>
        <div className={styles.error}>
          <p>{error}</p>
          <div className={styles.errorActions}>
            <button onClick={() => navigate('/lessons')} className={styles.btnPrimary}>
              {t('review.backToLessons')}
            </button>
            <button onClick={() => loadReviewSession()} className={styles.btnSecondary}>
              {t('common.retry')}
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (!currentQuestion) {
    return (
      <div className={styles.container}>
        <div className={styles.loading}>
          <p>{t('review.noQuestions')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.container}>
      <SEO title={lessonTitle ? `Review: ${lessonTitle}` : 'Review Lesson'} />
      <div className={styles.header}>
        <div className={styles.headerTop}>
          <div>
            <h1>{t('review.title')}</h1>
            {lessonTitle && (
              <div className={styles.subHeader}>
                {t('review.lesson')}: <strong>{lessonTitle}</strong>
              </div>
            )}
          </div>
          <button
            onClick={() => setShowStopModal(true)}
            className={styles.btnStop}
          >
            {t('review.stopReview')}
          </button>
        </div>
        <div className={styles.progress}>
          <div className={styles.progressBar}>
            <div
              className={styles.progressFill}
              style={{
                width: `${((currentQuestionIndex + 1) / cards.length) * 100}%`,
              }}
            />
          </div>
          <div className={styles.progressText}>
            {t('review.questionProgress', { current: currentQuestionIndex + 1, total: cards.length })} | {t('review.score')}: {score}
          </div>
        </div>
      </div>

      <div className={styles.content}>
        <ReviewCard
          question={currentQuestion}
          showResult={showResult}
          selectedAnswer={selectedAnswer}
          isCorrect={isCorrect}
          isPlaying={isPlaying}
          onSelectAnswer={handleSelectAnswer}
          onPlayPronunciation={handlePlayPronunciation}
        />
      </div>



      <Modal
        isOpen={showCompleteModal}
        onClose={() => setShowCompleteModal(false)}
        title={t('review.completeTitle')}
        message={completeMessage + '\n\n' + t('review.reviewAgainConfirm')}
        type="confirm"
        confirmText={t('review.reviewAgain')}
        cancelText={t('review.backToLessons')}
        onConfirm={handleContinueReview}
        onCancel={handleFinishReview}
      />

      <Modal
        isOpen={showStopModal}
        onClose={() => setShowStopModal(false)}
        title={t('review.stopTitle')}
        message={t('review.stopConfirm')}
        type="confirm"
        confirmText={t('common.stop')}
        cancelText={t('common.continue')}
        onConfirm={() => navigate('/lessons')}
        onCancel={() => setShowStopModal(false)}
      />
    </div>
  );
}
