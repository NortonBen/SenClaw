import React from 'react'
import ReactDOM from 'react-dom/client'
import { LunaAlmanacWidget } from './widgets/LunaAlmanacWidget'
import './widget-base.css'
const params = new URLSearchParams(window.location.search)
const t0 = params.get('theme'); if (t0) document.documentElement.setAttribute('data-theme', t0)
window.addEventListener('message', (e) => {
  const d: any = e.data
  if (d && (d.type === 'senclaw:init' || d.type === 'senclaw:theme')) {
    const th = d.theme || d.env?.theme
    if (th) document.documentElement.setAttribute('data-theme', th)
  }
})
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode><LunaAlmanacWidget /></React.StrictMode>,
)
window.parent.postMessage({ type: 'senclaw:ready' }, '*')
