import React from 'react'
import ReactDOM from 'react-dom/client'
import { WorldWidget } from './widgets/WorldWidget'
import './widget-base.css'

const params = new URLSearchParams(window.location.search)
const theme = params.get('theme')
if (theme) document.documentElement.setAttribute('data-theme', theme)

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WorldWidget />
  </React.StrictMode>,
)
