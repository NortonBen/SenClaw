import { useState, useEffect } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { PenTool } from 'lucide-react';
import api from '@/lib/api';
import Modal from '@/components/common/Modal';
import WritingPracticeCard from '@/components/study/WritingPracticeCard';
import { Card } from '@/types';
import './WritingPractice.css';
import SEO from '@/components/common/SEO';

export default function WritingPractice() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const [cards, setCards] = useState<Card[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [currentCard, setCurrentCard] = useState<Card | null>(null);
  const [completedCards, setCompletedCards] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCompleteModal, setShowCompleteModal] = useState(false);
  const [showStopModal, setShowStopModal] = useState(false);
  const [completeMessage, setCompleteMessage] = useState('');

  useEffect(() => {
    loadSession();
  }, []);

  const loadSession = async () => {
    try {
      setLoading(true);
      setError(null);

      // Kiểm tra xem có cards từ spaced-repetition không
      const fromSpacedRepetition = searchParams.get('from') === 'spaced-repetition';
      let sessionCards: Card[] = [];

      if (fromSpacedRepetition) {
        const storedCards = sessionStorage.getItem('spacedRepetitionCards');
        if (storedCards) {
          try {
            sessionCards = JSON.parse(storedCards);
            // Xóa cards khỏi sessionStorage sau khi sử dụng
            sessionStorage.removeItem('spacedRepetitionCards');
          } catch (e) {
            console.error('Failed to parse stored cards:', e);
          }
        }
      }

      // Nếu không có cards từ spaced-repetition, gọi API
      if (sessionCards.length === 0) {
        const { data } = await api.get('/writing/session');
        sessionCards = data.cards || [];
      }

      if (sessionCards.length === 0) {
        setError(t('writing.sessionNoCards'));
        setLoading(false);
        return;
      }

      setCards(sessionCards);
      setCurrentIndex(0);
      setCompletedCards(new Set());
      setCurrentCard(sessionCards[0]);
      setLoading(false);
    } catch (err: any) {
      console.error('Failed to load session:', err);
      setError(t('writing.loadError'));
      setLoading(false);
    }
  };

  const handleWritingResult = (isCorrect: boolean) => {
    if (!currentCard) return;

    if (isCorrect) {
      setCompletedCards((prev) => new Set(prev).add(currentCard.id));
      submitReview(currentCard.id, true);
    } else {
      submitReview(currentCard.id, false);
    }
  };

  const submitReview = async (cardId: string, isCorrect: boolean) => {
    try {
      await api.post(`/writing/submit/${cardId}`, { isCorrect });
    } catch (error) {
      console.error('Failed to submit writing result:', error);
    }
  };

  const handleNext = () => {
    // Tìm câu tiếp theo chưa hoàn thành
    let nextIndex = currentIndex;
    let found = false;

    // Tìm từ tiếp theo chưa hoàn thành
    for (let i = 0; i < cards.length; i++) {
      const checkIndex = (currentIndex + i + 1) % cards.length;
      const card = cards[checkIndex];
      if (!completedCards.has(card.id)) {
        nextIndex = checkIndex;
        found = true;
        break;
      }
    }

    // Nếu tất cả đã hoàn thành, kết thúc
    if (!found || completedCards.size === cards.length) {
      handleComplete();
      return;
    }

    setCurrentIndex(nextIndex);
    setCurrentCard(cards[nextIndex]);
  };

  const handleComplete = () => {
    // Chỉ kết thúc khi đã trả lời đúng tất cả
    if (completedCards.size === cards.length) {
      const message = t('writing.completeMessage', { count: cards.length });
      setCompleteMessage(message);
      setShowCompleteModal(true);
    }
  };

  const handleContinue = () => {
    setShowCompleteModal(false);
    setCurrentIndex(0);
    loadSession();
  };

  const handleFinish = () => {
    setShowCompleteModal(false);
    navigate('/');
  };


  if (loading) {
    return (
      <div className="writing-container">
        <div className="writing-loading">
          <p>{t('writing.loading')}</p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="writing-container">
        <div className="writing-error">
          <div className="writing-error-icon">
            <PenTool size={64} strokeWidth={1.5} />
          </div>
          <h2 className="writing-error-title">{t('common.error')}</h2>
          <p className="writing-error-message">{error}</p>
          <div className="writing-error-actions">
            <button onClick={() => navigate('/')} className="btn-primary">
              {t('common.backToHome')}
            </button>
            <button onClick={() => loadSession()} className="btn-secondary">
              {t('common.retry')}
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (!currentCard) {
    return (
      <div className="writing-container">
        <div className="writing-empty">
          <p>{t('writing.noCards')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="writing-container">
      <SEO title="Writing Practice - Spelling & Recall" />
      <div className="writing-header">
        <div className="writing-header-top">
          <h1>{t('writing.title')}</h1>
          <button
            onClick={() => setShowStopModal(true)}
            className="btn-stop"
          >
            {t('writing.stop')}
          </button>
        </div>
        <div className="writing-progress">
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{
                width: `${((currentIndex + 1) / cards.length) * 100}%`,
              }}
            />
          </div>
          <div className="progress-text">
            {t('writing.progress', { current: currentIndex + 1, total: cards.length, completed: completedCards.size })}
          </div>
        </div>
      </div>

      <div className="writing-content">
        {currentCard && (
          <WritingPracticeCard
            card={currentCard}
            onResult={handleWritingResult}
            onNext={handleNext}
            showNextButton={true}
          />
        )}
      </div>

      <Modal
        isOpen={showCompleteModal}
        onClose={() => setShowCompleteModal(false)}
        title={t('writing.completeTitle')}
        message={completeMessage + '\n\n' + t('writing.continuePrompt')}
        type="confirm"
        confirmText={t('writing.continue')}
        cancelText={t('common.backToHome')}
        onConfirm={handleContinue}
        onCancel={handleFinish}
      />

      <Modal
        isOpen={showStopModal}
        onClose={() => setShowStopModal(false)}
        title={t('writing.stopConfirmTitle')}
        message={t('writing.stopConfirmMessage')}
        type="confirm"
        confirmText={t('common.stop')}
        cancelText={t('common.continue')}
        onConfirm={() => navigate('/')}
        onCancel={() => setShowStopModal(false)}
      />
    </div>
  );
}

