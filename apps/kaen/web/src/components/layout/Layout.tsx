import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Menu, Flame, Sparkles, Settings, Sun, Moon, MonitorSmartphone } from 'lucide-react';
import Navigation from './Navigation';
import Logo from './logo';
import Drawer from './Drawer';
import SEO from '../common/SEO';
import { useThemeStore } from '@/store/themeStore';
import { useOverviewStore } from '@/store/overviewStore';
import './Layout.css';

interface LayoutProps {
  children: React.ReactNode;
}

/** Collapse the rail to icons before the content column gets squeezed. */
function useCompactRail() {
  const [compact, setCompact] = useState(
    () => typeof window !== 'undefined' && window.matchMedia('(max-width: 1180px)').matches
  );
  useEffect(() => {
    const mq = window.matchMedia('(max-width: 1180px)');
    const onChange = (e: MediaQueryListEvent) => setCompact(e.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);
  return compact;
}

const THEME_CYCLE = { system: 'light', light: 'dark', dark: 'system' } as const;
const THEME_ICON = { system: MonitorSmartphone, light: Sun, dark: Moon };
const THEME_KEY = { system: 'themeSystem', light: 'themeLight', dark: 'themeDark' } as const;

export default function Layout({ children }: LayoutProps) {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const compact = useCompactRail();
  const { theme, setTheme } = useThemeStore();
  const { data: overview, load } = useOverviewStore();
  const { t } = useTranslation();

  // One fetch feeds the rail badges, the mobile top bar and the dashboard.
  useEffect(() => {
    load();
  }, [load]);

  const ThemeIcon = THEME_ICON[theme];
  const themeLabel = t(`shell.${THEME_KEY[theme]}`);

  return (
    <div className="app-shell">
      <SEO />

      <aside className={`sidebar${compact ? ' sidebar--compact' : ''}`}>
        <Link to="/" className="brand" title="Kaen">
          <span className="brand__mark">
            <Logo />
          </span>
          <span className="brand__text">
            <strong>Kaen</strong>
            <em>{t('shell.tagline')}</em>
          </span>
        </Link>

        <div className="sidebar__scroll">
          <Navigation />
        </div>

        <div className="sidebar__foot">
          <div className="sidebar__stats">
            <span className="stat-pill" title={t('shell.streakTitle')}>
              <Flame size={14} />
              <b className="k-num">{overview?.currentStreak ?? 0}</b>
            </span>
            <span className="stat-pill" title={t('shell.xpTitle')}>
              <Sparkles size={14} />
              <b className="k-num">{overview?.totalXP ?? 0}</b>
            </span>
          </div>
          <div className="sidebar__tools">
            <button
              type="button"
              className="icon-btn"
              onClick={() => setTheme(THEME_CYCLE[theme])}
              title={`${t('shell.themeLabel')}: ${themeLabel}`}
              aria-label={t('shell.changeTheme')}
            >
              <ThemeIcon size={17} />
            </button>
            <Link to="/settings" className="icon-btn" title={t('shell.settings')} aria-label={t('shell.settings')}>
              <Settings size={17} />
            </Link>
          </div>
        </div>
      </aside>

      <div className="app-main">
        <header className="topbar">
          <button className="icon-btn" onClick={() => setMobileMenuOpen(true)} aria-label={t('shell.openMenu')}>
            <Menu size={20} />
          </button>
          <Link to="/" className="topbar__brand">
            <span className="brand__mark">
              <Logo />
            </span>
            <strong>Kaen</strong>
          </Link>
          {!!overview?.dueNow && (
            <Link to="/review" className="topbar__due">
              {t('shell.dueWords', { count: overview.dueNow })}
            </Link>
          )}
        </header>

        <main className="main-content">{children}</main>
      </div>

      <Drawer isOpen={mobileMenuOpen} onClose={() => setMobileMenuOpen(false)} title="Kaen">
        <div className="drawer-content-inner">
          <Navigation onItemClick={() => setMobileMenuOpen(false)} />
          {/* The drawer header already carries a theme toggle, so only the
              settings link belongs here. */}
          <div className="drawer-tools">
            <Link
              to="/settings"
              className="k-btn k-btn--ghost"
              onClick={() => setMobileMenuOpen(false)}
            >
              <Settings size={16} /> {t('shell.settings')}
            </Link>
          </div>
        </div>
      </Drawer>
    </div>
  );
}
