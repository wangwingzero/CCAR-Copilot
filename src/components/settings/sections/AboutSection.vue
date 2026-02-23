<template>
  <div class="about-section">
    <!-- App Info Group -->
    <SettingsGroup :title="$t('settings.about.title')">
      <!-- Logo and App Name -->
      <div class="app-header">
        <img
          src="/resources/PNG/虎哥截图.png"
          alt="虎哥截图"
          class="app-logo"
          @error="handleLogoError"
        />
        <div class="app-info">
          <h2 class="app-name">{{ appName }}</h2>
          <span class="app-version">{{ $t('settings.about.version', { version: appVersion }) }}</span>
        </div>
      </div>

      <!-- Description -->
      <p class="app-description">{{ $t('settings.about.description') }}</p>
    </SettingsGroup>

    <!-- License Group -->
    <SettingsGroup :title="$t('settings.about.license')">
      <div class="license-info">
        <span class="license-type">MIT License</span>
        <p class="license-text">{{ $t('settings.about.licenseText') }}</p>
      </div>
    </SettingsGroup>

    <!-- Third-Party Acknowledgments -->
    <SettingsGroup :title="$t('settings.about.thirdParty')">
      <div class="acknowledgments">
        <div
          v-for="lib in thirdPartyLibs"
          :key="lib.name"
          class="lib-item"
        >
          <span class="lib-name">{{ lib.name }}</span>
          <span class="lib-license">{{ lib.license }}</span>
        </div>
      </div>
    </SettingsGroup>

    <!-- Links Group -->
    <SettingsGroup :title="$t('settings.about.links')">
      <div class="link-buttons">
        <button class="link-btn" @click="openGitHub">
          <span class="link-icon">📦</span>
          <span class="link-text">GitHub</span>
        </button>
        <button class="link-btn" @click="openIssues">
          <span class="link-icon">🐛</span>
          <span class="link-text">{{ $t('settings.about.reportIssue') }}</span>
        </button>
        <button class="link-btn" @click="openDocs">
          <span class="link-icon">📖</span>
          <span class="link-text">{{ $t('settings.about.documentation') }}</span>
        </button>
      </div>
    </SettingsGroup>
  </div>
</template>

<script setup lang="ts">
/**
 * AboutSection - About Settings Section
 *
 * Displays application information:
 * - Logo and app name
 * - Version number
 * - License info (MIT)
 * - Third-party acknowledgments
 * - GitHub and issue tracker links
 *
 * Uses the reusable settings control components:
 * - SettingsGroup for card-style grouping
 *
 * @validates Requirements 10.1, 10.2, 10.3, 10.4, 10.5, 10.6
 */

import { ref } from 'vue'
import { open } from '@tauri-apps/plugin-shell'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'

// ============================================
// Constants
// ============================================

/** Application name */
const appName = '虎哥截图'

/** Application version - should be read from package.json or Tauri config */
const appVersion = ref('0.1.0')

/** GitHub repository URL */
const GITHUB_URL = 'https://github.com/hugescreenshot/hugescreenshot'

/** Issues URL */
const ISSUES_URL = 'https://github.com/hugescreenshot/hugescreenshot/issues'

/** Documentation URL */
const DOCS_URL = 'https://github.com/hugescreenshot/hugescreenshot#readme'

/** Third-party libraries used */
const thirdPartyLibs = [
  { name: 'Tauri', license: 'MIT/Apache-2.0' },
  { name: 'Vue.js', license: 'MIT' },
  { name: 'Pinia', license: 'MIT' },
  { name: 'Lucide Icons', license: 'ISC' },
  { name: 'fast-check', license: 'MIT' },
  { name: 'RapidOCR', license: 'Apache-2.0' },
]

// ============================================
// Methods
// ============================================

/**
 * Handle logo load error
 * Falls back to a placeholder or hides the image
 */
function handleLogoError(event: Event): void {
  const img = event.target as HTMLImageElement
  img.style.display = 'none'
}

// ============================================
// Event Handlers
// ============================================

/**
 * Open GitHub repository in browser
 * @validates Requirements 10.5
 */
async function openGitHub(): Promise<void> {
  try {
    await open(GITHUB_URL)
  } catch (error) {
    console.error('Failed to open GitHub:', error)
  }
}

/**
 * Open issues page in browser
 * @validates Requirements 10.6
 */
async function openIssues(): Promise<void> {
  try {
    await open(ISSUES_URL)
  } catch (error) {
    console.error('Failed to open issues:', error)
  }
}

/**
 * Open documentation in browser
 */
async function openDocs(): Promise<void> {
  try {
    await open(DOCS_URL)
  } catch (error) {
    console.error('Failed to open docs:', error)
  }
}
</script>

<style scoped>
.about-section {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.app-header {
  display: flex;
  align-items: center;
  gap: 16px;
  margin-bottom: 16px;
}

.app-logo {
  width: 64px;
  height: 64px;
  border-radius: 12px;
}

.app-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.app-name {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
}

.app-version {
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
}

.app-description {
  color: rgba(255, 255, 255, 0.7);
  font-size: 13px;
  line-height: 1.6;
  margin: 0;
}

.license-info {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.license-type {
  color: #4285f4;
  font-size: 14px;
  font-weight: 500;
}

.license-text {
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
  line-height: 1.5;
  margin: 0;
}

.acknowledgments {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.lib-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  background: rgba(255, 255, 255, 0.03);
  border-radius: 4px;
}

.lib-name {
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
}

.lib-license {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
}

.link-buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

.link-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.1s;
}

.link-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.2);
}

.link-icon {
  font-size: 16px;
}

.link-text {
  font-weight: 500;
}
</style>
