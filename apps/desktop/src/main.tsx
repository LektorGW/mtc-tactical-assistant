import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { Overlay } from './Overlay';
import './styles.css';
const overlay = new URLSearchParams(location.search).has('overlay');
document.body.classList.toggle('overlay-body', overlay);
document.documentElement.classList.toggle('overlay-html', overlay);
ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode>{overlay ? <Overlay /> : <App />}</React.StrictMode>);

