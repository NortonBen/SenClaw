import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuthStore } from '@/store/authStore';
import { useLanguageStore } from '@/store/languageStore';
import api from '@/lib/api';
import { BarChart3, Flame, Sparkles, Trophy, GraduationCap } from 'lucide-react';
import { LEVEL_GUIDE } from '@/constants/levelGuide';
import './Profile.css';
import SEO from '@/components/common/SEO';

interface VocabularyStatistics {
  byLevel: {
    level0: number;
    level1: number;
    level2: number;
    level3: number;
    level4: number;
    level5: number;
    level6Plus: number;
  };
  totalWords: number;
  totalLearned: number;
  newWords: number;
  detailed: Record<number, number>;
}

const PROFILE_LEVELS = LEVEL_GUIDE.filter((level) => level.showInProfile);

export default function Profile() {
  const { t } = useTranslation();
  const { user } = useAuthStore();
  const [vocabStats, setVocabStats] = useState<VocabularyStatistics | null>(null);
  const [loadingStats, setLoadingStats] = useState(false);

  useEffect(() => {
    const fetchVocabStats = async () => {
      setLoadingStats(true);
      try {
        const response = await api.get('/study/statistics/level');
        setVocabStats(response.data);
      } catch (error) {
        console.error('Failed to fetch vocabulary statistics:', error);
      } finally {
        setLoadingStats(false);
      }
    };

    fetchVocabStats();
  }, []);

  // Fetch languages to display correct name
  const { languages, fetchLanguages, getLanguageByCode } = useLanguageStore();

  useEffect(() => {
    if (languages.length === 0) {
      fetchLanguages();
    }
  }, [languages.length, fetchLanguages]);

  const getCountryName = (code: string) => {
    const language = getLanguageByCode(code);
    if (language) {
      return `${language.flag} ${language.name}`;
    }
    // Fallback if languages not loaded yet or code not found
    const flagMap: Record<string, string> = {
      vi: '🇻🇳',
      en: '🇺🇸',
      ja: '🇯🇵',
      ko: '🇰🇷',
      zh: '🇨🇳',
      fr: '🇫🇷',
      de: '🇩🇪',
      es: '🇪🇸',
    };
    const flag = flagMap[code];
    if (!flag) return code;
    return `${flag} ${t(`profile.languageNames.${code}`)}`;
  };

  return (
    <div className="profile">
      <SEO title={t('seo.profile')} description={t('seo.profileDesc')} />
      <div className="k-page-head">
        <div>
          <h1>{t('profile.myProfile')}</h1>
          <p>{t('profile.subtitle')}</p>
        </div>
      </div>

      <div className="profile-stats">
        <div className="profile-stat k-card">
          <Flame size={17} className="profile-stat__icon is-streak" />
          <div className="profile-stat__value k-num">{user?.currentStreak || 0}</div>
          <div className="profile-stat__label">{t('profile.consecutiveDays')}</div>
        </div>
        <div className="profile-stat k-card">
          <Trophy size={17} className="profile-stat__icon is-xp" />
          <div className="profile-stat__value k-num">{user?.totalXP || 0}</div>
          <div className="profile-stat__label">{t('profile.totalXP')}</div>
        </div>
        {vocabStats && (
          <>
            <div className="profile-stat k-card">
              <GraduationCap size={17} className="profile-stat__icon is-learned" />
              <div className="profile-stat__value k-num">{vocabStats.totalLearned}</div>
              <div className="profile-stat__label">{t('profile.wordsLearned')}</div>
            </div>
            <div className="profile-stat k-card">
              <Sparkles size={17} className="profile-stat__icon is-new" />
              <div className="profile-stat__value k-num">{vocabStats.newWords}</div>
              <div className="profile-stat__label">{t('profile.newWords')}</div>
            </div>
          </>
        )}
      </div>

      {vocabStats && (
        <section className="profile-section k-card">
          <div className="profile-section__head">
            <BarChart3 size={18} />
            <h2>{t('profile.vocabStatsByLevel')}</h2>
          </div>

          <div className="profile-summary">
            <div className="profile-summary__item">
              <span className="profile-summary__label">{t('profile.totalWords')}</span>
              <span className="profile-summary__value k-num">{vocabStats.totalWords}</span>
            </div>
            <div className="profile-summary__item">
              <span className="profile-summary__label">{t('profile.learned')}</span>
              <span className="profile-summary__value k-num">{vocabStats.totalLearned}</span>
            </div>
            <div className="profile-summary__item">
              <span className="profile-summary__label">{t('profile.newWordsLabel')}</span>
              <span className="profile-summary__value k-num">{vocabStats.newWords}</span>
            </div>
          </div>

          <div className="level-chart">
            {PROFILE_LEVELS.map((level) => {
              const levelKey = level.key as keyof typeof vocabStats.byLevel;
              const value = vocabStats.byLevel[levelKey];
              const maxValue = Math.max(
                ...PROFILE_LEVELS.map((profileLevel) => {
                  const key = profileLevel.key as keyof typeof vocabStats.byLevel;
                  return vocabStats.byLevel[key];
                }),
                0
              );
              const percentage = maxValue > 0 ? (value / maxValue) * 100 : 0;

              return (
                <div key={level.key} className="level-bar-item">
                  <div className="level-bar-header">
                    <span className="level-label">{t(`profile.level.${level.key}.label`)}</span>
                    <span className="level-value k-num">{value}</span>
                  </div>
                  <div className="level-bar-container">
                    <div
                      className="level-bar-fill"
                      style={{
                        width: `${percentage}%`,
                        backgroundColor: level.color,
                      }}
                    />
                  </div>
                  <div className="level-tooltip k-card" role="tooltip">
                    <p className="level-tooltip-title">{t(`profile.level.${level.key}.label`)}</p>
                    <span className="level-tooltip-interval">{t(`profile.level.${level.key}.interval`)}</span>
                    <p className="level-tooltip-description">{t(`profile.level.${level.key}.description`)}</p>
                  </div>
                </div>
              );
            })}
          </div>

        </section>
      )}

      {loadingStats && (
        <div className="loading-stats">
          <p>{t('profile.loadingStats')}</p>
        </div>
      )}

      <section className="profile-section k-card">
        <div className="profile-section__head">
          <h2>{t('profile.personalInfo')}</h2>
        </div>
        <div className="profile-info-card">
          <div className="info-item">
            <label>{t('profile.email')}</label>
            <div className="info-value">{user?.email || t('profile.notSet')}</div>
          </div>
          <div className="info-item">
            <label>{t('profile.username')}</label>
            <div className="info-value">{user?.username || t('profile.notSet')}</div>
          </div>
          <div className="info-item">
            <label>{t('profile.fullName')}</label>
            <div className="info-value">{user?.fullName || t('profile.notSet')}</div>
          </div>
          <div className="info-item">
            <label>{t('profile.bio')}</label>
            <div className="info-value">{user?.bio || t('profile.notSet')}</div>
          </div>
          <div className="info-item">
            <label>{t('profile.nativeLanguage')}</label>
            <div className="info-value">
              {user?.nativeLanguage ? getCountryName(user.nativeLanguage) : t('profile.notSet')}
            </div>
          </div>
          <div className="info-item">
            <label>{t('profile.studySlots')}</label>
            <div className="info-value">
              {user?.studySlots && (user.studySlots as string[]).length > 0
                ? (user.studySlots as string[]).map((slot, index) => (
                  <span key={index} className="k-chip k-num">
                    {slot}
                  </span>
                ))
                : t('profile.notSet')}
            </div>
          </div>
          <div className="info-item">
            <label>{t('profile.dailyWordGoal')}</label>
            <div className="info-value">
              <span className="k-num">{user?.dailyWordGoal || 10}</span>&nbsp;{t('profile.words')}
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

