/// <reference types="vite/client" />
declare module '*.vue' { import type { DefineComponent } from 'vue'; const component: DefineComponent; export default component }

/** Injected by Vite from the desktop package version. */
declare const __APP_VERSION__: string
