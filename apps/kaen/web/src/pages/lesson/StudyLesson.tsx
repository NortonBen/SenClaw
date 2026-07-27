import { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { ArrowLeft, ArrowRight, X } from 'lucide-react';
import api from '@/lib/api';
import FlipCard from '@/components/study/FlipCard';
import Modal from '@/components/common/Modal';
import { useAuthStore } from '@/store/authStore';
import '../practice/Study.css';
import SEO from '@/components/common/SEO';

import { Card as BaseCard } from '@/types';

interface Card extends BaseCard {
  progress?: {
    level: number;
    isUrgent: boolean;
    nextReview?: string;
  };
}

export default function StudyLesson() {
  const { t } = useTranslation();
  const { id: lessonId } = useParams<{ id: string }>();
  const [cards, setCards] = useState<Card[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [reviewLessonTitle, setReviewLessonTitle] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [showCompleteModal, setShowCompleteModal] = useState(false);
  const [showErrorModal, setShowErrorModal] = useState(false);
  const [showStopModal, setShowStopModal] = useState(false);
  const [errorMessage, setErrorMessage] = useState('');
  const navigate = useNavigate();

  useEffect(() => {
    if (lessonId) {
      loadLessonCards(lessonId);
    } else {
      const { token } = useAuthStore.getState();
      navigate(token ? '/study' : '/lessons');
    }
  }, [lessonId, navigate]);

  const loadLessonCards = async (lessonId: string) => {
    try {
      setLoading(true);
      const { data } = await api.get(`/lessons/${lessonId}/cards`);

      const sessionCards = data.cards || [];
      setCards(sessionCards);
      setReviewLessonTitle(data.lesson?.title || null);

      if (sessionCards.length === 0) {
        setLoading(false);
        return;
      }

      setCurrentIndex(0);
    } catch (error: any) {
      console.error('Failed to load lesson cards:', error);
      const errorMsg = error.response?.data?.message || error.message || t('editLesson.loadFailed');
      setErrorMessage(errorMsg);
      setShowErrorModal(true);
    } finally {
      setLoading(false);
    }
  };

  const handleReview = async (cardId: string, result: 'REMEMBER' | 'FORGOT') => {
    try {
      const { token } = useAuthStore.getState();
      if (token) {
        await api.post(`/study/review/${cardId}`, { result, mode: 'FLIP' });

        // Reload cards để cập nhật progress mới nhất
        if (lessonId) {
          const { data } = await api.get(`/lessons/${lessonId}/cards`);
          const sessionCards = data.cards || [];
          setCards(sessionCards);
        }
      } else {
        // Guest mode: update local state only
        setCards(prev => prev.map(c =>
          c.id === cardId ? { ...c, progress: { level: result === 'REMEMBER' ? 1 : 0, isUrgent: false } } : c
        ));
      }

      // Chuyển sang card tiếp theo
      if (currentIndex < cards.length - 1) {
        setCurrentIndex(currentIndex + 1);
      } else {
        // Đã học hết tất cả cards
        setShowCompleteModal(true);
      }
    } catch (error) {
      console.error('Failed to submit review:', error);
      setErrorMessage(t('study.saveResultFailed'));
      setShowErrorModal(true);
    }
  };

  const handleCompleteConfirm = () => {
    setShowCompleteModal(false);
    navigate('/lessons');
  };

  const handleErrorConfirm = () => {
    setShowErrorModal(false);
    navigate('/lessons');
  };

  const handleStop = () => {
    setShowCompleteModal(true);
  };

  const handlePrevious = () => {
    if (currentIndex > 0) {
      setCurrentIndex(currentIndex - 1);
    }
  };

  const handleNext = () => {
    if (currentIndex < cards.length - 1) {
      setCurrentIndex(currentIndex + 1);
    }
  };

  if (loading) {
    return (
      <div className="study-container">
        <div className="study-empty">
          <p>{t('common.loading')}</p>
        </div>
      </div>
    );
  }

  if (cards.length === 0) {
    return (
      <div className="study-container">
        <div className="study-empty">
          <p>
            {reviewLessonTitle
              ? t('study.lessonNoCardsNamed', { title: reviewLessonTitle })
              : t('study.lessonNoCards')}
          </p>
          <p style={{ fontSize: '0.9rem', color: 'var(--text-light)', marginTop: '0.5rem' }}>
            {t('study.lessonAddCardsHint')}
          </p>
          <div style={{ display: 'flex', gap: '1rem', marginTop: '1.5rem' }}>
            <button
              onClick={() => navigate('/lessons')}
              className="btn-primary"
            >
              {t('review.backToLessons')}
            </button>
            {lessonId && (
              <button
                onClick={() => navigate(`/lessons/${lessonId}`)}
                className="btn-secondary"
              >
                {t('study.lessonViewDetail')}
              </button>
            )}
          </div>
        </div>
      </div>
    );
  }

  const currentCard = cards[currentIndex];
  const isFirstCard = currentIndex === 0;
  const isLastCard = currentIndex === cards.length - 1;

  return (
    <div className="study-container">
      <SEO title={t('study.lessonSeoTitle', { title: reviewLessonTitle || t('study.studyVocabulary') })} />
      <div className="study-header">
        <div className="study-header-controls">
          {reviewLessonTitle ? (
            <div className="review-mode-banner">
              <span>📚 {reviewLessonTitle}</span>
            </div>
          ) : (
            <div style={{ flex: 1 }}></div>
          )}

          <div className="study-navigation">
            <button
              onClick={handlePrevious}
              disabled={isFirstCard}
              className="btn-nav"
              title={t('study.prevCard')}
            >
              <ArrowLeft size={20} />
            </button>
            <span className="study-counter">
              {currentIndex + 1} / {cards.length}
            </span>
            <button
              onClick={handleNext}
              disabled={isLastCard}
              className="btn-nav"
              title={t('study.nextCard')}
            >
              <ArrowRight size={20} />
            </button>
          </div>

          <button
            onClick={handleStop}
            className="btn-exit"
            title={t('study.end')}
          >
            <X size={24} />
          </button>
        </div>
      </div>

      <div className="study-content">
        {currentCard && (
          <FlipCard
            key={currentCard.id}
            card={currentCard}
            onResult={(result) => handleReview(currentCard.id, result)}
          />
        )}
      </div>

      <Modal
        isOpen={showCompleteModal}
        onClose={handleCompleteConfirm}
        title={t('study.complete')}
        message={t('study.lessonCompleteMessage')}
        type="success"
        confirmText={t('common.close')}
        onConfirm={handleCompleteConfirm}
      />

      <Modal
        isOpen={showErrorModal}
        onClose={handleErrorConfirm}
        title={t('notification.title')}
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
        onConfirm={handleStop}
        onCancel={() => setShowStopModal(false)}
      />
    </div>
  );
}
