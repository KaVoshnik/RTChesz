import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  build: {
    // Две страницы: основной сайт и игра (game.html)
    rollupOptions: {
      input: { main: 'index.html', game: 'game.html' },
    },
  },
});
