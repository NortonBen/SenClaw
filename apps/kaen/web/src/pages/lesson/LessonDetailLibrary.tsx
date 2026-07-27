import { useState, useEffect } from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import api from '@/lib/api';
import {
  ArrowLeft,
  Calendar,
  BookOpen,
  RotateCcw,
  ListChecks,
  Loader2,
} from 'lucide-react';
import './LessonDetail.css';
import SEO from '@/components/common/SEO';
import moment from 'moment';

import { Card } from '@/types';
import CardItem from '@/components/study/CardItem';
import { playPronunciation } from '@/lib/audioUtils';

interface LessonDetailLocationState {
  from?: string;
}

interface Lesson {
  id: string;
  title: string;
  description?: string;
  createdAt: string;
  cards: Card[];
}

export default function LessonDetailLibrary() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const location = useLocation();
  const locationState = (location.state as LessonDetailLocationState) || {};
  const [lesson, setLesson] = useState<Lesson | null>(null);
  const [loading, setLoading] = useState(true);
  const [playingWords, setPlayingWords] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (id) {
      loadLesson();
    }
  }, [id]);


  const loadLesson = async () => {
    try {
      setLoading(true);
      const { data } = await api.get(`/lessons/${id}`);

      // Transform card meanings
      const cardsData = data.cards.map((card: any) => {
        const meanings = card.meanings || {};
        const otherMeanings = { ...meanings };
        delete otherMeanings['vi'];

        return {
          ...card,
          otherMeanings
        };
      });
      data.cards = cardsData;

      setLesson(data);
    } catch (error: any) {
      console.error('Failed to load lesson:', error);
      if (error.response?.status === 404) {
        alert(t('lessonDetail.notFound'));
        navigate('/lessons');
      }
    } finally {
      setLoading(false);
    }
  };

  const handleStudy = () => {
    if (!lesson) return;
    navigate(`/study/lesson/${lesson.id}`);
  };

  const handleReview = () => {
    if (!lesson) return;
    navigate(`/study/review/${lesson.id}`);
  };

  const handleScrollToCards = () => {
    const section = document.getElementById('library-lesson-cards');
    section?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  };

  const formatDate = (dateString: string) => {
    // createdAt từ backend là UTC -> dùng moment để hiển thị theo giờ local của trình duyệt
    return moment.utc(dateString).local().format('D MMMM YYYY');
  };

  const handlePlayPronunciation = async (word: string) => {
    await playPronunciation(
      word,
      () => setPlayingWords((prev) => new Set(prev).add(word)),
      () => setPlayingWords((prev) => {
        const next = new Set(prev);
        next.delete(word);
        return next;
      })
    );
  };

  if (loading) {
    return (
      <div className="lesson-detail">
        <div className="lesson-detail-loading">
          <Loader2 className="spin" size={32} />
          <p>{t('common.loading')}</p>
        </div>
      </div>
    );
  }

  if (!lesson) {
    return (
      <div className="lesson-detail">
        <div className="lesson-detail-error">{t('lessonDetail.notFound')}</div>
      </div>
    );
  }

  return (
    <div className="lesson-detail">
      <SEO title={t('seo.lesson', { title: lesson?.title || t('seo.vocabularyListFallback') })} />
      <div className="lesson-detail-header">
        <button
          className="k-btn k-btn--quiet"
          onClick={() => {
            if (locationState.from) {
              navigate(locationState.from);
            } else {
              navigate('/lessons');
            }
          }}
        >
          <ArrowLeft size={16} />
          {t('common.back')}
        </button>
      </div>

      <div className="lesson-info-card k-card">
        <div className="lesson-info-header">
          <div className="lesson-info-title-section">
            <h1>{lesson.title}</h1>
            <div className="lesson-info-meta">
              <div className="lesson-meta-item">
                <Calendar size={16} />
                <span>{formatDate(lesson.createdAt)}</span>
              </div>
            </div>
          </div>
        </div>

        {lesson.description && (
          <p className="lesson-description">{lesson.description}</p>
        )}

        <div className="lesson-stats">
          <div className="lesson-stat-item">
            <span className="stat-value k-num">{lesson.cards.length}</span>
            <span className="stat-label">{t('lessonDetail.vocabulary')}</span>
          </div>
        </div>

        <div className="lesson-detail-actions">
          <div className="lesson-detail-actions-primary">
            <button className="k-btn k-btn--primary" onClick={handleStudy}>
              <BookOpen size={16} />
              <span>{t('lessonDetail.study')}</span>
            </button>
            <button className="k-btn k-btn--ghost" onClick={handleReview}>
              <RotateCcw size={15} />
              <span>{t('lessonDetail.review')}</span>
            </button>
          </div>

          <div className="lesson-detail-actions-secondary">
            <button className="k-btn k-btn--quiet" onClick={handleScrollToCards}>
              <ListChecks size={15} />
              <span>{t('lessonDetail.vocabulary')}</span>
            </button>
          </div>
        </div>
      </div>

      <div className="cards-section k-card" id="library-lesson-cards">
        <h2>{t('lessonDetail.vocabularyList', { count: lesson.cards.length })}</h2>
        {lesson.cards.length === 0 ? (
          <div className="cards-empty">
            <p>{t('lessonDetail.noVocabulary')}</p>
          </div>
        ) : (
          <div className="cards-grid">
            {lesson.cards.map((card, index) => (
              <CardItem
                key={card.id}
                card={card}
                index={index}
                isViewMode={true}
                showImage={true}
                onPlayPronunciation={handlePlayPronunciation}
                isPlaying={playingWords.has(card.word)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

