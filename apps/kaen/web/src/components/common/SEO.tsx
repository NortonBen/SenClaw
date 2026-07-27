import { Helmet } from 'react-helmet-async';
import { useTranslation } from 'react-i18next';

interface SEOProps {
    title?: string;
    description?: string;
    keywords?: string;
    ogImage?: string;
    ogType?: 'website' | 'article';
    canonical?: string;
}

const SEO = ({
    title,
    description,
    keywords = "english learning, vocabulary, kaen, language learning, dictation, remember words, spaced repetition",
    ogImage = "https://kaen.bacnd.com/logo.png",
    ogType = "website",
    canonical,
}: SEOProps) => {
    const { t } = useTranslation();
    const siteTitle = "Kaen";
    const metaDescription = description || t('seo.siteDescription');
    const fullTitle = title ? `${title} | ${siteTitle}` : t('seo.siteTitle');
    const url = window.location.href;

    return (
        <Helmet>
            {/* Basic Meta Tags */}
            <title>{fullTitle}</title>
            <meta name="description" content={metaDescription} />
            <meta name="keywords" content={keywords} />
            {canonical && <link rel="canonical" href={canonical} />}

            {/* Open Graph / Facebook */}
            <meta property="og:type" content={ogType} />
            <meta property="og:url" content={url} />
            <meta property="og:title" content={fullTitle} />
            <meta property="og:description" content={metaDescription} />
            <meta property="og:image" content={ogImage} />

            {/* Twitter */}
            <meta property="twitter:card" content="summary_large_image" />
            <meta property="twitter:url" content={url} />
            <meta property="twitter:title" content={fullTitle} />
            <meta property="twitter:description" content={metaDescription} />
            <meta property="twitter:image" content={ogImage} />
        </Helmet>
    );
};

export default SEO;
