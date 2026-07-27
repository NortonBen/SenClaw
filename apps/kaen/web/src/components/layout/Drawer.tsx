import React, { useEffect } from 'react';
import { Moon, Sun, X } from 'lucide-react';
import './Drawer.css';
import { useThemeStore } from '@/store/themeStore';

interface DrawerProps {
    isOpen: boolean;
    onClose: () => void;
    title?: string;
    children: React.ReactNode;
}

export default function Drawer({ isOpen, onClose, title, children }: DrawerProps) {
    const { theme, setTheme } = useThemeStore();

    // Prevent body scroll when drawer is open
    useEffect(() => {
        if (isOpen) {
            document.body.style.overflow = 'hidden';
        } else {
            document.body.style.overflow = 'unset';
        }
        return () => {
            document.body.style.overflow = 'unset';
        };
    }, [isOpen]);

    return (
        <>
            <div
                className={`drawer-overlay ${isOpen ? 'open' : ''}`}
                onClick={onClose}
                aria-hidden="true"
            />
            <div className={`drawer-content ${isOpen ? 'open' : ''}`}>
                <div className="drawer-header">
                    <span className="drawer-title">{title || 'Menu'}</span>
                    <div className="mobile-header-actions">
                        <button
                            className="theme-toggle-mobile"
                            onClick={() => {
                                const isDark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
                                setTheme(isDark ? 'light' : 'dark');
                            }}
                            aria-label="Toggle theme"
                        >
                            {theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches) ? (
                                <Sun size={20} />
                            ) : (
                                <Moon size={20} />
                            )}
                        </button>
                        <button
                            className="theme-toggle-mobile"
                            onClick={onClose}
                            aria-label="Close menu"
                        >
                            <X size={24} />
                        </button>
                    </div>
                </div>
                {children}
            </div>
        </>
    );
}
