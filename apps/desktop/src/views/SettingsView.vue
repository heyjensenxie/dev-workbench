<script setup lang="ts">
import { LOG_LIMIT_MAX, LOG_LIMIT_MIN } from '@dev-workbench/shared'
import { Info, Languages, Palette, Settings2 } from 'lucide-vue-next'
import { onMounted } from 'vue'
import { useI18n } from '../i18n'
import { useSettingsStore } from '../stores/settings'

const { t, locale, setLocale } = useI18n()
const settings = useSettingsStore()
const version = __APP_VERSION__

function onLogLimitChange(event: Event): void {
  const value = Number((event.target as HTMLInputElement).value)
  if (Number.isFinite(value)) void settings.setLogLimit(value)
}

function onConfirmChange(event: Event): void {
  void settings.setConfirmBeforeKill((event.target as HTMLInputElement).checked)
}

function onOpenLastProjectChange(event: Event): void {
  void settings.setOpenLastProject((event.target as HTMLInputElement).checked)
}

onMounted(() => { if (!settings.loaded) void settings.load() })
</script>

<template>
  <section class="view settings-view">
    <header class="view-header settings-page-header">
      <div>
        <p class="eyebrow">{{ t('preferences') }}</p>
        <h1>{{ t('settings') }}</h1>
        <p>{{ t('settingsSubtitle') }}</p>
      </div>
    </header>

    <p v-if="settings.error" class="error-banner">{{ settings.error }}</p>

    <section class="detail-panel settings-panel">
      <div class="settings-panel-header">
        <span class="settings-panel-icon"><Palette :size="15" /></span>
        <div>
          <h2>{{ t('appearance') }}</h2>
          <p>{{ t('appearanceHint') }}</p>
        </div>
      </div>

      <div class="settings-list">
        <div class="settings-item">
          <div class="settings-copy">
            <span class="settings-label">{{ t('theme') }}</span>
            <small>{{ t('themeHint') }}</small>
          </div>
          <div class="settings-segmented" role="group" :aria-label="t('theme')">
            <button type="button" :class="{ active: settings.theme === 'system' }" :aria-pressed="settings.theme === 'system'" @click="settings.setTheme('system')">{{ t('themeSystem') }}</button>
            <button type="button" :class="{ active: settings.theme === 'dark' }" :aria-pressed="settings.theme === 'dark'" @click="settings.setTheme('dark')">{{ t('themeDark') }}</button>
            <button type="button" :class="{ active: settings.theme === 'light' }" :aria-pressed="settings.theme === 'light'" @click="settings.setTheme('light')">{{ t('themeLight') }}</button>
          </div>
        </div>

        <div class="settings-item">
          <div class="settings-copy">
            <span class="settings-label"><Languages :size="13" />{{ t('language') }}</span>
            <small>{{ t('languageHint') }}</small>
          </div>
          <div class="settings-segmented" role="group" :aria-label="t('language')">
            <button type="button" :class="{ active: locale === 'zh-CN' }" :aria-pressed="locale === 'zh-CN'" @click="setLocale('zh-CN')">中文</button>
            <button type="button" :class="{ active: locale === 'en-US' }" :aria-pressed="locale === 'en-US'" @click="setLocale('en-US')">English</button>
          </div>
        </div>
      </div>
    </section>

    <section class="detail-panel settings-panel">
      <div class="settings-panel-header">
        <span class="settings-panel-icon"><Settings2 :size="15" /></span>
        <div>
          <h2>{{ t('general') }}</h2>
          <p>{{ t('generalHint') }}</p>
        </div>
      </div>

      <div class="settings-list">
        <label class="settings-item settings-toggle-item">
          <span class="settings-copy">
            <span class="settings-label">{{ t('openLastProject') }}</span>
            <small>{{ t('openLastProjectHint') }}</small>
          </span>
          <span class="settings-switch">
            <input type="checkbox" :checked="settings.openLastProject" @change="onOpenLastProjectChange" />
            <span class="settings-switch-track"><span></span></span>
          </span>
        </label>
      </div>
    </section>

    <section class="detail-panel settings-panel">
      <div class="settings-panel-header">
        <span class="settings-panel-icon"><Settings2 :size="15" /></span>
        <div>
          <h2>{{ t('runtime') }}</h2>
          <p>{{ t('runtimeHint') }}</p>
        </div>
      </div>

      <div class="settings-list">
        <div class="settings-item">
          <div class="settings-copy">
            <span class="settings-label">{{ t('logLimit') }}</span>
            <small>{{ t('logLimitHint') }}</small>
          </div>
          <div class="settings-number-control">
          <input
            class="number-input"
            type="number"
            :min="LOG_LIMIT_MIN"
            :max="LOG_LIMIT_MAX"
            step="100"
            :aria-label="t('logLimit')"
            :value="settings.logLimit"
            @change="onLogLimitChange"
          />
            <span>{{ t('logLines') }}</span>
          </div>
        </div>

        <label class="settings-item settings-toggle-item">
          <span class="settings-copy">
            <span class="settings-label">{{ t('confirmBeforeKill') }}</span>
            <small>{{ t('confirmBeforeKillHint') }}</small>
          </span>
          <span class="settings-switch">
            <input type="checkbox" :checked="settings.confirmBeforeKill" @change="onConfirmChange" />
            <span class="settings-switch-track"><span></span></span>
          </span>
        </label>
      </div>
    </section>

    <section class="detail-panel settings-panel">
      <div class="settings-panel-header">
        <span class="settings-panel-icon"><Info :size="15" /></span>
        <div>
          <h2>{{ t('about') }}</h2>
          <p>{{ t('aboutHint') }}</p>
        </div>
      </div>
      <div class="about-details">
        <div><span>{{ t('name') }}</span><strong>Dev Workbench</strong></div>
        <div><span>{{ t('version') }}</span><code>{{ version }}</code></div>
      </div>
      <p class="settings-note">{{ t('dataLocation') }}</p>
    </section>
  </section>
</template>
