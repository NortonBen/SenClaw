import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  BookOpen,
  Clock,
  Target,
  TrendingUp,
  Lightbulb,
  CheckCircle,
  RotateCcw,
  Headphones,
  PenTool,
  Link2,
  Sparkles,
} from 'lucide-react';
import { LEVEL_GUIDE } from '@/constants/levelGuide';
import './StudyGuide.css';
import SEO from '@/components/common/SEO';

export default function StudyGuide() {
  const { t } = useTranslation();

  const highlightCards = [
    {
      icon: Clock,
      title: t('studyGuide.pillar1Title'),
      description: t('studyGuide.pillar1Description'),
    },
    {
      icon: Target,
      title: t('studyGuide.pillar2Title'),
      description: t('studyGuide.pillar2Description'),
    },
    {
      icon: TrendingUp,
      title: t('studyGuide.pillar3Title'),
      description: t('studyGuide.pillar3Description'),
    },
  ];

  const quickSteps = [
    {
      title: t('studyGuide.step1Time'),
      label: t('studyGuide.step1Label'),
      description: t('studyGuide.step1Description'),
      tip: t('studyGuide.step1Tip'),
    },
    {
      title: t('studyGuide.step2Time'),
      label: t('studyGuide.step2Label'),
      description: t('studyGuide.step2Description'),
      tip: t('studyGuide.step2Tip'),
    },
    {
      title: t('studyGuide.step3Time'),
      label: t('studyGuide.step3Label'),
      description: t('studyGuide.step3Description'),
      tip: t('studyGuide.step3Tip'),
    },
  ];

  const practiceFeatures = [
    {
      to: '/review',
      icon: RotateCcw,
      title: t('studyGuide.practice1Title'),
      description: t('studyGuide.practice1Description'),
    },
    {
      to: '/listening',
      icon: Headphones,
      title: t('studyGuide.practice2Title'),
      description: t('studyGuide.practice2Description'),
    },
    {
      to: '/writing',
      icon: PenTool,
      title: t('studyGuide.practice3Title'),
      description: t('studyGuide.practice3Description'),
    },
    {
      to: '/matching',
      icon: Link2,
      title: t('studyGuide.practice4Title'),
      description: t('studyGuide.practice4Description'),
    },
  ];

  const studyTips = [
    t('studyGuide.tip1'),
    t('studyGuide.tip2'),
    t('studyGuide.tip3'),
    t('studyGuide.tip4'),
    t('studyGuide.tip5'),
    t('studyGuide.tip6'),
  ];
  return (
    <div className="study-guide">
      <SEO title="Study Guide - How to Learn Effectively" />
      <header className="guide-header">
        <p className="guide-pill">{t('studyGuide.pill')}</p>
        <h1>{t('studyGuide.title')}</h1>
        <p className="guide-subtitle">
          {t('studyGuide.subtitle')}
        </p>
        <div className="guide-header-actions">
          <Link to="/study" className="btn-primary">
            <BookOpen size={18} />
            {t('studyGuide.startLearning')}
          </Link>
          <Link to="/review" className="btn-secondary">
            <RotateCcw size={18} />
            {t('studyGuide.reviewNow')}
          </Link>
        </div>
      </header>

      <div className="guide-content">
        <section className="guide-section">
          <div className="section-header">
            <Sparkles size={26} />
            <div>
              <h2>{t('studyGuide.pillarsTitle')}</h2>
              <p className="section-description">{t('studyGuide.pillarsDescription')}</p>
            </div>
          </div>
          <div className="pillars-grid">
            {highlightCards.map(({ icon: Icon, title, description }) => (
              <div key={title} className="pillar-card">
                <div className="pillar-icon">
                  <Icon size={22} />
                </div>
                <div>
                  <h3>{title}</h3>
                  <p>{description}</p>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="guide-section">
          <div className="section-header">
            <BookOpen size={26} />
            <div>
              <h2>{t('studyGuide.timelineTitle')}</h2>
              <p className="section-description">{t('studyGuide.timelineDescription')}</p>
            </div>
          </div>
          <div className="quick-steps">
            {quickSteps.map((step, index) => (
              <div key={step.label} className="step-card">
                <div className="step-number">{index + 1}</div>
                <div className="step-body">
                  <span className="step-label">{step.title}</span>
                  <h3>{step.label}</h3>
                  <p>{step.description}</p>
                  <div className="step-tip">
                    <Lightbulb size={16} />
                    <span>{step.tip}</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="guide-section">
          <div className="section-header">
            <RotateCcw size={26} />
            <div>
              <h2>{t('studyGuide.levelTitle')}</h2>
              <p className="section-description">
                {t('studyGuide.levelDescription')}
              </p>
            </div>
          </div>
          <div className="level-layout">
            <div className="level-timeline">
              {LEVEL_GUIDE.map((level, index) => (
                <div key={level.key} className="timeline-item">
                  <div className="timeline-dot" style={{ backgroundColor: level.color }} />
                  <div className="timeline-content">
                    <div className="timeline-heading">
                      <span className="timeline-index">{index + 1}</span>
                      <span className="timeline-title">{t(`studyGuide.level.${level.key}.label`)}</span>
                      <span className="timeline-interval">{t(`studyGuide.level.${level.key}.interval`)}</span>
                    </div>
                    <p>{t(`studyGuide.level.${level.key}.description`)}</p>
                  </div>
                </div>
              ))}
            </div>
          </div>
          <div className="info-box">
            <CheckCircle size={20} />
            <div>
              <strong>{t('studyGuide.remember')}:</strong> {t('studyGuide.rememberText')}
            </div>
          </div>
        </section>

        <section className="guide-section">
          <div className="section-header">
            <Target size={26} />
            <div>
              <h2>{t('studyGuide.practiceTitle')}</h2>
              <p className="section-description">{t('studyGuide.practiceDescription')}</p>
            </div>
          </div>
          <div className="practice-grid">
            {practiceFeatures.map(({ to, icon: Icon, title, description }) => (
              <Link key={title} to={to} className="practice-card">
                <div className="practice-icon">
                  <Icon size={24} />
                </div>
                <div>
                  <h3>{title}</h3>
                  <p>{description}</p>
                </div>
              </Link>
            ))}
          </div>
        </section>

        <section className="guide-section">
          <div className="section-header">
            <Lightbulb size={26} />
            <div>
              <h2>{t('studyGuide.tipsTitle')}</h2>
              <p className="section-description">{t('studyGuide.tipsDescription')}</p>
            </div>
          </div>
          <div className="tips-grid">
            {studyTips.map((tip) => (
              <div key={tip} className="tip-card">
                <CheckCircle size={18} />
                <p>{tip}</p>
              </div>
            ))}
          </div>
        </section>

        <section className="guide-section guide-cta">
          <div className="cta-content">
            <h2>{t('studyGuide.ctaTitle')}</h2>
            <p>
              {t('studyGuide.ctaDescription')}
            </p>
            <div className="cta-actions">
              <Link to="/study" className="cta-button primary">
                <BookOpen size={18} />
                {t('studyGuide.startLearning')}
              </Link>
              <Link to="/review" className="cta-button ghost">
                <RotateCcw size={18} />
                {t('studyGuide.startReview')}
              </Link>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

