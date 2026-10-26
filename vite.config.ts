import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'es2022',
    sourcemap: false,
    rolldownOptions: {
      output: {
        // Keep Monaco lazy while splitting its implementation into bounded chunks.
        strictExecutionOrder: true,
        codeSplitting: {
          groups: [
            {
              name: 'react',
              test: /[/\\](?:react|react-dom|scheduler)[/\\]/,
              entriesAware: true,
            },
            {
              name: 'monaco',
              test: /[/\\]monaco-editor[/\\]/,
              entriesAware: true,
              minSize: 100_000,
              maxSize: 1_000_000,
            },
          ],
        },
      },
    },
  },
  worker: { format: 'es' },
});
