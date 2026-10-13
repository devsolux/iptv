import { setupIonicReact } from '@ionic/react';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@ionic/react/css/core.css';
import '@ionic/react/css/normalize.css';
import '@ionic/react/css/structure.css';
import '@ionic/react/css/typography.css';
import './styles/app.css';
import App from './App';

setupIonicReact({ mode: 'md', animated: false, rippleEffect: false });
const root = document.getElementById('root');
if (!root) throw new Error('IPTV root element is missing.');
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
