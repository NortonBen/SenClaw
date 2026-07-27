import { Link, useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
    LayoutDashboard,
    BookOpen,
    RotateCcw,
    Library,
    FileText,
    BookMarked,
    Headphones,
} from 'lucide-react';
import { useOverviewStore } from '@/store/overviewStore';
import './Navigation.css';

interface NavigationProps {
    onItemClick?: () => void;
}

type Item = {
    path: string;
    /** i18n key under `nav.` */
    label: string;
    icon: typeof BookOpen;
    /** Extra path prefixes that belong to this section. */
    also?: string[];
    badge?: 'due' | 'grammar';
};

/** Seven sections is too many for one flat row — group them by what the user
 *  is trying to do, so the rail reads as three short lists. Content authoring
 *  is not part of the daily loop, so it lives in Settings instead. */
const GROUPS: { caption?: string; items: Item[] }[] = [
    {
        items: [{ path: '/', label: 'today', icon: LayoutDashboard }],
    },
    {
        caption: 'vocabulary',
        items: [
            { path: '/lessons', label: 'lessons', icon: BookOpen, also: ['/library', '/study'] },
            {
                path: '/review',
                label: 'review',
                icon: RotateCcw,
                badge: 'due',
                also: ['/listening', '/writing', '/matching'],
            },
            { path: '/bank', label: 'bank', icon: Library, also: ['/learned'] },
        ],
    },
    {
        caption: 'practiceMore',
        items: [
            {
                path: '/grammar',
                label: 'grammar',
                icon: FileText,
                badge: 'grammar',
                also: ['/grammar-tests'],
            },
            { path: '/stories', label: 'stories', icon: BookMarked },
            {
                path: '/dictation',
                label: 'dictation',
                icon: Headphones,
                also: ['/dictation-history'],
            },
        ],
    },
];

export default function Navigation({ onItemClick }: NavigationProps) {
    const { pathname } = useLocation();
    const { t } = useTranslation();
    const overview = useOverviewStore((s) => s.data);

    const isActive = (item: Item) =>
        item.path === '/'
            ? pathname === '/'
            : pathname.startsWith(item.path) || (item.also ?? []).some((p) => pathname.startsWith(p));

    const badgeValue = (kind?: Item['badge']) => {
        if (!overview || !kind) return 0;
        return kind === 'due' ? overview.dueNow : overview.library.grammarDue;
    };

    return (
        <nav className="k-nav">
            {GROUPS.map((group, i) => (
                <div className="k-nav__group" key={group.caption ?? `g${i}`}>
                    {group.caption && <p className="k-nav__caption">{t(`nav.${group.caption}`)}</p>}
                    {group.items.map((item) => {
                        const count = badgeValue(item.badge);
                        return (
                            <Link
                                key={item.path}
                                to={item.path}
                                onClick={onItemClick}
                                title={t(`nav.${item.label}`)}
                                className={`k-nav__item${isActive(item) ? ' is-active' : ''}`}
                            >
                                <item.icon size={19} strokeWidth={2} />
                                <span className="k-nav__label">{t(`nav.${item.label}`)}</span>
                                {count > 0 && (
                                    <span
                                        className={`k-nav__badge${
                                            item.badge === 'due' ? ' k-nav__badge--due' : ''
                                        }`}
                                    >
                                        {count > 99 ? '99+' : count}
                                    </span>
                                )}
                            </Link>
                        );
                    })}
                </div>
            ))}
        </nav>
    );
}
