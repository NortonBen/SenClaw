import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Link2 } from 'lucide-react';
import api from '@/lib/api';
import Modal from '@/components/common/Modal';
import MatchingGame from '@/components/game/MatchingGame';
import { Card } from '@/types';
import './MatchingPractice.css';
import SEO from '@/components/common/SEO';

export default function MatchingPractice() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [cards, setCards] = useState<Card[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showCompleteModal, setShowCompleteModal] = useState(false);
  const [showStopModal, setShowStopModal] = useState(false);
  const [completeMessage, setCompleteMessage] = useState('');
  const [gameStarted, setGameStarted] = useState(false);

  useEffect(() => {
    loadSession();
  }, []);

  const loadSession = async () => {
    try {
      setLoading(true);
      setError(null);
      const { data } = await api.get('/matching/session');
      const sessionCards = data.cards || [];

      if (sessionCards.length === 0) {
        setError(t('matching.sessionNoCards'));
        setLoading(false);
        return;
      }

      setCards(sessionCards);
      setGameStarted(false);
      setLoading(false);
    } catch (err: any) {
      console.error('Failed to load session:', err);
      setError(t('matching.loadError'));
      setLoading(false);
    }
  };

  const handleGameStart = () => {
    setGameStarted(true);
  };

  const handleGameComplete = async (totalScore: number) => {
    const totalCards = cards.length;
    const percentage = Math.round((totalScore / totalCards) * 100);
    const message = t('matching.completeMessage', { score: totalScore, total: totalCards, percentage });
    setCompleteMessage(message);

    // Submit kết quả cho tất cả các cards
    // Trong MatchingGame, score là số cards đã match đúng
    // Khi game hoàn thành (onComplete được gọi), có nghĩa là tất cả cards trong tất cả rounds đã được match
    // Vì vậy tất cả cards đều được submit là đúng
    for (const card of cards) {
      try {
        await api.post(`/matching/submit/${card.id}`, { isCorrect: true });
      } catch (error) {
        console.error(`Failed to submit result for card ${card.id}:`, error);
      }
    }

    setShowCompleteModal(true);
  };

  const handleContinue = () => {
    setShowCompleteModal(false);
    setGameStarted(false);
    loadSession();
  };

  const handleFinish = () => {
    setShowCompleteModal(false);
    navigate('/');
  };


  if (loading) {
    return (
      <div className="matching-practice-container">
        <div className="matching-practice-loading">
          <p>{t('matching.loading')}</p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="matching-practice-container">
        <div className="matching-practice-error">
          <div className="matching-practice-error-icon">
            <Link2 size={64} strokeWidth={1.5} />
          </div>
          <h2 className="matching-practice-error-title">{t('common.error')}</h2>
          <p className="matching-practice-error-message">{error}</p>
          <div className="matching-practice-error-actions">
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

  if (!gameStarted) {
    return (
      <div className="matching-practice-container">
        <div className="matching-practice-header">
          <div className="matching-practice-header-top">
            <h1>{t('matching.title')}</h1>
            <button
              onClick={() => navigate('/')}
              className="btn-stop"
            >
              {t('common.stop')}
            </button>
          </div>
        </div>

        <div className="matching-practice-start">
          <div className="matching-practice-start-card">
            <div className="matching-practice-start-icon">
              <Link2 size={64} strokeWidth={1.5} />
            </div>
            <h2>{t('matching.startTitle')}</h2>
            <p>{t('matching.startDescription')}</p>
            <button onClick={handleGameStart} className="btn-primary btn-large">
              {t('matching.start')}
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="matching-practice-container">
      <SEO title="Matching Practice - Fast Reflexes" />
      <div className="matching-practice-header">
        <div className="matching-practice-header-top">
          <h1>{t('matching.title')}</h1>
          <button
            onClick={() => setShowStopModal(true)}
            className="btn-stop"
          >
            {t('matching.stop')}
          </button>
        </div>
      </div>

      <div className="matching-practice-content">
        <MatchingGame
          cards={cards}
          onComplete={handleGameComplete}
        />
      </div>

      <Modal
        isOpen={showCompleteModal}
        onClose={() => setShowCompleteModal(false)}
        title={t('matching.completeTitle')}
        message={completeMessage + '\n\n' + t('matching.continuePrompt')}
        type="confirm"
        confirmText={t('common.continue')}
        cancelText={t('common.backToHome')}
        onConfirm={handleContinue}
        onCancel={handleFinish}
      />

      <Modal
        isOpen={showStopModal}
        onClose={() => setShowStopModal(false)}
        title={t('matching.stopConfirmTitle')}
        message={t('matching.stopConfirmMessage')}
        type="confirm"
        confirmText={t('common.stop')}
        cancelText={t('common.continue')}
        onConfirm={() => navigate('/')}
        onCancel={() => setShowStopModal(false)}
      />
    </div>
  );
}

