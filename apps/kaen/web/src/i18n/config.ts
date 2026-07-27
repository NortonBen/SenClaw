import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

import enTranslations from './locales/en.json';
import viTranslations from './locales/vi.json';

// Kaen là app học tiếng Anh cho người Việt: giao diện mặc định tiếng Việt,
// chỉ nội dung học (từ vựng, ví dụ) mới là tiếng Anh. Trước đây mặc định 'en'
// khiến vỏ app (sidebar, nút) tiếng Việt còn từng trang lại tiếng Anh.
const getStoredLanguage = (): string => {
  const stored = localStorage.getItem('i18nextLng');
  return stored === 'vi' || stored === 'en' ? stored : 'vi';
};

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: {
        translation: enTranslations,
      },
      vi: {
        translation: viTranslations,
      },
    },
    fallbackLng: 'vi',
    lng: getStoredLanguage(),
    detection: {
      order: ['localStorage'],
      caches: ['localStorage'],
      lookupLocalStorage: 'i18nextLng',
    },
    interpolation: {
      escapeValue: false, // React đã tự động escape
    },
  });

export default i18n;

