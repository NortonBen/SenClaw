import { createRoot } from 'react-dom/client'
import { DraftsWidget } from './widgets/DraftsWidget'
import './widget-base.css'

createRoot(document.getElementById('root')!).render(<DraftsWidget />)
