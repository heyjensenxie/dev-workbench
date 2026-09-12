import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'
import packageJson from './package.json'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  define: { __APP_VERSION__: JSON.stringify(packageJson.version) },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
})
