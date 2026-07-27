import { useEffect, useState } from 'react';
import { useParams } from 'react-router-dom';
import { DictationLessonDetail, dictationApi } from '../../lib/dictationApi';
import DictationListener from '../../components/dictation/DictationListener';
import { Loader2 } from 'lucide-react';
import SEO from '../../components/common/SEO';

const DictationListenPage = () => {
    const { id } = useParams<{ id: string }>();
    const [lesson, setLesson] = useState<DictationLessonDetail | null>(null);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        const fetchLesson = async () => {
            if (!id) return;
            try {
                const data = await dictationApi.getLesson(+id);
                setLesson(data);
            } catch (error) {
                console.error('Failed to fetch lesson detail', error);
            } finally {
                setLoading(false);
            }
        };
        fetchLesson();
    }, [id]);

    if (loading) {
        return (
            <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100vh', flexDirection: 'column', gap: '1rem' }}>
                <Loader2 className="animate-spin" size={40} />
                <p>Loading lesson...</p>
            </div>
        );
    }

    if (!lesson) {
        return (
            <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100vh' }}>
                Lesson not found
            </div>
        );
    }

    return (
        <div style={{ paddingBottom: '2rem' }}>
            <SEO title={`Dictation Listening: ${lesson.title}`} />
            <DictationListener lesson={lesson} />
        </div>
    );
};

export default DictationListenPage;
