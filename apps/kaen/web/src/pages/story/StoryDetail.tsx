import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import api from '@/lib/api';
import { Loader2, ArrowLeft, Volume2, VolumeX, BookOpen, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import './StoryDetail.css';
import SEO from '@/components/common/SEO';

interface StoryStep {
  id: string;
  /** Backend uses 'STEP1' | 'STEP2' | 'STEP3'; normalized via stepKey(). */
  stepType: string;
  content: string;
  order: number;
}

interface Card {
  id: string;
  word: string;
  ipa?: string;
  partOfSpeech?: string;
  meanings?: Record<string, string>;
  example?: string;
  examples?: string[];
  explain?: string;
  otherMeanings?: string[];
  level?: string;
}

interface Story {
  id: string;
  title: string;
  topic?: string;
  description?: string;
  steps: StoryStep[];
  lesson: {
    id: string;
    title: string;
    cards: Card[];
  };
  progress?: {
    currentStep: number;
    completedSteps: number[];
    viewedVocabIds: string[];
    listenedVocabIds: string[];
  };
}

/** Normalize backend step types (STEP1/STEP2/STEP3) to kaizen's lowercase keys. */
const stepKey = (stepType: string) => stepType.toLowerCase();

export default function StoryDetail() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [story, setStory] = useState<Story | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [currentStep, setCurrentStep] = useState(1);
  const [isReading, setIsReading] = useState(false);
  const [viewedVocab, setViewedVocab] = useState<Set<string>>(new Set());
  const [listenedVocab, setListenedVocab] = useState<Set<string>>(new Set());
  const [synth, setSynth] = useState<SpeechSynthesis | null>(null);

  useEffect(() => {
    if (typeof window !== 'undefined' && 'speechSynthesis' in window) {
      setSynth(window.speechSynthesis);
    }
  }, []);

  useEffect(() => {
    if (!id) return;
    loadStory();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  useEffect(() => {
    if (story?.progress) {
      setCurrentStep(story.progress.currentStep || 1);
      setViewedVocab(new Set(story.progress.viewedVocabIds || []));
      setListenedVocab(new Set(story.progress.listenedVocabIds || []));
    }
  }, [story]);

  const loadStory = async () => {
    if (!id) return;
    try {
      setLoading(true);
      setError(null);
      const { data } = await api.get<Story>(`/stories/${id}`);
      setStory(data);
    } catch (err: any) {
      if (err.response?.status === 404) {
        setError(t('story.notFound'));
      } else {
        setError(t('story.loadError'));
      }
    } finally {
      setLoading(false);
    }
  };

  const updateProgress = async (updates: {
    currentStep?: number;
    viewedVocabIds?: string[];
    listenedVocabIds?: string[];
    incrementTtsCount?: boolean;
  }) => {
    if (!id) return;
    try {
      await api.post(`/stories/${id}/progress`, updates);
    } catch (err) {
      console.error('Failed to update story progress:', err);
    }
  };

  // Card meaning in native language
  const getCardMeaning = (card: Card): string => {
    return card.meanings?.vi || card.meanings?.vn || '';
  };

  const getCardExample = (card: Card): string => {
    return card.example || (card.examples && card.examples[0]) || '';
  };

  const handleStepChange = (step: number) => {
    setCurrentStep(step);
    updateProgress({ currentStep: step });
  };

  const speakWord = (word: string, cardId: string) => {
    if (!synth) return;
    synth.cancel();
    const utterance = new SpeechSynthesisUtterance(word);
    utterance.lang = 'en-US';
    utterance.rate = 0.9;
    synth.speak(utterance);

    const newListened = new Set(listenedVocab);
    newListened.add(cardId);
    setListenedVocab(newListened);
    updateProgress({ listenedVocabIds: Array.from(newListened) });
  };

  const toggleReading = () => {
    if (!synth || !story) return;
    if (isReading) {
      synth.cancel();
      setIsReading(false);
      return;
    }

    const currentStepData = story.steps.find((s) => s.order === currentStep);
    if (!currentStepData) return;

    // Extract text from HTML
    const tempDiv = document.createElement('div');
    tempDiv.innerHTML = currentStepData.content;
    const text = tempDiv.textContent || tempDiv.innerText || '';

    const utterance = new SpeechSynthesisUtterance(text);
    utterance.lang = 'en-US';
    utterance.rate = 0.85;
    utterance.onend = () => setIsReading(false);
    utterance.onerror = () => setIsReading(false);

    synth.speak(utterance);
    setIsReading(true);
    updateProgress({ incrementTtsCount: true });
  };

  const handleVocabClick = (cardId: string) => {
    const newViewed = new Set(viewedVocab);
    newViewed.add(cardId);
    setViewedVocab(newViewed);
    updateProgress({ viewedVocabIds: Array.from(newViewed) });
  };

  const getCurrentStepData = () => {
    if (!story) return null;
    return story.steps.find((s) => s.order === currentStep);
  };

  const getStepLabel = (stepType: string) => {
    switch (stepKey(stepType)) {
      case 'step1':
        return 'Full English';
      case 'step2':
        return 'Sandwich';
      case 'step3':
        return 'Full Vietnamese';
      default:
        return '';
    }
  };

  const getStepIcon = (stepType: string) => {
    switch (stepKey(stepType)) {
      case 'step1':
        return '🌐';
      case 'step2':
        return '📝';
      case 'step3':
        return '🇻🇳';
      default:
        return '';
    }
  };

  // Process content to highlight vocabulary words
  const processContentWithVocab = (content: string) => {
    if (!content) return content;

    // Convert newlines to <br> tags to preserve line breaks
    const processedContent = content.replace(/\n/g, '<br>');

    if (!cards.length) return processedContent;

    // Create a map of words to cards (case-insensitive)
    const wordMap = new Map<string, Card>();
    cards.forEach((card) => {
      const wordLower = card.word.toLowerCase().trim();
      if (wordLower && !wordMap.has(wordLower)) {
        wordMap.set(wordLower, card);
      }
    });

    if (wordMap.size === 0) return processedContent;

    // Sort words by length (longest first) to match compound words first
    const sortedWords = Array.from(wordMap.keys()).sort((a, b) => b.length - a.length);

    // Use DOM to process text nodes only
    const tempDiv = document.createElement('div');
    tempDiv.innerHTML = processedContent;

    const processTextNode = (node: Node): void => {
      if (node.nodeType === Node.TEXT_NODE && node.textContent) {
        const text = node.textContent;
        let modified = false;
        const fragments: Array<string | HTMLElement> = [];
        let lastIndex = 0;

        sortedWords.forEach((wordLower) => {
          const card = wordMap.get(wordLower)!;
          const escapedWord = wordLower.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
          const regex = new RegExp(`\\b${escapedWord}\\b`, 'gi');
          let match;

          while ((match = regex.exec(text)) !== null) {
            if (match.index > lastIndex) {
              fragments.push(text.substring(lastIndex, match.index));
            }
            const span = document.createElement('span');
            span.className = 'vocab-word-wrapper';
            span.setAttribute('data-word', card.word);
            span.setAttribute('data-card-id', card.id);
            span.setAttribute('data-level', card.level || '');
            span.setAttribute('data-explain', card.explain || '');
            span.setAttribute('data-meaning', getCardMeaning(card));
            span.setAttribute('data-ipa', card.ipa || '');
            span.setAttribute('data-pos', card.partOfSpeech || '');
            span.textContent = match[0];
            fragments.push(span);
            lastIndex = match.index + match[0].length;
            modified = true;
          }
        });

        if (modified && node.parentNode) {
          fragments.push(text.substring(lastIndex));
          const fragment = document.createDocumentFragment();
          fragments.forEach((item) => {
            if (typeof item === 'string') {
              fragment.appendChild(document.createTextNode(item));
            } else {
              fragment.appendChild(item);
            }
          });
          node.parentNode.replaceChild(fragment, node);
        }
      }
    };

    const walkNodes = (node: Node): void => {
      if (node.nodeType === Node.ELEMENT_NODE) {
        const element = node as Element;
        if (element.classList.contains('vocab-word-wrapper')) {
          return; // Skip already wrapped words
        }
      }
      processTextNode(node);
      if (node.childNodes.length > 0) {
        Array.from(node.childNodes).forEach(walkNodes);
      }
    };

    Array.from(tempDiv.childNodes).forEach(walkNodes);
    return tempDiv.innerHTML;
  };

  const [hoveredVocab, setHoveredVocab] = useState<{
    word: string;
    meaning: string;
    explain: string;
    ipa: string;
    pos: string;
    level: string;
    x: number;
    y: number;
    position: 'top' | 'bottom';
  } | null>(null);
  const [isVocabDialogOpen, setIsVocabDialogOpen] = useState(false);
  const [selectedVocabCard, setSelectedVocabCard] = useState<Card | null>(null);

  if (loading) {
    return (
      <div className="story-detail-loading">
        <Loader2 className="spin" size={32} />
        <p>{t('story.loading')}</p>
      </div>
    );
  }

  if (error || !story) {
    return (
      <div className="story-detail-error">
        <p>{error || t('story.notFound')}</p>
        <button type="button" className="btn-primary" onClick={() => navigate('/stories')}>
          <ArrowLeft size={16} />
          {t('common.back')}
        </button>
      </div>
    );
  }

  const currentStepData = getCurrentStepData();
  const cards = story.lesson?.cards || [];

  return (
    <div className="story-detail">
      <SEO title={t('story.seoRead', { title: story.title })} description={story.description} />
      <div className="story-detail-header">
        <div className="story-header-content">
          <div className="story-header-info">
            <h1>{story.title}</h1>
            {story.topic && <span className="story-topic-badge">{story.topic}</span>}
            {story.description && <p className="story-description-header">{story.description}</p>}
          </div>
          <button type="button" className="btn-back" onClick={() => navigate('/stories')}>
            <ArrowLeft size={18} />
            {t('common.back')}
          </button>
        </div>
      </div>

      <div className="story-detail-layout">
        <main className="story-content">
          <div className="story-toolbar">
            <div className="story-toolbar-left">
              <div className="story-steps-tabs">
                {story.steps.map((step) => (
                  <button
                    key={step.id}
                    type="button"
                    className={`story-step-tab ${currentStep === step.order ? 'active' : ''} ${currentStep > step.order ? 'completed' : ''
                      }`}
                    onClick={() => handleStepChange(step.order)}
                    title={getStepLabel(step.stepType)}
                  >
                    <div className="step-tab-content">
                      <span className="step-tab-icon">{getStepIcon(step.stepType)}</span>
                      <div className="step-tab-info">
                        <span className="step-tab-label">{getStepLabel(step.stepType)}</span>
                        <span className="step-tab-number">{t('story.stepNumber', { n: step.order })}</span>
                      </div>
                    </div>
                  </button>
                ))}
              </div>
            </div>
            <div className="story-toolbar-right">
              <button
                type="button"
                className="btn-vocab"
                onClick={() => setIsVocabDialogOpen(true)}
              >
                <BookOpen size={18} />
                <span>{t('lessons.vocabulary')}</span>
                {cards.length > 0 && (
                  <span className="vocab-count-badge">{cards.length}</span>
                )}
              </button>
              {currentStepData && stepKey(currentStepData.stepType) !== 'step3' && (
                <div className="story-toolbar-actions">
                  {isReading && (
                    <button
                      type="button"
                      className="btn-stop"
                      onClick={() => {
                        if (synth) synth.cancel();
                        setIsReading(false);
                      }}
                    >
                      <VolumeX size={18} />
                      {t('common.stop')}
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn-read-aloud"
                    onClick={toggleReading}
                    disabled={!currentStepData}
                  >
                    {isReading ? <VolumeX size={20} /> : <Volume2 size={20} />}
                    <span>{isReading ? t('story.stopReading') : t('story.readAloud')}</span>
                  </button>
                </div>
              )}
            </div>
          </div>

          <div className="story-content-display">
            {currentStepData ? (
              <>
                <div
                  className="story-text-content"
                  dangerouslySetInnerHTML={{
                    __html: processContentWithVocab(currentStepData.content),
                  }}
                  onMouseMove={(e) => {
                    const target = e.target as HTMLElement;
                    const vocabWrapper = target.closest('.vocab-word-wrapper');
                    if (vocabWrapper) {
                      const rect = vocabWrapper.getBoundingClientRect();
                      const container = document.querySelector('.story-content-display') as HTMLElement;
                      if (!container) return;

                      const containerRect = container.getBoundingClientRect();
                      const viewportHeight = window.innerHeight;
                      const tooltipHeight = 150; // Estimated tooltip height
                      const spaceBelow = viewportHeight - rect.bottom;
                      const spaceAbove = rect.top;

                      const showBelow = spaceBelow >= tooltipHeight || spaceBelow > spaceAbove;

                      const x = rect.left - containerRect.left + rect.width / 2;
                      const y = showBelow
                        ? rect.bottom - containerRect.top + 8
                        : rect.top - containerRect.top - 10;

                      setHoveredVocab({
                        word: vocabWrapper.getAttribute('data-word') || '',
                        explain: vocabWrapper.getAttribute('data-explain')?.replace(/&#39;/g, "'").replace(/&quot;/g, '"') || '',
                        meaning: vocabWrapper.getAttribute('data-meaning')?.replace(/&#39;/g, "'").replace(/&quot;/g, '"') || '',
                        ipa: vocabWrapper.getAttribute('data-ipa')?.replace(/&quot;/g, '"') || '',
                        pos: vocabWrapper.getAttribute('data-pos')?.replace(/&quot;/g, '"') || '',
                        level: vocabWrapper.getAttribute('data-level') || '',
                        x,
                        y,
                        position: showBelow ? 'bottom' : 'top',
                      });
                    } else {
                      setHoveredVocab(null);
                    }
                  }}
                  onMouseLeave={() => setHoveredVocab(null)}
                  onClick={(e) => {
                    const target = e.target as HTMLElement;
                    const vocabWrapper = target.closest('.vocab-word-wrapper');
                    if (vocabWrapper) {
                      const cardId = vocabWrapper.getAttribute('data-card-id');
                      if (cardId) {
                        const card = cards.find((c) => c.id === cardId);
                        if (card) {
                          setSelectedVocabCard(card);
                          handleVocabClick(cardId);
                        }
                      }
                    }
                  }}
                />
                {hoveredVocab && (
                  <div
                    className={`vocab-tooltip vocab-tooltip-${hoveredVocab.position}`}
                    style={{
                      left: `${hoveredVocab.x}px`,
                      top: `${hoveredVocab.y}px`,
                    }}
                  >
                    <div className="vocab-tooltip-header">
                      <div>
                        <h4>{hoveredVocab.word}</h4>
                        <span className="vocab-tooltip-ipa">{hoveredVocab.ipa}</span>
                        <span className="vocab-tooltip-pos">{hoveredVocab.pos}</span>
                      </div>
                      {hoveredVocab.level && (
                        <span
                          className={`vocab-tooltip-level ${hoveredVocab.level === 'beginner' ? 'level-beginner' : 'level-intermediate'
                            }`}
                        >
                          {hoveredVocab.level === 'beginner' ? t('story.levelBeginner') : t('story.levelIntermediate')}
                        </span>
                      )}
                    </div>
                    <p className="vocab-tooltip-meaning">{hoveredVocab.meaning}</p>
                    <p className="vocab-tooltip-explain">{hoveredVocab.explain}</p>
                  </div>
                )}
              </>
            ) : (
              <p>{t('story.stepContentMissing')}</p>
            )}
          </div>
        </main>
      </div>

      {/* Single Vocab Word Dialog */}
      {selectedVocabCard && (
        <div className="vocab-word-dialog-overlay" onClick={() => setSelectedVocabCard(null)}>
          <div className="vocab-word-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="vocab-word-dialog-header">
              <div className="vocab-word-dialog-title">
                <h2>{selectedVocabCard.word}</h2>
                <div className="vocab-word-dialog-meta">
                  <span className="vocab-word-ipa">{selectedVocabCard.ipa}</span>
                  <span className="vocab-word-pos">{selectedVocabCard.partOfSpeech}</span>
                  {selectedVocabCard.level && (
                    <span
                      className={`vocab-word-level ${selectedVocabCard.level === 'beginner' ? 'level-beginner' : 'level-intermediate'
                        }`}
                    >
                      {selectedVocabCard.level === 'beginner' ? t('story.levelBeginner') : t('story.levelIntermediate')}
                    </span>
                  )}
                </div>
              </div>
              <button
                type="button"
                className="vocab-word-dialog-close"
                onClick={() => setSelectedVocabCard(null)}
              >
                <X size={20} />
              </button>
            </div>
            <div className="vocab-word-dialog-content">
              <div className="vocab-word-meaning-section">
                <h3>{t('story.meaning')}</h3>
                <p className="vocab-word-meaning">{getCardMeaning(selectedVocabCard)}</p>
              </div>
              {getCardExample(selectedVocabCard) && (
                <div className="vocab-word-example-section">
                  <h3>{t('story.example')}</h3>
                  <p className="vocab-word-example">{getCardExample(selectedVocabCard)}</p>
                </div>
              )}
              {selectedVocabCard.otherMeanings && selectedVocabCard.otherMeanings.length > 0 && (
                <div className="vocab-word-other-meanings-section">
                  <h3>{t('story.otherMeanings')}</h3>
                  <ul className="vocab-word-other-meanings-list">
                    {selectedVocabCard.otherMeanings.map((meaning, index) => (
                      <li key={index}>{meaning}</li>
                    ))}
                  </ul>
                </div>
              )}
              <div className="vocab-word-dialog-actions">
                <button
                  type="button"
                  className="btn-vocab-speak"
                  onClick={() => {
                    if (selectedVocabCard) {
                      speakWord(selectedVocabCard.word, selectedVocabCard.id);
                    }
                  }}
                >
                  {listenedVocab.has(selectedVocabCard.id) ? (
                    <Volume2 size={20} />
                  ) : (
                    <VolumeX size={20} />
                  )}
                  <span>{t('story.pronounce')}</span>
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Vocabulary Dialog */}
      {isVocabDialogOpen && (
        <div className="vocab-dialog-overlay" onClick={() => setIsVocabDialogOpen(false)}>
          <div className="vocab-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="vocab-dialog-header">
              <div className="vocab-dialog-title">
                <span className="vocab-header-icon">📖</span>
                <h2>{t('lessons.vocabulary')}</h2>
                {cards.length > 0 && (
                  <span className="vocab-badge">{t('lessons.wordCount', { count: cards.length })}</span>
                )}
              </div>
              <button
                type="button"
                className="vocab-dialog-close"
                onClick={() => setIsVocabDialogOpen(false)}
              >
                <X size={20} />
              </button>
            </div>
            <div className="vocab-dialog-content">
              {cards.length === 0 ? (
                <p className="vocab-empty">{t('story.noVocabulary')}</p>
              ) : (
                <div className="vocab-gallery">
                  {cards.map((card) => (
                    <div
                      key={card.id}
                      className={`vocab-card ${viewedVocab.has(card.id) ? 'viewed' : ''} ${card.level === 'beginner' ? 'level-beginner' : 'level-intermediate'
                        }`}
                      onClick={() => handleVocabClick(card.id)}
                    >
                      <div className="vocab-card-header">
                        <div>
                          <h4>{card.word}</h4>
                          <span className="vocab-ipa">{card.ipa}</span>
                          <span className="vocab-pos">{card.partOfSpeech}</span>
                        </div>
                        <button
                          type="button"
                          className="vocab-speak-btn"
                          onClick={(e) => {
                            e.stopPropagation();
                            speakWord(card.word, card.id);
                          }}
                        >
                          {listenedVocab.has(card.id) ? (
                            <Volume2 size={18} />
                          ) : (
                            <VolumeX size={18} />
                          )}
                        </button>
                      </div>
                      <p className="vocab-meaning">{getCardMeaning(card)}</p>
                      {getCardExample(card) && (
                        <p className="vocab-example">{getCardExample(card)}</p>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
