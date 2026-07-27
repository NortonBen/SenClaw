import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useThemeStore } from '@/store/themeStore';
import {
  Settings as SettingsIcon,
  Moon,
  Sun,
  Monitor,
  Languages,
  PencilRuler,
  ListMusic,
  ChevronRight,
} from 'lucide-react';
import './Settings.css';
import SEO from '@/components/common/SEO';

/** UI languages that ship with the app (both locale files are complete). */
const LANGUAGES = [
  { code: 'vi', key: 'settings.vietnamese' },
  { code: 'en', key: 'settings.english' },
] as const;

export default function Settings() {
  const { t, i18n } = useTranslation();
  const { theme, setTheme } = useThemeStore();
  const current = i18n.resolvedLanguage ?? i18n.language;

  return (
    <div className="settings">
      <SEO title={t('settings.title')} />
      <div className="k-page-head">
        <div>
          <h1>{t('settings.title')}</h1>
          <p>{t('settings.subtitle')}</p>
        </div>
      </div>

      <div className="settings-content">
        <section className="settings-section">
          <h2 className="settings-section__title">
            <SettingsIcon size={16} />
            {t('settings.interface')}
          </h2>
          <div className="settings-card k-card">
            <div className="settings-item">
              <div className="settings-item-info">
                <h3>{t('settings.theme')}</h3>
                <p className="settings-item-description">{t('settings.themeDescription')}</p>
              </div>
              <div className="theme-selector">
                <button
                  type="button"
                  className={`theme-btn ${theme === 'light' ? 'active' : ''}`}
                  onClick={() => setTheme('light')}
                  title={t('settings.light')}
                >
                  <Sun size={16} />
                  <span>{t('settings.light')}</span>
                </button>
                <button
                  type="button"
                  className={`theme-btn ${theme === 'dark' ? 'active' : ''}`}
                  onClick={() => setTheme('dark')}
                  title={t('settings.dark')}
                >
                  <Moon size={16} />
                  <span>{t('settings.dark')}</span>
                </button>
                <button
                  type="button"
                  className={`theme-btn ${theme === 'system' ? 'active' : ''}`}
                  onClick={() => setTheme('system')}
                  title={t('settings.system')}
                >
                  <Monitor size={16} />
                  <span>{t('settings.system')}</span>
                </button>
              </div>
            </div>

            {/* i18next persists the choice to localStorage itself. */}
            <div className="settings-item">
              <div className="settings-item-info">
                <h3>{t('settings.language')}</h3>
                <p className="settings-item-description">{t('settings.languageDescription')}</p>
              </div>
              <div className="theme-selector">
                {LANGUAGES.map((l) => (
                  <button
                    key={l.code}
                    type="button"
                    className={`theme-btn ${current === l.code ? 'active' : ''}`}
                    onClick={() => i18n.changeLanguage(l.code)}
                  >
                    <Languages size={16} />
                    <span>{t(l.key)}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* Content authoring used to be a separate CMS behind its own login; in a
            single-user local app it belongs here rather than in the daily nav. */}
        <section className="settings-section">
          <h2 className="settings-section__title">
            <PencilRuler size={16} />
            {t('adminEntry.section')}
          </h2>
          <p className="settings-section__lead">{t('adminEntry.description')}</p>
          <div className="settings-card settings-card--flush k-card">
            <Link to="/manage/grammar" className="settings-link">
              <span className="settings-link__icon" style={{ color: 'var(--accent-review)' }}>
                <PencilRuler size={18} />
              </span>
              <span className="settings-link__text">
                <strong>{t('adminEntry.grammar')}</strong>
                <span>{t('adminEntry.grammarDesc')}</span>
              </span>
              <ChevronRight size={18} />
            </Link>
            <Link to="/manage/dictation" className="settings-link">
              <span className="settings-link__icon" style={{ color: 'var(--accent-streak)' }}>
                <ListMusic size={18} />
              </span>
              <span className="settings-link__text">
                <strong>{t('adminEntry.dictation')}</strong>
                <span>{t('adminEntry.dictationDesc')}</span>
              </span>
              <ChevronRight size={18} />
            </Link>
          </div>
        </section>
      </div>
    </div>
  );
}
