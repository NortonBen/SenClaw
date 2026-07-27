import { useState, useEffect, useRef, useCallback } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Plus, BookOpen, Database, Loader2 } from 'lucide-react';
import api from '@/lib/api';
import FlipCard from '@/components/study/FlipCard';
import MatchingGame from '@/components/game/MatchingGame';
import MultipleChoiceQuestion from '@/components/game/MultipleChoiceQuestion';
import ImportDialog from '@/components/common/ImportDialog';
import Modal from '@/components/common/Modal';
import { Card } from '@/types';
import './Study.css';
import SEO from '@/components/common/SEO';

type Phase = 'new' | 'review' | 'quiz' | 'game';

type StudyCard = Card & {
  progress?: {
    level: number;
    isUrgent: boolean;
    nextReview?: string;
  };
};

export default function Study() {
  const { t } = useTranslation();
  const [cards, setCards] = useState<StudyCard[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [phase, setPhase] = useState<Phase>('new');
  const [startTime] = useState(Date.now());
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  // Số thẻ mới đã học + số lần ôn trong phiên (dùng cho log)
  const [reviewedCount, setReviewedCount] = useState(0);
  const [newWordsCount, setNewWordsCount] = useState(0);
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [isReviewMode, setIsReviewMode] = useState(false);
  const [reviewLessonTitle, setReviewLessonTitle] = useState<string | null>(null);
  const [showCompleteModal, setShowCompleteModal] = useState(false);
  const [showErrorModal, setShowErrorModal] = useState(false);
  const [showStopModal, setShowStopModal] = useState(false);
  const [completeMessage, setCompleteMessage] = useState('');
  const [errorMessage, setErrorMessage] = useState('');
  const [quizIndex, setQuizIndex] = useState(0);
  const [quizScore, setQuizScore] = useState(0);
  const [isLoading, setIsLoading] = useState(true);
  const [hasLoadedOnce, setHasLoadedOnce] = useState(false);
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const newWordsCountRef = useRef(newWordsCount);
  const reviewedCountRef = useRef(reviewedCount);
  const cardsRef = useRef(cards);
  // Lưu tất cả review results để submit batch khi hoàn thành
  const [reviewResults, setReviewResults] = useState<Array<{ cardId: string; result: 'REMEMBER' | 'FORGOT'; mode: 'FLIP' }>>([]);

  // Cập nhật refs khi state thay đổi
  useEffect(() => {
    newWordsCountRef.current = newWordsCount;
    reviewedCountRef.current = reviewedCount;
    cardsRef.current = cards;
  }, [newWordsCount, reviewedCount, cards]);

  // Reset reviewResults khi load session mới
  useEffect(() => {
    setReviewResults([]);
  }, [searchParams]);

  // Timer 6 phút (360 giây)
  const SESSION_DURATION = 360;
  const PHASE_1_END = 120; // Phút 0-2: Từ mới
  const PHASE_2_END = 240; // Phút 2-4: Ôn tập
  const PHASE_3_END = 300; // Phút 4-5: Trắc nghiệm

  useEffect(() => {
    let isMounted = true;
    const abortController = new AbortController();

    const lessonId = searchParams.get('lessonId');
    if (lessonId) {
      setIsReviewMode(true);
      loadReviewSession(lessonId, abortController.signal, isMounted);
    } else {
      setIsReviewMode(false);
      loadSession(abortController.signal, isMounted);
    }

    return () => {
      isMounted = false;
      abortController.abort();
    };
  }, [searchParams]);

  const handleSessionComplete = useCallback(async (gameScore?: number) => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
    }

    const durationSeconds = Math.floor((Date.now() - startTime) / 1000);

    try {
      // Submit tất cả review results cùng lúc qua batch API (bao gồm cả study log)
      await api.post('/study/review-batch', {
        reviews: reviewResults,
        durationSeconds,
        newWordsLearned: newWordsCountRef.current,
        cardsReviewed: reviewedCountRef.current,
        gameScore: gameScore || 0,
      });

      const message = gameScore !== undefined
        ? t('study.completeWithScore', { score: gameScore, total: Math.min(10, cardsRef.current.length) })
        : t('study.complete');

      setCompleteMessage(message);
      setShowCompleteModal(true);
    } catch (error) {
      console.error('Failed to save study log:', error);
      setErrorMessage(t('study.completeButLogFailed'));
      setShowErrorModal(true);
    }
  }, [startTime, reviewResults]);

  const handleCompleteConfirm = () => {
    setShowCompleteModal(false);
    navigate('/');
  };

  const handleErrorConfirm = () => {
    setShowErrorModal(false);
    navigate('/');
  };

  const newCards = isReviewMode
    ? []
    : cards.filter((c) => !c.progress || c.progress.level === 0);
  const reviewCards = isReviewMode
    ? cards
    : cards.filter((c) => c.progress && c.progress.level > 0);
  const quizCards = cards.slice(0, Math.min(5, cards.length));

  // Timer effect
  useEffect(() => {
    intervalRef.current = setInterval(() => {
      const elapsed = Math.floor((Date.now() - startTime) / 1000);
      setElapsedSeconds(elapsed);

      // Phase transitions dựa trên thời gian
      if (elapsed >= SESSION_DURATION) {
        handleSessionComplete();
      } else if (elapsed >= PHASE_3_END && phase !== 'game') {
        setPhase('game');
      } else if (elapsed >= PHASE_2_END && phase !== 'quiz' && phase !== 'game') {
        const currentQuizCards = cards.slice(0, Math.min(5, cards.length));
        if (currentQuizCards.length === 0) {
          setPhase('game');
        } else {
          setQuizIndex(0);
          setQuizScore(0);
          setPhase('quiz');
        }
      } else if (elapsed >= PHASE_1_END && phase === 'new') {
        setPhase('review');
      }
    }, 1000);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
      }
    };
  }, [startTime, phase, cards.length, handleSessionComplete]);

  const loadSession = async (signal?: AbortSignal, isMounted?: boolean) => {
    try {
      // Đảm bảo luôn hiển thị loading khi bắt đầu load
      setIsLoading(true);
      const { data } = await api.get('/study/session', { signal });

      if (!isMounted) {
        setIsLoading(false);
        return;
      }

      const sessionCards = data.cards || [];

      // Set tất cả state trước, sau đó mới set isLoading = false ở cuối cùng
      // React sẽ batch các state updates này lại, nhưng isLoading sẽ được set false sau cùng
      setCards(sessionCards);
      setQuizIndex(0);
      setQuizScore(0);
      setNewWordsCount(0);
      setReviewedCount(0);
      setReviewResults([]);
      setHasLoadedOnce(true);

      // Set isLoading = false sau cùng để đảm bảo loading hiển thị đến khi API hoàn thành
      setIsLoading(false);
    } catch (error: any) {
      if (!isMounted) {
        setIsLoading(false);
        return;
      }
      setHasLoadedOnce(true);
      setIsLoading(false);
      if (error.name === 'CanceledError' || error.name === 'AbortError') {
        return;
      }
      if (isMounted) {
        console.error('Failed to load session:', error);
        setErrorMessage(t('study.loadSessionFailed'));
        setShowErrorModal(true);
      }
    }
  };

  const loadReviewSession = async (lessonId: string, signal?: AbortSignal, isMounted?: boolean) => {
    try {
      // Đảm bảo luôn hiển thị loading khi bắt đầu load
      setIsLoading(true);
      const { data } = await api.get(`/study/session?lessonId=${lessonId}`, { signal });

      if (!isMounted) {
        setIsLoading(false);
        return;
      }

      const sessionCards = data.cards || [];

      // Set tất cả state trước, sau đó mới set isLoading = false ở cuối cùng
      setCards(sessionCards);
      setReviewLessonTitle(data.lesson?.title || null);
      setQuizIndex(0);
      setQuizScore(0);
      setNewWordsCount(0);
      setReviewedCount(0);
      setHasLoadedOnce(true);

      // Tự động chuyển sang phase review
      if (sessionCards.length > 0) {
        setPhase('review');
        setCurrentIndex(0);
      }

      // Set isLoading = false sau cùng để đảm bảo loading hiển thị đến khi API hoàn thành
      setIsLoading(false);
    } catch (error: any) {
      if (!isMounted) {
        setIsLoading(false);
        return;
      }
      setHasLoadedOnce(true);
      setIsLoading(false);
      if (error.name === 'CanceledError' || error.name === 'AbortError') {
        return;
      }
      if (isMounted) {
        console.error('Failed to load review session:', error);
        setErrorMessage(t('study.loadReviewFailed'));
        setShowErrorModal(true);
      }
    }
  };

  // Lấy thẻ phù hợp với phase hiện tại
  const getCurrentPhaseCards = () => {
    if (phase === 'new') {
      return newCards.slice(0, 5); // 3-5 từ mới
    } else if (phase === 'review') {
      return reviewCards; // Từ đến hạn
    } else if (phase === 'quiz') {
      return quizCards; // Kiểm tra nhanh
    }
    return cards.slice(0, Math.min(10, cards.length)); // Game
  };

  const phaseCards = getCurrentPhaseCards();
  const phaseCurrentIndex = phase === 'new'
    ? Math.min(currentIndex, phaseCards.length - 1)
    : phase === 'review'
      ? Math.max(0, currentIndex - newCards.length)
      : phase === 'quiz'
        ? Math.min(quizIndex, Math.max(phaseCards.length - 1, 0))
        : 0;

  const handleReview = async (cardId: string, result: 'REMEMBER' | 'FORGOT', mode: 'FLIP') => {
    // Lưu review result vào state thay vì gọi API ngay
    setReviewResults((prev) => {
      // Kiểm tra xem cardId đã có trong results chưa, nếu có thì update, nếu chưa thì thêm mới
      const existingIndex = prev.findIndex((r) => r.cardId === cardId);
      if (existingIndex >= 0) {
        // Update existing review
        const updated = [...prev];
        updated[existingIndex] = { cardId, result, mode };
        return updated;
      } else {
        // Add new review
        return [...prev, { cardId, result, mode }];
      }
    });

    setReviewedCount((prev) => prev + 1);

    const currentCard = cards.find((c) => c.id === cardId);
    // Nếu là từ mới (chưa có progress hoặc level 0) và trả lời đúng -> tính là "từ mới đã học" cho log
    if (result === 'REMEMBER' && currentCard && (!currentCard.progress || currentCard.progress.level === 0)) {
      setNewWordsCount((prev) => prev + 1);
    }

    // Move to next card trong phase hiện tại
    if (phaseCurrentIndex < phaseCards.length - 1) {
      if (phase === 'new') {
        setCurrentIndex(currentIndex + 1);
      } else if (phase === 'review') {
        setCurrentIndex(currentIndex + 1);
      }
    } else {
      // Hết thẻ trong phase, chuyển phase nếu chưa đến thời gian
      if (elapsedSeconds < PHASE_1_END && phase === 'new') {
        // Chuyển sang review sớm nếu đã học hết từ mới
        setPhase('review');
        setCurrentIndex(newCards.length);
      } else if (elapsedSeconds < PHASE_2_END && phase === 'review') {
        if (quizCards.length > 0) {
          setPhase('quiz');
          setQuizIndex(0);
          setQuizScore(0);
        } else {
          setPhase('game');
        }
      }
    }
  };

  const handleGameComplete = async (score: number) => {
    await handleSessionComplete(score);
  };

  const handleQuizResult = (result: 'REMEMBER' | 'FORGOT') => {
    if (result === 'REMEMBER') {
      setQuizScore((prev) => prev + 1);
    }

    if (quizIndex < quizCards.length - 1) {
      setQuizIndex((prev) => prev + 1);
    } else {
      setPhase('game');
    }
  };

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  const getPhaseName = () => {
    if (isReviewMode) return t('study.reviewLesson');
    if (phase === 'new') return t('study.loadNewWords');
    if (phase === 'review') return t('study.reviewSRS');
    if (phase === 'quiz') return t('study.quiz');
    return t('study.reflex');
  };

  const getRemainingTime = () => SESSION_DURATION - elapsedSeconds;

  const handleImportLesson = async (title: string, parsedCards: Card[]) => {
    try {
      // Tạo lesson rỗng trước
      const { data: lesson } = await api.post('/lessons', { title });

      // Thêm từng card vào lesson
      for (const card of parsedCards) {
        await api.post(`/lessons/${lesson.id}/cards`, {
          word: card.word,

          examples: card.examples,
        });
      }

      // Reload session để có cards mới
      await loadSession(undefined, true);

      // Đóng dialog
      setShowImportDialog(false);
    } catch (error) {
      console.error('Failed to import lesson:', error);
      throw error;
    }
  };

  if (isLoading || !hasLoadedOnce) {
    return (
      <div className="study-container">
        <div className="study-loading k-card">
          <Loader2 className="spin" size={40} />
          <p>{t('study.loading')}</p>
        </div>
      </div>
    );
  }

  if (cards.length === 0) {
    return (
      <div className="study-container">
        <div className="study-empty k-card">
          <div className="study-empty-icon">
            <BookOpen size={30} strokeWidth={1.6} />
          </div>
          <h2 className="study-empty-title">{t('study.noCards')}</h2>
          <p className="study-empty-description">
            {t('study.emptyDescription')}
          </p>
          <div className="study-empty-actions">
            <button onClick={() => navigate('/lessons/create')} className="k-btn k-btn--primary">
              <Plus size={17} />
              {t('study.createNewLesson')}
            </button>
            <button onClick={() => navigate('/bank')} className="k-btn k-btn--ghost">
              <Database size={17} />
              {t('study.goToBank')}
            </button>
          </div>
        </div>
      </div>
    );
  }

  const currentCard = phaseCards[phaseCurrentIndex];
  const totalPhaseCards = phaseCards.length;
  const displayedCardNumber = totalPhaseCards > 0 ? phaseCurrentIndex + 1 : 0;

  // Nếu không có thẻ trong phase hiện tại, chuyển phase
  if (!currentCard && phase !== 'game') {
    if (phase === 'new' && reviewCards.length > 0) {
      setPhase('review');
      setCurrentIndex(newCards.length);
    } else if (phase === 'review') {
      if (quizCards.length > 0) {
        setPhase('quiz');
        setQuizIndex(0);
        setQuizScore(0);
      } else {
        setPhase('game');
      }
    } else if (phase === 'quiz') {
      setPhase('game');
    }
    return null;
  }

  return (
    <div className="study-container">
      <SEO title={t('seo.timedStudy')} description={t('seo.timedStudyDesc')} />
      <div className="study-header">
        <div className="k-page-head study-head">
          <div>
            <h1>{t('study.studyVocabulary')}</h1>
            {isReviewMode && reviewLessonTitle && (
              <span className="k-chip study-lesson-chip">
                {t('study.reviewingLesson', { title: reviewLessonTitle })}
              </span>
            )}
          </div>
          <button
            onClick={() => setShowStopModal(true)}
            className="k-btn k-btn--ghost study-stop"
          >
            {t('study.endEarly')}
          </button>
        </div>
        <div className="study-timer k-card">
          <div className="timer-display">
            <span className="timer-label">{t('study.timeRemaining')}</span>
            <span className="timer-value k-num">{formatTime(getRemainingTime())}</span>
          </div>
          <div className="phase-indicator">
            <span className={`phase-badge phase-${phase}`}>{getPhaseName()}</span>
          </div>
        </div>
        <div className="study-progress">
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{ width: `${(elapsedSeconds / SESSION_DURATION) * 100}%` }}
            />
          </div>
          <div className="progress-labels">
            <span className={elapsedSeconds < PHASE_1_END ? 'active' : elapsedSeconds >= PHASE_1_END ? 'completed' : ''}>{t('study.phaseNew')}</span>
            <span className={elapsedSeconds >= PHASE_1_END && elapsedSeconds < PHASE_2_END ? 'active' : elapsedSeconds >= PHASE_2_END ? 'completed' : ''}>{t('study.phaseReview')}</span>
            <span className={elapsedSeconds >= PHASE_2_END && elapsedSeconds < PHASE_3_END ? 'active' : elapsedSeconds >= PHASE_3_END ? 'completed' : ''}>{t('study.phaseQuiz')}</span>
            <span className={elapsedSeconds >= PHASE_3_END ? 'active' : ''}>{t('study.phaseGame')}</span>
          </div>
        </div>
        <div className="study-stats">
          <span className="k-chip">{t('study.statsNew')}: <b className="k-num">{newWordsCount}</b></span>
          <span className="k-chip">{t('study.statsReview')}: <b className="k-num">{reviewedCount}</b></span>
          <span className="k-chip">{t('study.statsCards')}: <b className="k-num">{displayedCardNumber}/{totalPhaseCards}</b></span>
          {phase === 'quiz' && (
            <span className="k-chip">{t('study.statsQuiz')}: <b className="k-num">{quizScore}/{quizCards.length}</b></span>
          )}
        </div>
      </div>

      <div className="study-content">
        {phase === 'game' ? (
          <MatchingGame
            cards={cards.slice(0, Math.min(10, cards.length))}
            onComplete={handleGameComplete}
          />
        ) : phase === 'quiz' && currentCard ? (
          <MultipleChoiceQuestion
            key={`${currentCard.id}-quiz`}
            card={currentCard}
            allCards={cards}
            onResult={handleQuizResult}
          />
        ) : currentCard ? (
          <FlipCard
            key={currentCard.id}
            card={currentCard}
            onResult={(result) => handleReview(currentCard.id, result, 'FLIP')}
          />
        ) : null}
      </div>

      <Modal
        isOpen={showCompleteModal}
        onClose={handleCompleteConfirm}
        title={t('study.complete')}
        message={completeMessage}
        type="success"
        confirmText={t('common.close')}
        onConfirm={handleCompleteConfirm}
      />

      <Modal
        isOpen={showErrorModal}
        onClose={handleErrorConfirm}
        title={t('common.notification')}
        message={errorMessage}
        type="error"
        confirmText={t('common.close')}
        onConfirm={handleErrorConfirm}
      />

      <Modal
        isOpen={showStopModal}
        onClose={() => setShowStopModal(false)}
        title={t('study.endEarly')}
        message={t('study.endEarlyConfirm')}
        type="confirm"
        confirmText={t('study.end')}
        cancelText={t('common.continue')}
        onConfirm={handleSessionComplete}
        onCancel={() => setShowStopModal(false)}
      />

      <ImportDialog
        isOpen={showImportDialog}
        onClose={() => setShowImportDialog(false)}
        onImport={handleImportLesson}
      />
    </div>
  );
}

