import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { host: process.env.TAURI_DEV_HOST ?? '127.0.0.1', port: 1420, strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] } },
  envPrefix: ['VITE_'],
  build: { target: ['chrome111', 'safari16.4', 'firefox128'], sourcemap: false },
});
