import { createRoot } from 'react-dom/client'
import { FeedWidget } from './widgets/FeedWidget'
import './widget-base.css'

createRoot(document.getElementById('root')!).render(<FeedWidget />)
