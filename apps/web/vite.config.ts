import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    strictPort: true, // Ошибка если порт занят.
  },
  build: {
    // Две страницы: основной сайт и игра (game.html)
    rollupOptions: {
      input: { main: 'index.html', game: 'game.html' },
    },
  },
});
