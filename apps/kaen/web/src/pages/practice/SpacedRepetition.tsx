import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { BookOpen, Loader2 } from 'lucide-react';
import api from '@/lib/api';
import FlipCard from '@/components/study/FlipCard';
import MatchingGame from '@/components/game/MatchingGame';
import MultipleChoiceQuestion from '@/components/game/MultipleChoiceQuestion';
import ListeningPracticeCard from '@/components/study/ListeningPracticeCard';
import WritingPracticeCard from '@/components/study/WritingPracticeCard';
import Modal from '@/components/common/Modal';
import './Study.css';
import SEO from '@/components/common/SEO';

import { Card as BaseCard } from '@/types';

interface Card extends BaseCard {
  progress?: {
    level: number;
    isUrgent: boolean;
    nextReview?: string;
  };
}

type Phase = 'flashcard' | 'multiple-choice' | 'matching' | 'listening' | 'writing';

export default function SpacedRepetition() {
  const { t } = useTranslation();
  const { reviewNotificationId } = useParams<{ reviewNotificationId: string }>();
  const [cards, setCards] = useState<Card[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [phase, setPhase] = useState<Phase>('flashcard');
  const [startTime] = useState(Date.now());
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [reviewedCount, setReviewedCount] = useState(0);
  const [newWordsCount, setNewWordsCount] = useState(0);
  const [completedCardsInPhase, setCompletedCardsInPhase] = useState<Set<string>>(new Set());
  // Lưu tất cả kết quả reviews để submit một lần khi hoàn thành
  const [pendingReviews, setPendingReviews] = useState<Map<string, { result: 'REMEMBER' | 'FORGOT'; mode: 'FLIP' | 'TYPING' }>>(new Map());
  const [showCompleteModal, setShowCompleteModal] = useState(false);
  const [showErrorModal, setShowErrorModal] = useState(false);
  const [showStopModal, setShowStopModal] = useState(false);
  const [showContinueModal, setShowContinueModal] = useState(false);
  const [completeMessage, setCompleteMessage] = useState('');
  const [errorMessage, setErrorMessage] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const navigate = useNavigate();
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const loadSpacedRepetitionSession = useCallback(async (reviewNotificationId: string) => {
    try {
      setIsLoading(true);
      const { data } = await api.get(`/study/spaced-repetition/${reviewNotificationId}`);
      const sessionCards = data.cards || [];
      setCards(sessionCards);

      // Đếm số từ mới
      const newCount = sessionCards.filter(
        (c: Card) => !c.progress
      ).length;

      setNewWordsCount(newCount);
      setReviewedCount(0);
      setPendingReviews(new Map()); // Reset pending reviews khi load session mới

      // Tự động chuyển sang phase flashcard
      if (sessionCards.length > 0) {
        setPhase('flashcard');
        setCurrentIndex(0);
        setCompletedCardsInPhase(new Set());
      }
      setIsLoading(false);
    } catch (error) {
      setIsLoading(false);
      console.error('Failed to load spaced repetition session:', error);
      setErrorMessage(t('spacedRepetition.loadFailed'));
      setShowErrorModal(true);
    }
  }, [t]);

  // Hàm generate listening question - phải được định nghĩa trước khi sử dụng
  const generateListeningQuestion = useCallback((card: Card, allCards: Card[]) => {
    const correctAnswer = card.meanings?.['vi'] || '';
    const wrongOptions: string[] = [];
    const otherCards = allCards.filter((c) => c.id !== card.id);
    const shuffledOthers = [...otherCards].sort(() => Math.random() - 0.5);

    for (let i = 0; i < Math.min(3, shuffledOthers.length); i++) {
      const wrongCard = shuffledOthers[i];
      const wrongMeaning = wrongCard.meanings?.['vi'] || '';
      if (wrongMeaning && wrongMeaning !== correctAnswer && !wrongOptions.includes(wrongMeaning)) {
        wrongOptions.push(wrongMeaning);
      }
    }

    // Nếu không đủ 3 đáp án sai, thêm các đáp án mặc định
    while (wrongOptions.length < 3) {
      const defaultOptions = [t('review.unknown'), t('spacedRepetition.notLearned'), t('spacedRepetition.needLookup')];
      for (const option of defaultOptions) {
        if (!wrongOptions.includes(option)) {
          wrongOptions.push(option);
          break;
        }
      }
      if (wrongOptions.length >= 3) break;
    }

    const allOptions = [correctAnswer, ...wrongOptions.slice(0, 3)];
    const shuffled = [...allOptions].sort(() => Math.random() - 0.5);

    return {
      card,
      word: card.word,
      options: shuffled,
      correctAnswer: correctAnswer,
    };
  }, [t]);

  // Tính toán reviewCards ở top level để có thể sử dụng trong các hàm khác
  // Tính toán reviewCards ở top level để có thể sử dụng trong các hàm khác
  const reviewCards = useMemo(() => cards.filter((c) => c.progress && new Date(c.progress.nextReview || Date.now()) <= new Date()), [cards]);

  useEffect(() => {
    if (reviewNotificationId) {
      loadSpacedRepetitionSession(reviewNotificationId);
    }
  }, [reviewNotificationId, loadSpacedRepetitionSession]);

  // Hàm submit tất cả reviews một lần
  const submitAllReviews = async () => {
    if (pendingReviews.size === 0) return;

    try {
      // Gọi tất cả API reviews cùng lúc
      const reviewPromises = Array.from(pendingReviews.entries()).map(([cardId, { result, mode }]) =>
        api.post(`/study/spaced-repetition/review/${cardId}`, { result, mode })
      );

      await Promise.all(reviewPromises);

      // Xóa tất cả pending reviews sau khi submit thành công
      setPendingReviews(new Map());
    } catch (error) {
      console.error('Failed to submit reviews:', error);
      // Không hiển thị lỗi cho user, chỉ log để debug
    }
  };

  const handleSessionComplete = async (gameScore?: number) => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
    }

    // Submit tất cả reviews trước khi lưu log
    await submitAllReviews();

    const durationSeconds = Math.floor((Date.now() - startTime) / 1000);

    // Tính số lượng cards cho matching game
    const matchingCardsCount = reviewCards.length > 0 ? reviewCards.length : cards.length;

    try {
      await api.post('/study/log', {
        durationSeconds,
        newWordsLearned: newWordsCount,
        cardsReviewed: reviewedCount,
        gameScore: gameScore || 0,
      });

      const message = gameScore !== undefined
        ? t('spacedRepetition.completeWithScore', { score: gameScore, total: matchingCardsCount })
        : t('spacedRepetition.complete');

      setCompleteMessage(message);
      setShowContinueModal(true);
    } catch (error) {
      console.error('Failed to save study log:', error);
      setErrorMessage(t('spacedRepetition.completeButLogFailed'));
      setShowErrorModal(true);
    }
  };

  const handleCompleteConfirm = () => {
    setShowCompleteModal(false);
    navigate('/');
  };

  const handleContinueToPractice = (practiceType: 'listening' | 'writing') => {
    setShowContinueModal(false);
    // Chuyển sang phase listening trong cùng SpacedRepetition
    // Sau khi hoàn thành listening sẽ tự động chuyển sang writing
    setPhase(practiceType);
    setCurrentIndex(0);
    setCompletedCardsInPhase(new Set());
  };

  const handleContinueCancel = () => {
    setShowContinueModal(false);
    navigate('/');
  };

  const handleErrorConfirm = () => {
    setShowErrorModal(false);
    navigate('/');
  };

  // Timer effect - chỉ đếm thời gian đã học, không tự động chuyển phase hay submit
  useEffect(() => {
    intervalRef.current = setInterval(() => {
      const elapsed = Math.floor((Date.now() - startTime) / 1000);
      setElapsedSeconds(elapsed);
    }, 1000);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
      }
    };
  }, [startTime]);



  // Lấy thẻ phù hợp với phase hiện tại
  const getCurrentPhaseCards = () => {
    if (phase === 'flashcard' || phase === 'multiple-choice' || phase === 'listening' || phase === 'writing') {
      return reviewCards.length > 0 ? reviewCards : cards; // Ưu tiên từ đến hạn, nếu không có thì dùng tất cả
    }
    // Matching game: dùng tất cả từ vựng học lại, nếu không có thì dùng tất cả cards
    return reviewCards.length > 0 ? reviewCards : cards;
  };

  const phaseCards = getCurrentPhaseCards();
  const phaseCurrentIndex = (phase === 'flashcard' || phase === 'multiple-choice' || phase === 'listening' || phase === 'writing')
    ? Math.max(0, currentIndex)
    : 0;

  // Tính toán currentCard trước các return statements
  const currentCard = phaseCards.length > 0 && phaseCurrentIndex < phaseCards.length
    ? phaseCards[phaseCurrentIndex]
    : undefined;

  // Cache listening question để tránh shuffle lại mỗi lần render
  // Phải được gọi ở top level, trước các return statements
  const listeningQuestion = useMemo(() => {
    if (phase === 'listening' && currentCard) {
      return generateListeningQuestion(currentCard, cards);
    }
    return null;
  }, [phase, currentCard, cards, generateListeningQuestion]);

  const handleReview = (cardId: string, result: 'REMEMBER' | 'FORGOT', mode: 'FLIP' | 'TYPING') => {
    // Lưu kết quả vào state thay vì gọi API ngay
    setPendingReviews((prev) => {
      const newMap = new Map(prev);
      newMap.set(cardId, { result, mode });
      return newMap;
    });

    setReviewedCount((prev) => prev + 1);

    const currentCard = cards.find((c) => c.id === cardId);
    if (result === 'REMEMBER' && currentCard && !currentCard.progress) {
      setNewWordsCount((prev) => prev + 1);
    }

    // Đánh dấu card đã hoàn thành trong phase hiện tại
    setCompletedCardsInPhase((prev) => new Set(prev).add(cardId));

    // Kiểm tra xem đã hoàn thành tất cả cards trong phase chưa
    const allCompleted = phaseCards.every(card => completedCardsInPhase.has(card.id) || card.id === cardId);

    if (allCompleted) {
      // Đã hoàn thành tất cả cards, chuyển phase
      if (phase === 'flashcard') {
        setPhase('multiple-choice');
        setCurrentIndex(0);
        setCompletedCardsInPhase(new Set());
      } else if (phase === 'multiple-choice') {
        setPhase('matching');
        setCompletedCardsInPhase(new Set());
      }
    } else {
      // Chưa hoàn thành, chuyển sang card tiếp theo
      if (phaseCurrentIndex < phaseCards.length - 1) {
        setCurrentIndex(currentIndex + 1);
      }
    }
  };

  const handleGameComplete = async (score: number) => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
    }

    // Submit tất cả reviews trước khi lưu log
    await submitAllReviews();

    const durationSeconds = Math.floor((Date.now() - startTime) / 1000);

    // Tính số lượng cards cho matching game
    const matchingCardsCount = reviewCards.length > 0 ? reviewCards.length : cards.length;

    try {
      await api.post('/study/log', {
        durationSeconds,
        newWordsLearned: newWordsCount,
        cardsReviewed: reviewedCount,
        gameScore: score || 0,
      });

      const message = t('spacedRepetition.completeWithScore', { score, total: matchingCardsCount });
      setCompleteMessage(message);
      setShowContinueModal(true);
    } catch (error) {
      console.error('Failed to save study log:', error);
      const message = t('spacedRepetition.completeWithScore', { score, total: matchingCardsCount });
      setCompleteMessage(message);
      setShowContinueModal(true);
    }
  };

  const handleMultipleChoiceResult = (cardId: string, result: 'REMEMBER' | 'FORGOT') => {
    // Lưu kết quả vào state thay vì gọi API ngay
    setPendingReviews((prev) => {
      const newMap = new Map(prev);
      newMap.set(cardId, { result, mode: 'FLIP' });
      return newMap;
    });

    setReviewedCount((prev) => prev + 1);

    const currentCard = cards.find((c) => c.id === cardId);
    if (result === 'REMEMBER' && currentCard && !currentCard.progress) {
      setNewWordsCount((prev) => prev + 1);
    }

    // Đánh dấu card đã hoàn thành trong phase hiện tại
    setCompletedCardsInPhase((prev) => new Set(prev).add(cardId));

    // Kiểm tra xem đã hoàn thành tất cả cards trong phase chưa
    const allCompleted = phaseCards.every(card => completedCardsInPhase.has(card.id) || card.id === cardId);

    if (allCompleted) {
      // Đã hoàn thành tất cả cards, chuyển sang matching
      setPhase('matching');
      setCompletedCardsInPhase(new Set());
    } else {
      // Chưa hoàn thành, chuyển sang card tiếp theo
      if (phaseCurrentIndex < phaseCards.length - 1) {
        setCurrentIndex(currentIndex + 1);
      }
    }
  };

  const handleListeningResult = (isCorrect: boolean) => {
    if (!currentCard) return;

    // Lưu kết quả vào state thay vì gọi API ngay
    setPendingReviews((prev) => {
      const newMap = new Map(prev);
      newMap.set(currentCard.id, {
        result: isCorrect ? 'REMEMBER' : 'FORGOT',
        mode: 'FLIP'
      });
      return newMap;
    });

    setReviewedCount((prev) => prev + 1);

    if (isCorrect && !currentCard.progress) {
      setNewWordsCount((prev) => prev + 1);
    }

    // Đánh dấu card đã hoàn thành
    setCompletedCardsInPhase((prev) => new Set(prev).add(currentCard.id));

    // Kiểm tra xem đã hoàn thành tất cả cards chưa
    const allCompleted = phaseCards.every(card => completedCardsInPhase.has(card.id) || card.id === currentCard.id);

    if (allCompleted) {
      // Đã hoàn thành tất cả, chuyển sang writing
      setPhase('writing');
      setCurrentIndex(0);
      setCompletedCardsInPhase(new Set());
    } else {
      // Chưa hoàn thành, chuyển sang card tiếp theo
      if (phaseCurrentIndex < phaseCards.length - 1) {
        setCurrentIndex(currentIndex + 1);
      }
    }
  };

  const handleWritingResult = (isCorrect: boolean) => {
    if (!currentCard) return;

    // Lưu kết quả vào state thay vì gọi API ngay
    setPendingReviews((prev) => {
      const newMap = new Map(prev);
      newMap.set(currentCard.id, {
        result: isCorrect ? 'REMEMBER' : 'FORGOT',
        mode: 'FLIP'
      });
      return newMap;
    });

    setReviewedCount((prev) => prev + 1);

    if (isCorrect && !currentCard.progress) {
      setNewWordsCount((prev) => prev + 1);
    }

    // Đánh dấu card đã hoàn thành (chỉ khi đúng)
    if (isCorrect) {
      setCompletedCardsInPhase((prev) => new Set(prev).add(currentCard.id));
    }
  };

  const handleWritingNext = async () => {
    if (!currentCard) return;

    // Kiểm tra xem đã hoàn thành tất cả cards chưa
    const allCompleted = phaseCards.every(card => completedCardsInPhase.has(card.id));

    if (allCompleted) {
      // Đã hoàn thành tất cả, submit reviews và kết thúc
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
      }

      // Submit tất cả reviews trước khi lưu log
      await submitAllReviews();

      const durationSeconds = Math.floor((Date.now() - startTime) / 1000);

      // Lưu log nhưng không hiển thị modal, trực tiếp về trang chủ
      api.post('/study/log', {
        durationSeconds,
        newWordsLearned: newWordsCount,
        cardsReviewed: reviewedCount,
        gameScore: 0,
      }).catch(error => {
        console.error('Failed to save study log:', error);
      });

      navigate('/');
    } else {
      // Chưa hoàn thành, chuyển sang card tiếp theo
      if (phaseCurrentIndex < phaseCards.length - 1) {
        setCurrentIndex(currentIndex + 1);
      } else {
        // Đã hết cards nhưng chưa hoàn thành tất cả, quay lại từ đầu
        setCurrentIndex(0);
      }
    }
  };

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  const getPhaseName = () => {
    if (phase === 'flashcard') return t('spacedRepetition.phaseFlashcard');
    if (phase === 'multiple-choice') return t('spacedRepetition.phaseMultipleChoice');
    if (phase === 'listening') return t('spacedRepetition.phaseListening');
    if (phase === 'writing') return t('spacedRepetition.phaseWriting');
    return t('spacedRepetition.phaseMatching');
  };


  if (isLoading) {
    return (
      <div className="study-container">
        <div className="study-loading">
          <Loader2 className="spin" size={48} />
          <p>{t('spacedRepetition.loading')}</p>
        </div>
      </div>
    );
  }

  if (cards.length === 0) {
    return (
      <div className="study-container">
        <div className="study-empty">
          <div className="study-empty-icon">
            <BookOpen size={64} />
          </div>
          <h2 className="study-empty-title">{t('spacedRepetition.noCards')}</h2>
          <p className="study-empty-description">
            {t('spacedRepetition.notificationExpired')}
          </p>
        </div>
      </div>
    );
  }

  // Nếu không có thẻ trong phase hiện tại và đã hoàn thành tất cả, chuyển phase
  if (!currentCard && phase !== 'matching' && phase !== 'listening' && phase !== 'writing') {
    const allCompleted = phaseCards.length > 0 && phaseCards.every(card => completedCardsInPhase.has(card.id));
    if (allCompleted) {
      if (phase === 'flashcard') {
        setPhase('multiple-choice');
        setCurrentIndex(0);
        setCompletedCardsInPhase(new Set());
      } else if (phase === 'multiple-choice') {
        setPhase('matching');
        setCompletedCardsInPhase(new Set());
      }
    }
    return null;
  }

  return (
    <div className="study-container">
      <SEO title="Spaced Repetition - Long-term Memory" />
      <div className="study-header">
        <div className="study-header-top">
          <div>
            <h1>{t('spacedRepetition.title')}</h1>
            <div className="review-mode-banner">
              <span>{t('spacedRepetition.reviewingVocabulary')}</span>
            </div>
          </div>
          <button
            onClick={() => setShowStopModal(true)}
            className="btn-stop"
          >
            {t('spacedRepetition.endEarly')}
          </button>
        </div>
        <div className="study-timer">
          <div className="timer-display">
            <span className="timer-label">{t('spacedRepetition.timeElapsed')}</span>
            <span className="timer-value">{formatTime(elapsedSeconds)}</span>
          </div>
          <div className="phase-indicator">
            <span className={`phase-badge phase-${phase}`}>{getPhaseName()}</span>
          </div>
        </div>
        <div className="study-stats">
          <span>{t('spacedRepetition.new')}: {newWordsCount}</span>
          <span>{t('spacedRepetition.review')}: {reviewedCount}</span>
          <span>{t('spacedRepetition.cards')}: {phaseCurrentIndex + 1}/{phaseCards.length}</span>
        </div>
      </div>

      <div className="study-content">
        {phase === 'matching' ? (
          <MatchingGame
            cards={phaseCards}
            onComplete={handleGameComplete}
          />
        ) : phase === 'listening' && listeningQuestion && currentCard ? (
          <ListeningPracticeCard
            key={currentCard.id}
            question={listeningQuestion}
            onResult={handleListeningResult}
            autoPlay={true}
          />
        ) : phase === 'writing' && currentCard ? (
          <WritingPracticeCard
            key={currentCard.id}
            card={currentCard}
            onResult={handleWritingResult}
            onNext={handleWritingNext}
            showNextButton={true}
          />
        ) : phase === 'multiple-choice' && currentCard ? (
          <MultipleChoiceQuestion
            key={currentCard.id}
            card={currentCard}
            allCards={cards}
            onResult={(result) => handleMultipleChoiceResult(currentCard.id, result)}
          />
        ) : phase === 'flashcard' && currentCard ? (
          <FlipCard
            key={currentCard.id}
            card={currentCard}
            onResult={(result) => handleReview(currentCard.id, result, 'FLIP')}
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
        title={t('spacedRepetition.completeTitle')}
        message={completeMessage}
        type="success"
        confirmText={t('common.close')}
        onConfirm={handleCompleteConfirm}
      />

      <Modal
        isOpen={showErrorModal}
        onClose={handleErrorConfirm}
        title={t('common.error')}
        message={errorMessage}
        type="error"
        confirmText={t('common.close')}
        onConfirm={handleErrorConfirm}
      />

      <Modal
        isOpen={showStopModal}
        onClose={() => setShowStopModal(false)}
        title={t('spacedRepetition.endEarlyTitle')}
        message={t('spacedRepetition.endEarlyConfirm')}
        type="confirm"
        confirmText={t('spacedRepetition.end')}
        cancelText={t('review.continue')}
        onConfirm={handleSessionComplete}
        onCancel={() => setShowStopModal(false)}
      />

      {showContinueModal && (
        <div className="modal-overlay" onClick={handleContinueCancel}>
          <div className="modal-container modal-confirm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-content">
              <div className="modal-icon confirm-icon">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
                  <path d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </div>
              <h2 className="modal-title">{t('spacedRepetition.completeTitle')}</h2>
              <p className="modal-message">{completeMessage}<br /><br />{t('spacedRepetition.continuePractice')}</p>
            </div>
            <div className="modal-actions" style={{ flexDirection: 'column', gap: '0.75rem' }}>
              <button
                className="modal-button modal-button-confirm"
                onClick={() => handleContinueToPractice('listening')}
                style={{ width: '100%' }}
              >
                {t('spacedRepetition.continueWithPractice')}
              </button>
              <button
                className="modal-button modal-button-cancel"
                onClick={handleContinueCancel}
                style={{ width: '100%' }}
              >
                {t('review.backHome')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

