import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'es2024',
    // Official Vite capability: emits the licences of the dependencies this
    // bundle actually packages. Tauri embeds the built frontend in the
    // distributed binary, so the report ships with the artifact. It covers what
    // the bundler sees — not the Rust crates under `src-tauri/`, which stay in
    // the THIRD-PARTY-NOTICES.txt snapshot.
    license: { fileName: 'legal/BUNDLED-LICENSES.md' },
    rolldownOptions: {
      output: {
        postBanner: '/* Third-party licenses: /legal/BUNDLED-LICENSES.md */',
      },
    },
  },
});
