import { useState, useEffect } from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import api from '@/lib/api';
import { ArrowLeft, Calendar, Volume2, Loader2 } from 'lucide-react';
import './LessonDetail.css';
import SEO from '@/components/common/SEO';
import moment from 'moment';
import { playPronunciation } from '@/lib/audioUtils';
import { useLanguageStore } from '@/store/languageStore';

interface Card {
  id: string;
  word: string;
  meanings?: Record<string, string>;
  meaning: string;
  example?: string;
  ipa?: string;
  explain?: string;
  partOfSpeech?: string;
  imageUrl?: string;
  otherMeanings?: Record<string, string>;
}

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

export default function LessonDetailBank() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const location = useLocation();
  const locationState = (location.state as LessonDetailLocationState) || {};
  const [lesson, setLesson] = useState<Lesson | null>(null);
  const [loading, setLoading] = useState(true);
  const [playingWords, setPlayingWords] = useState<Set<string>>(new Set());
  const { languages: countries } = useLanguageStore();

  useEffect(() => {
    if (id) {
      loadLesson();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
        navigate('/bank');
      }
    } finally {
      setLoading(false);
    }
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
      <SEO title={t('seo.bankLesson', { title: lesson?.title || t('seo.detailFallback') })} />
      <div className="lesson-detail-header">
        <div className="lesson-detail-header-left">
          <button
            className="k-btn k-btn--quiet"
            onClick={() => {
              if (locationState.from) {
                navigate(locationState.from);
              } else {
                navigate('/bank');
              }
            }}
          >
            <ArrowLeft size={16} />
            {t('common.back')}
          </button>
        </div>
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
      </div>

      <div className="cards-section k-card">
        <h2>{t('lessonDetail.vocabularyList', { count: lesson.cards.length })}</h2>
        {lesson.cards.length === 0 ? (
          <div className="cards-empty">
            <p>{t('lessonDetail.noVocabulary')}</p>
          </div>
        ) : (
          <div className="cards-grid">
            {lesson.cards.map((card, index) => (
              <div key={card.id} className="card-item">
                <div className="card-item-header">
                  <span className="card-number">#{index + 1}</span>
                </div>
                <div className="card-item-content">
                  {card.imageUrl && (
                    <img
                      src={card.imageUrl}
                      alt={card.word}
                      className="card-image"
                    />
                  )}
                  <div className="card-word-section">
                    <div className="card-word-with-pronunciation">
                      <h3 className="card-word">{card.word}</h3>
                      <button
                        className="btn-pronunciation"
                        onClick={() => handlePlayPronunciation(card.word)}
                        disabled={playingWords.has(card.word)}
                        title={t('lessonDetail.playPronunciation')}
                      >
                        <Volume2 size={16} />
                      </button>
                    </div>
                    {card.ipa && <span className="card-ipa">/{card.ipa}/</span>}
                    {card.partOfSpeech && (
                      <span className="card-pos">{card.partOfSpeech}</span>
                    )}
                  </div>
                  <div className="card-meaning">{card.meanings?.['vi'] || card.meaning}</div>
                  {card.example && (
                    <div className="card-example">
                      <span className="example-label">{t('lessonDetail.example')}:</span>
                      <span className="example-text">"{card.example}"</span>
                    </div>
                  )}
                  {card.otherMeanings && Object.keys(card.otherMeanings).length > 0 && (
                    <div className="card-other-meanings">
                      <span className="other-meanings-label">{t('lessonDetail.otherMeanings')}:</span>
                      <ul className="other-meanings-list">
                        {Object.entries(card.otherMeanings).map(([countryCode, meaning]) => {
                          const country = countries.find(c => c.code === countryCode);
                          return (
                            <li key={countryCode}>
                              <strong>{country?.flag} {country?.name || countryCode}:</strong> {meaning}
                            </li>
                          );
                        })}
                      </ul>
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

