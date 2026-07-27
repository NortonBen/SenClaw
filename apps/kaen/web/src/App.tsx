import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { HelmetProvider } from 'react-helmet-async';
import { Toaster } from 'sonner';
import { useAuthStore } from './store/authStore';
import { useThemeStore } from './store/themeStore';
import { useEffect } from 'react';
import Home from './pages/home/Home';
import Study from './pages/practice/Study';
import StudyLesson from './pages/lesson/StudyLesson';
import Review from './pages/practice/Review';
import ReviewLesson from './pages/lesson/ReviewLesson';
import ListeningPractice from './pages/practice/ListeningPractice';
import WritingPractice from './pages/practice/WritingPractice';
import MatchingPractice from './pages/practice/MatchingPractice';
import BankWord from './pages/bank/BankWord';
import Profile from './pages/profile/Profile';
import CreateLesson from './pages/lesson/CreateLesson';
import EditLesson from './pages/lesson/EditLesson';
import ManageLessons from './pages/lesson/ManageLessons';
import LessonDetailBank from './pages/lesson/LessonDetailBank';
import LessonDetailLibrary from './pages/lesson/LessonDetailLibrary';
import Layout from './components/layout/Layout';
import LearnedCards from './pages/practice/LearnedCards';
import GrammarListPage from './pages/grammar/GrammarListPage';
import GrammarDetailPage from './pages/grammar/GrammarDetailPage';
import GrammarTestTopicsPage from './pages/grammar-test/GrammarTestTopicsPage';
import GrammarTestSessionPage from './pages/grammar-test/GrammarTestSessionPage';
import GrammarTestResultPage from './pages/grammar-test/GrammarTestResultPage';
import GenerateAITestPage from './pages/grammar-test/GenerateAITestPage';
import SpacedRepetition from './pages/practice/SpacedRepetition';
import StudyGuide from './pages/practice/StudyGuide';
import Settings from './pages/profile/Settings';
import ManageStories from './pages/story/ManageStories';
import CreateStory from './pages/story/CreateStory';
import EditStory from './pages/story/EditStory';
import StoryDetail from './pages/story/StoryDetail';
import DictationTopicsPage from './pages/dictation/DictationTopicsPage';
import DictationLessonListPage from './pages/dictation/DictationLessonListPage';
import DictationPracticePage from './pages/dictation/DictationPracticePage';
import DictationListenPage from './pages/dictation/DictationListenPage';
import DictationHistoryPage from './pages/dictation/DictationHistoryPage';
import ManageGrammar from './pages/manage/ManageGrammar';
import ManageDictation from './pages/manage/ManageDictation';

function App() {
  const { fetchProfile } = useAuthStore();
  const { theme } = useThemeStore();

  useEffect(() => {
    fetchProfile();
  }, [fetchProfile]);

  // Initial theme application
  useEffect(() => {
    useThemeStore.getState().setTheme(theme);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <HelmetProvider>
      <Toaster position="top-center" richColors />
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<Layout><Home /></Layout>} />
          <Route path="/lessons" element={<Layout><ManageLessons /></Layout>} />
          <Route path="/lessons/create" element={<Layout><CreateLesson /></Layout>} />
          <Route path="/lessons/:id/edit" element={<Layout><EditLesson /></Layout>} />
          <Route path="/lessons/:id" element={<Layout><LessonDetailLibrary /></Layout>} />
          <Route path="/library/lessons/:id" element={<Layout><LessonDetailLibrary /></Layout>} />
          <Route path="/bank" element={<Layout><BankWord /></Layout>} />
          <Route path="/bank/lessons/:id" element={<Layout><LessonDetailBank /></Layout>} />
          <Route path="/study" element={<Layout><Study /></Layout>} />
          <Route path="/study/lesson/:id" element={<Layout><StudyLesson /></Layout>} />
          <Route path="/study/review/:id" element={<Layout><ReviewLesson /></Layout>} />
          <Route path="/review" element={<Layout><Review /></Layout>} />
          <Route path="/listening" element={<Layout><ListeningPractice /></Layout>} />
          <Route path="/writing" element={<Layout><WritingPractice /></Layout>} />
          <Route path="/matching" element={<Layout><MatchingPractice /></Layout>} />
          <Route path="/learned" element={<Layout><LearnedCards /></Layout>} />
          <Route path="/grammar" element={<Layout><GrammarListPage /></Layout>} />
          <Route path="/grammar/:slug" element={<Layout><GrammarDetailPage /></Layout>} />
          <Route path="/grammar-tests" element={<Layout><GrammarTestTopicsPage /></Layout>} />
          <Route path="/grammar-tests/generate" element={<Layout><GenerateAITestPage /></Layout>} />
          <Route path="/grammar-tests/results/:sessionId" element={<Layout><GrammarTestResultPage /></Layout>} />
          <Route path="/grammar-tests/:topicId" element={<Layout><GrammarTestSessionPage /></Layout>} />
          <Route path="/spaced-repetition/:reviewNotificationId" element={<Layout><SpacedRepetition /></Layout>} />
          <Route path="/stories" element={<Layout><ManageStories /></Layout>} />
          <Route path="/stories/create" element={<Layout><CreateStory /></Layout>} />
          <Route path="/stories/:id/edit" element={<Layout><EditStory /></Layout>} />
          <Route path="/stories/:id" element={<Layout><StoryDetail /></Layout>} />
          <Route path="/dictation" element={<Layout><DictationTopicsPage /></Layout>} />
          <Route path="/dictation/practice/:id" element={<Layout><DictationPracticePage /></Layout>} />
          <Route path="/dictation/listen/:id" element={<Layout><DictationListenPage /></Layout>} />
          <Route path="/dictation/:topic" element={<Layout><DictationLessonListPage /></Layout>} />
          <Route path="/dictation-history" element={<Layout><DictationHistoryPage /></Layout>} />
          <Route path="/guide" element={<Layout><StudyGuide /></Layout>} />
          <Route path="/profile" element={<Layout><Profile /></Layout>} />
          <Route path="/settings" element={<Layout><Settings /></Layout>} />
          {/* Quản trị nội dung — gộp từ CMS riêng của kaizen, không cần đăng nhập */}
          <Route path="/manage/grammar" element={<Layout><ManageGrammar /></Layout>} />
          <Route path="/manage/dictation" element={<Layout><ManageDictation /></Layout>} />
        </Routes>
      </BrowserRouter>
    </HelmetProvider>
  );
}

export default App;
