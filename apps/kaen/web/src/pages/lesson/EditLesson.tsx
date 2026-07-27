import { useState, useEffect } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Plus, X, FileText, Upload } from 'lucide-react';
import api from '@/lib/api';
import { useAuthStore } from '@/store/authStore';
import { useLanguageStore } from '@/store/languageStore';
import ImportDialog from '@/components/common/ImportDialog';
import CardItem from '@/components/study/CardItem';
import './EditLesson.css';
import SEO from '@/components/common/SEO';

import { Card as BaseCard } from '@/types';

interface Card extends Omit<BaseCard, 'id' | 'lessonId'> {
  id?: string;
  lessonId?: string; // Optional since we are editing/creating
}

export default function EditLesson() {
  const { t } = useTranslation();
  const { user } = useAuthStore();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [cards, setCards] = useState<Card[]>([]);
  const [originalCards, setOriginalCards] = useState<Card[]>([]); // Lưu cards ban đầu để so sánh
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const { languages } = useLanguageStore();
  const [otherMeanings, setOtherMeanings] = useState<Record<string, string>>({});

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

      setTitle(data.title);
      setDescription(data.description || '');

      // Load cards với đầy đủ thông tin
      const cardsData: Card[] = data.cards.map((card: any) => {
        // Extract 'vi' meaning or fallback
        const meanings = card.meanings || {};

        // Other meanings includes all meanings
        const otherMeanings = { ...meanings };
        // delete otherMeanings['vi']; // Don't remove 'vi', show all in list

        return {
          id: card.id,
          word: card.word,
          examples: card.examples || [],
          ipa: card.ipa || '',
          partOfSpeech: card.partOfSpeech || '',
          explain: card.explain || '',
          otherMeanings: otherMeanings,
        };
      });

      setCards(cardsData);
      setOriginalCards(JSON.parse(JSON.stringify(cardsData))); // Deep copy để so sánh
    } catch (error: any) {
      console.error('Failed to load lesson:', error);
      if (error.response?.status === 404) {
        alert(t('editLesson.notFound'));
        navigate('/lessons');
      } else {
        setError(t('editLesson.loadFailed'));
      }
    } finally {
      setLoading(false);
    }
  };

  // Form state cho thêm card thủ công
  const [cardForm, setCardForm] = useState<Card>({
    word: '',
    examples: [],
    ipa: '',
    partOfSpeech: '',
    explain: '',
    otherMeanings: {},
  });

  const handleAddCard = () => {
    if (!cardForm.word.trim() || !cardForm.explain?.trim()) {
      setError(t('createLesson.wordAndExplainRequired'));
      return;
    }

    const cardToAdd: Card = {
      ...cardForm,
      otherMeanings: Object.keys(otherMeanings).length > 0 ? { ...otherMeanings } : undefined,
    };

    if (editingIndex !== null) {
      // Edit card
      const newCards = [...cards];
      newCards[editingIndex] = cardToAdd;
      setCards(newCards);
      setEditingIndex(null);
    } else {
      // Add new card
      setCards([...cards, cardToAdd]);
    }

    // Reset form
    setCardForm({
      id: '',
      lessonId: '',
      word: '',
      examples: [],
      ipa: '',
      partOfSpeech: '',
      explain: '',
      otherMeanings: {},
    });
    setOtherMeanings({
      [(user?.nativeLanguage === 'vn' ? 'vi' : (user?.nativeLanguage || 'vi'))]: ''
    });
    setError('');
  };

  const handleEditCard = (index: number) => {
    const card = cards[index];
    setCardForm({ ...card });
    setOtherMeanings(card.otherMeanings || {});
    setEditingIndex(index);
    // Scroll to form
    document.getElementById('card-form')?.scrollIntoView({ behavior: 'smooth' });
  };

  const handleDeleteCard = (index: number) => {
    if (window.confirm(t('createLesson.deleteCardConfirm'))) {
      const newCards = cards.filter((_, i) => i !== index);
      setCards(newCards);
      if (editingIndex === index) {
        setEditingIndex(null);
        setCardForm({
          id: '',
          lessonId: '',
          word: '',
          examples: [],
          ipa: '',
          partOfSpeech: '',
          explain: '',
          otherMeanings: {},
        });
        setOtherMeanings({
          [(user?.nativeLanguage === 'vn' ? 'vi' : (user?.nativeLanguage || 'vi'))]: ''
        });
      }
    }
  };

  const handleImportCards = async (_importedTitle: string, importedCards: BaseCard[]) => {
    // Thêm cards vào danh sách hiện tại
    setCards([...cards, ...importedCards]);
    setShowImportDialog(false);
  };

  const handleSaveLesson = async () => {
    if (!title.trim()) {
      setError(t('createLesson.titleRequired'));
      return;
    }

    if (!title.trim()) {
      setError(t('createLesson.titleRequired'));
      return;
    }

    setIsSaving(true);
    setError('');

    try {
      // 1. Update lesson info
      await api.patch(`/lessons/${id}`, {
        title: title.trim(),
        description: description.trim() || undefined,
      });

      // 2. Xử lý cards
      // Tìm cards cần xóa (có id trong original nhưng không còn trong cards hiện tại)
      const currentCardIds = new Set(cards.filter(c => c.id).map(c => c.id));
      const cardsToDelete = originalCards.filter(c => c.id && !currentCardIds.has(c.id));

      // Xóa cards
      for (const card of cardsToDelete) {
        if (card.id) {
          await api.delete(`/lessons/${id}/cards/${card.id}`);
        }
      }

      // Tìm cards cần update (có id và đã thay đổi)
      const cardsToUpdate: Array<{ card: Card; original: Card }> = [];
      for (const card of cards) {
        if (card.id) {
          const original = originalCards.find(c => c.id === card.id);
          if (original) {
            // So sánh để xem có thay đổi không
            const hasChanged =
              card.word !== original.word ||
              JSON.stringify(card.examples || []) !== JSON.stringify(original.examples || []) ||
              card.ipa !== original.ipa ||
              card.partOfSpeech !== original.partOfSpeech ||
              card.explain !== original.explain ||
              JSON.stringify(card.otherMeanings || {}) !== JSON.stringify(original.otherMeanings || {});

            if (hasChanged) {
              cardsToUpdate.push({ card, original });
            }
          }
        }
      }

      // Update cards
      for (const { card } of cardsToUpdate) {
        if (card.id) {
          await api.patch(`/lessons/${id}/cards/${card.id}`, {
            word: card.word.trim(),
            meanings: {
              ...(card.otherMeanings || {}),
            },
            examples: card.examples?.filter(e => e.trim()) || [],
            ipa: card.ipa?.trim() || undefined,
            partOfSpeech: card.partOfSpeech?.trim() || undefined,
            explain: card.explain?.trim() || undefined,
          });
        }
      }

      // Tìm cards mới (không có id)
      const cardsToCreate = cards.filter(c => !c.id);

      // Tạo cards mới
      for (const card of cardsToCreate) {
        await api.post(`/lessons/${id}/cards`, {
          word: card.word.trim(),
          meanings: {
            ...(card.otherMeanings || {}),
          },
          examples: card.examples?.filter(e => e.trim()) || [],
          ipa: card.ipa?.trim() || undefined,
          partOfSpeech: card.partOfSpeech?.trim() || undefined,
          explain: card.explain?.trim() || undefined,
        });
      }

      // Navigate back to manage lessons
      navigate('/lessons');
    } catch (err: any) {
      console.error('Failed to update lesson:', err);
      setError(err.response?.data?.message || t('editLesson.updateFailed'));
    } finally {
      setIsSaving(false);
    }
  };

  const handleCancel = () => {
    if (!window.confirm(t('editLesson.cancelConfirm'))) {
      return;
    }
    navigate('/lessons');
  };

  if (loading) {
    return <div className="edit-lesson-loading">{t('common.loading')}</div>;
  }

  return (
    <div className="edit-lesson">
      <SEO title={t('seo.editLesson', { title })} />
      <div className="k-page-head">
        <div>
          <h1>{t('editLesson.title')}</h1>
          <p>{t('editLesson.subtitle')}</p>
        </div>
        <button onClick={handleCancel} className="k-btn k-btn--ghost">
          <X size={16} />
          {t('common.cancel')}
        </button>
      </div>

      <div className="edit-lesson-content">
        {/* Lesson Info */}
        <div className="lesson-info-section k-card">
          <div className="form-group">
            <label>{t('createLesson.lessonTitle')}</label>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={t('createLesson.titlePlaceholder', 'Ex: TOEIC Vocabulary')}
              className="form-input"
            />
          </div>

          <div className="form-group">
            <label>{t('createLesson.description')}</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t('createLesson.descriptionPlaceholder', 'Description about this lesson...')}
              className="form-textarea"
              rows={3}
            />
          </div>

        </div>

        {/* Import Section */}
        <div className="import-section k-card">
          <button
            onClick={() => setShowImportDialog(true)}
            className="k-btn k-btn--ghost"
          >
            <Upload size={16} />
            {t('createLesson.importFromText')}
          </button>
          <p className="import-hint">
            {t('createLesson.importHint')}
          </p>
        </div>

        {/* Add Card Form */}
        <div className="card-form-section k-card" id="card-form">
          <h2>{editingIndex !== null ? t('createLesson.editCard') : t('createLesson.addCard')}</h2>

          <div className="card-form">
            <div className="form-row">
              <div className="form-group">
                <label>{t('createLesson.word')}</label>
                <input
                  type="text"
                  value={cardForm.word}
                  onChange={(e) => setCardForm({ ...cardForm, word: e.target.value })}
                  placeholder={t('createLesson.wordPlaceholder', 'Ex: Apple')}
                  className="form-input"
                />
              </div>

              <div className="form-group">
                <label>
                  {t('createLesson.explain')} <span>*</span>
                </label>
                <input
                  type="text"
                  value={cardForm.explain}
                  onChange={(e) => setCardForm({ ...cardForm, explain: e.target.value })}
                  placeholder={t('createLesson.explainPlaceholder', 'Ex: A round fruit with red or green skin')}
                  className="form-input"
                />
              </div>
            </div>

            <div className="form-row">
              {/* Meaning input removed */}

              <div className="form-group" style={{ flex: 1 }}>
                <label>{t('createLesson.example')}</label>
                <div className="examples-container">
                  {(cardForm.examples || []).map((ex, idx) => (
                    <div key={idx} className="example-input-row">
                      <input
                        type="text"
                        value={ex}
                        onChange={(e) => {
                          const newExamples = [...(cardForm.examples || [])];
                          newExamples[idx] = e.target.value;
                          setCardForm({ ...cardForm, examples: newExamples });
                        }}
                        placeholder={t('createLesson.examplePlaceholder', { index: idx + 1 })}
                        className="form-input"
                      />
                      <button
                        type="button"
                        onClick={() => {
                          const newExamples = (cardForm.examples || []).filter((_, i) => i !== idx);
                          setCardForm({ ...cardForm, examples: newExamples });
                        }}
                        className="btn-remove-example"
                        aria-label={t('createLesson.removeExample')}
                      >
                        <X size={16} />
                      </button>
                    </div>
                  ))}
                  <button
                    type="button"
                    onClick={() => setCardForm({ ...cardForm, examples: [...(cardForm.examples || []), ''] })}
                    className="btn-add-example"
                  >
                    <Plus size={16} /> {t('createLesson.addExample')}
                  </button>
                </div>
              </div>
            </div>

            <div className="form-row">
              <div className="form-row-inline">
                <div className="form-group">
                  <label>{t('createLesson.ipa')}</label>
                  <input
                    type="text"
                    value={cardForm.ipa}
                    onChange={(e) => setCardForm({ ...cardForm, ipa: e.target.value })}
                    placeholder={t('createLesson.ipaPlaceholder', 'Ex: /\'æp.əl/')}
                    className="form-input"
                  />
                </div>

                <div className="form-group">
                  <label>{t('createLesson.partOfSpeech')}</label>
                  <select
                    value={cardForm.partOfSpeech}
                    onChange={(e) => setCardForm({ ...cardForm, partOfSpeech: e.target.value })}
                    className="form-select"
                  >
                    <option value="">{t('createLesson.select')}</option>
                    <option value="noun">{t('createLesson.noun')}</option>
                    <option value="verb">{t('createLesson.verb')}</option>
                    <option value="adjective">{t('createLesson.adjective')}</option>
                    <option value="adverb">{t('createLesson.adverb')}</option>
                    <option value="preposition">{t('createLesson.preposition')}</option>
                    <option value="conjunction">{t('createLesson.conjunction')}</option>
                    <option value="pronoun">{t('createLesson.pronoun')}</option>
                    <option value="interjection">{t('createLesson.interjection')}</option>
                  </select>
                </div>
              </div>
            </div>

            {/* Nghĩa đa quốc gia */}
            <div className="form-group">
              <label>{t('createLesson.otherMeanings')}</label>
              <p className="form-hint">{t('createLesson.otherMeaningsHint')}</p>
              <div className="other-meanings-container">
                {Object.entries(otherMeanings).map(([langCode, meaning]) => {
                  // Find language by code
                  const language = languages.find(l => l.code === langCode);

                  return (
                    <div key={langCode} className="other-meaning-item">
                      <div className="other-meaning-header">
                        <span className="country-flag-name">
                          {language?.flag} {language?.name || langCode}
                        </span>
                        <button
                          type="button"
                          onClick={() => {
                            const newMeanings = { ...otherMeanings };
                            delete newMeanings[langCode];
                            setOtherMeanings(newMeanings);
                          }}
                          className="btn-remove-meaning"
                        >
                          <X size={16} />
                        </button>
                      </div>
                      <input
                        type="text"
                        value={meaning}
                        onChange={(e) => {
                          setOtherMeanings({
                            ...otherMeanings,
                            [langCode]: e.target.value,
                          });
                        }}
                        placeholder={t('createLesson.enterMeaning')}
                        className="form-input"
                      />
                    </div>
                  );
                })}
                <div className="add-other-meaning">
                  <select
                    value=""
                    onChange={(e) => {
                      if (e.target.value) {
                        const selectedLangCode = e.target.value;

                        setOtherMeanings({
                          ...otherMeanings,
                          [selectedLangCode]: '',
                        });
                        e.target.value = '';
                      }
                    }}
                    className="form-select"
                  >
                    <option value="">{t('createLesson.addCountryMeaning')}</option>
                    {languages
                      .filter(language => {
                        return !otherMeanings[language.code];
                      })
                      .map((language) => (
                        <option key={language.id} value={language.code}>
                          {language.flag} {language.name}
                        </option>
                      ))}
                  </select>
                </div>
              </div>
            </div>

            <div className="form-actions">
              {editingIndex !== null && (
                <button
                  onClick={() => {
                    setEditingIndex(null);
                    setCardForm({
                      word: '',

                      examples: [],
                      ipa: '',
                      partOfSpeech: '',
                      explain: '',
                      otherMeanings: {},
                    });
                    setOtherMeanings({});
                  }}
                  className="k-btn k-btn--quiet"
                >
                  {t('createLesson.cancelEdit')}
                </button>
              )}
              <button onClick={handleAddCard} className="k-btn k-btn--ghost">
                <Plus size={16} />
                {editingIndex !== null ? t('common.update') : t('createLesson.addCard')}
              </button>
            </div>
          </div>
        </div>

        {/* Cards List */}
        {cards.length > 0 && (
          <div className="cards-list-section k-card">
            <h2>{t('createLesson.cardsList', { count: cards.length })}</h2>
            <div className="cards-grid">
              {cards.map((card, index) => <CardItem
                key={index}
                card={card}
                index={index}
                onEdit={handleEditCard}
                onDelete={handleDeleteCard}
              />
              )}
            </div>
          </div>
        )}

        {error && (
          <div className="error-message">{error}</div>
        )}

        {/* Submit Section */}
        <div className="submit-section">
          <button
            onClick={handleSaveLesson}
            disabled={isSaving || !title.trim() || cards.length === 0}
            className="k-btn k-btn--primary"
          >
            {isSaving ? (
              <>{t('editLesson.saving')}</>
            ) : (
              <>
                <FileText size={18} />
                {t('editLesson.saveChanges', { count: cards.length })}
              </>
            )}
          </button>
        </div>
      </div>

      <ImportDialog
        isOpen={showImportDialog}
        onClose={() => setShowImportDialog(false)}
        onImport={handleImportCards}
        showTitle={false}
      />
    </div>
  );
}

