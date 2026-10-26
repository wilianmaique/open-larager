import React from 'react';
import ReactDOM from 'react-dom/client';
import { AppProvider } from './state';
import App from './App';
import './styles.css';
import './controls.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <AppProvider>
      <App />
    </AppProvider>
  </React.StrictMode>,
);
