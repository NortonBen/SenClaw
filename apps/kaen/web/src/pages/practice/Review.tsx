import { useState, useEffect } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { BookOpen, Sparkles, ArrowRight, RotateCcw, Loader2 } from 'lucide-react';
import api from '@/lib/api';
import { useAuthStore } from '@/store/authStore';
import Modal from '@/components/common/Modal';
import ReviewCard from '@/components/study/ReviewCard';
import { Card } from '@/types';
import './Review.css';
import SEO from '@/components/common/SEO';
import { playPronunciation as basePlayPronunciation } from '@/lib/audioUtils';

interface Question {
  card: Card;
  questionType: 'word' | 'explain'; // 'word' = hiện từ, chọn explain | 'explain' = hiện explain, chọn từ
  options: string[];
  correctAnswer: string;
}

export default function Review() {
  const { t } = useTranslation();
  const navigate = useNavigate();
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
  const [isRepeatSession, setIsRepeatSession] = useState(false);
  const [originalCardsCount, setOriginalCardsCount] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [reviewResults, setReviewResults] = useState<Map<string, boolean>>(new Map());

  useEffect(() => {
    let isMounted = true;
    const abortController = new AbortController();

    const load = async () => {
      try {
        setLoading(true);
        setError(null);
        const { data } = await api.get('/review/session', {
          params: {
            allowRepeat: false,
          },
          signal: abortController.signal,
        });

        if (!isMounted) return;

        const sessionCards = data.cards || [];

        if (sessionCards.length === 0) {
          setError(t('review.noWordsToReview'));
          setLoading(false);
          return;
        }

        setCards(sessionCards);
        setOriginalCardsCount(sessionCards.length);
        setIsRepeatSession(false);
        setCurrentQuestionIndex(0);
        setScore(0);
        setSelectedAnswer(null);
        setShowResult(false);
        setReviewResults(new Map()); // Reset kết quả review
        generateQuestion(sessionCards, 0);
        setLoading(false);
      } catch (err: any) {
        if (err.name === 'CanceledError' || err.name === 'AbortError') {
          return;
        }
        if (isMounted) {
          console.error('Failed to load review session:', err);
          setError(t('review.loadFailed'));
          setLoading(false);
        }
      }
    };

    load();

    return () => {
      isMounted = false;
      abortController.abort();
    };
  }, []);

  // Debug: Log khi currentQuestion thay đổi
  useEffect(() => {
    console.log('[Review] currentQuestion changed:', currentQuestion);
  }, [currentQuestion]);

  // Debug: Log khi cards thay đổi
  useEffect(() => {
    console.log('[Review] cards changed:', cards.length, cards);
  }, [cards]);

  const loadReviewSession = async (allowRepeat: boolean = false, signal?: AbortSignal) => {
    try {
      setLoading(true);
      setError(null); // Clear error khi load lại
      console.log(`[Review] Loading session with allowRepeat: ${allowRepeat}`);
      const { data } = await api.get('/review/session', {
        params: {
          allowRepeat: allowRepeat ? 'true' : undefined,
        },
        signal,
      });
      console.log(`[Review] Received ${data.cards?.length || 0} cards`, data);
      const sessionCards = data.cards || [];

      if (sessionCards.length === 0) {
        if (allowRepeat || isRepeatSession) {
          // Đã ôn lại rồi mà vẫn không có từ, thông báo hoàn thành
          setError(t('review.allWordsDone'));
        } else {
          setError(t('review.noWordsToReview'));
        }
        setLoading(false);
        return;
      }

      // Lưu số lượng từ ban đầu (lần đầu tiên)
      if (!allowRepeat && originalCardsCount === 0) {
        setOriginalCardsCount(sessionCards.length);
      }

      setCards(sessionCards);
      setIsRepeatSession(allowRepeat);
      setCurrentQuestionIndex(0); // Reset index
      setScore(0); // Reset score
      setSelectedAnswer(null); // Reset selected answer
      setShowResult(false); // Reset show result
      setReviewResults(new Map()); // Reset kết quả review
      generateQuestion(sessionCards, 0);
      setLoading(false);
    } catch (err: any) {
      if (err.name === 'CanceledError' || err.name === 'AbortError') {
        return; // Request was cancelled, ignore
      }
      console.error('Failed to load review session:', err);
      setError(t('review.loadFailed'));
      setLoading(false);
    }
  };

  const generateQuestion = (cardList: Card[], index: number) => {
    console.log(`[Review] generateQuestion - index: ${index}, cardList.length: ${cardList.length}`);

    if (!cardList || cardList.length === 0) {
      console.warn('[Review] generateQuestion - cardList is empty');
      setCurrentQuestion(null);
      return;
    }

    if (index >= cardList.length) {
      // Đã hết câu hỏi
      console.log('[Review] generateQuestion - index out of range');
      setCurrentQuestion(null);
      return;
    }

    const card = cardList[index];

    if (!card || !card.word) {
      console.error('[Review] generateQuestion - invalid card:', card);
      setCurrentQuestion(null);
      return;
    }

    // Random chọn kiểu câu hỏi: 'word' hoặc 'explain'
    const isExplainType = Math.random() > 0.5;
    const questionType: 'word' | 'explain' = isExplainType ? 'explain' : 'word';

    let correctAnswer: string;
    const wrongOptions: string[] = [];

    const otherCards = cardList.filter((c) => c.id !== card.id);
    const shuffledOthers = [...otherCards].sort(() => Math.random() - 0.5);

    if (questionType === 'word') {
      // Hiện từ (Word) -> Chọn giải thích (Explain)
      correctAnswer = card.explain || card.meanings?.['vi'] || '';

      for (let i = 0; i < Math.min(3, shuffledOthers.length); i++) {
        const wrongCard = shuffledOthers[i];
        const wrongAnswer = wrongCard.explain || wrongCard.meanings?.['vi'] || '';
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
        // Với word option thì khó fake hơn, accept ít option hơn
        break;
      }
    }

    // Trộn đáp án
    const allOptions = [correctAnswer, ...wrongOptions];
    const shuffledOptions = allOptions.sort(() => Math.random() - 0.5);

    const question: Question = {
      card,
      questionType,
      options: shuffledOptions,
      correctAnswer,
    };

    console.log('[Review] generateQuestion - setting question:', question);
    setCurrentQuestion(question);
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
      console.log(`[Review] Stored review result for card ${currentQuestion.card.id}, isCorrect: ${correct}`);
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
      console.log('[Review] Guest mode: skipping batch submit');
      return { success: true, submitted: 0, total: 0 };
    }

    try {
      console.log(`[Review] Submitting batch review for ${results.length} cards...`);
      const response = await api.post('/review/submit/batch', { results });
      console.log(`[Review] Batch review submitted successfully:`, response.data);
      return response.data;
    } catch (error: any) {
      console.error(`[Review] Failed to submit batch review:`, error);
      // Ném lỗi để caller có thể xử lý
      throw error;
    }
  };

  const handlePlayPronunciation = async () => {
    if (!currentQuestion || !currentQuestion.card.word) return;
    await basePlayPronunciation(currentQuestion.card.word, () => setIsPlaying(true), () => setIsPlaying(false));
  };

  const handleComplete = async () => {
    // Submit batch tất cả kết quả review trước khi hoàn thành
    const results = Array.from(reviewResults.entries()).map(([cardId, isCorrect]) => ({
      cardId,
      isCorrect,
    }));

    if (results.length > 0) {
      try {
        await submitBatchReview(results);
        console.log(`[Review] Submitted batch review for ${results.length} cards`);
        // Reset kết quả sau khi submit thành công
        setReviewResults(new Map());
      } catch (error) {
        console.error('[Review] Failed to submit batch review:', error);
        // Vẫn tiếp tục dù có lỗi để không ảnh hưởng trải nghiệm
      }
    }

    const percentage = Math.round((score / cards.length) * 100);
    const message = t('review.completeMessage', { score, total: cards.length, percentage });

    // Luôn hiển thị dialog hỏi có muốn review tiếp không
    setCompleteMessage(message);
    setShowCompleteModal(true);
  };

  const handleContinueReview = () => {
    setShowCompleteModal(false);
    setCurrentQuestionIndex(0);
    setScore(0);
    setReviewResults(new Map()); // Reset kết quả review

    // Nếu chưa phải lần ôn lại, load round 2
    if (!isRepeatSession) {
      setIsRepeatSession(true); // Đánh dấu là round 2
      loadReviewSession(false); // allowRepeat = false cho round 2
    } else {
      // Đã ôn lại rồi, reset và load lại từ đầu
      setIsRepeatSession(false);
      setOriginalCardsCount(0);
      loadReviewSession(false);
    }
  };

  const handleFinishReview = () => {
    setShowCompleteModal(false);
    setIsRepeatSession(false);
    setOriginalCardsCount(0);
    navigate('/');
  };

  if (loading) {
    return (
      <div className="review-container">
        <div className="review-loading k-card">
          <Loader2 className="spin" size={40} />
          <p>{t('review.loading')}</p>
        </div>
      </div>
    );
  }

  if (error) {
    const isNoWordsError = error.includes(t('review.noWordsToReview')) || error.includes('hoàn thành tất cả từ vựng');

    return (
      <div className="review-container">
        <div className={`review-error k-card ${isNoWordsError ? 'review-empty-state' : ''}`}>
          <div className="review-error-icon">
            {isNoWordsError ? (
              <Sparkles size={30} strokeWidth={1.6} />
            ) : (
              <BookOpen size={30} strokeWidth={1.6} />
            )}
          </div>
          <h2 className="review-error-title">{isNoWordsError ? t('review.noWordsTitle') : t('review.errorTitle')}</h2>
          <p className="review-error-message">{error}</p>
          {isNoWordsError ? (
            <div className="review-empty-actions">
              <Link to="/study" className="k-btn k-btn--primary">
                <BookOpen size={17} />
                <span>{t('review.startLearning')}</span>
                <ArrowRight size={17} />
              </Link>
              {!isRepeatSession && (
                <button onClick={() => loadReviewSession(true)} className="k-btn k-btn--ghost">
                  <RotateCcw size={17} />
                  <span>{t('review.retryReviewed')}</span>
                </button>
              )}
              <button onClick={() => navigate('/')} className="k-btn k-btn--quiet">
                {t('review.backHome')}
              </button>
            </div>
          ) : (
            <div className="review-error-actions">
              <button onClick={() => navigate('/')} className="k-btn k-btn--primary">
                {t('review.backHome')}
              </button>
              <button onClick={() => loadReviewSession(true)} className="k-btn k-btn--ghost">
                {t('review.retry')}
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }

  if (!currentQuestion) {
    return (
      <div className="review-container">
        <div className="review-empty k-card">
          <p>{t('review.noQuestions')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="review-container">
      <SEO title={t('seo.quickReview')} description={t('seo.quickReviewDesc')} />
      <div className="review-header">
        <div className="k-page-head review-head">
          <div>
            <h1>{t('review.title')}</h1>
            {isRepeatSession && (
              <span className="k-chip review-repeat-chip">{t('review.repeatSession')}</span>
            )}
          </div>
          <button
            onClick={() => setShowStopModal(true)}
            className="k-btn k-btn--ghost review-stop"
          >
            {t('review.stopReview')}
          </button>
        </div>
        <div className="review-progress">
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{
                width: `${((currentQuestionIndex + 1) / cards.length) * 100}%`,
              }}
            />
          </div>
          <div className="progress-text k-num">
            {t('review.progress', { current: currentQuestionIndex + 1, total: cards.length, score })}
          </div>
        </div>
      </div>

      <div className="review-content">
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
        message={completeMessage + '\n\n' + t('review.continueReview')}
        type="confirm"
        confirmText={t('review.continueReviewButton')}
        cancelText={t('review.backHome')}
        onConfirm={handleContinueReview}
        onCancel={handleFinishReview}
      />

      <Modal
        isOpen={showStopModal}
        onClose={() => setShowStopModal(false)}
        title={t('review.stopTitle')}
        message={t('review.stopConfirm')}
        type="confirm"
        confirmText={t('review.stop')}
        cancelText={t('review.continue')}
        onConfirm={() => navigate('/')}
        onCancel={() => setShowStopModal(false)}
      />
    </div>
  );
}
