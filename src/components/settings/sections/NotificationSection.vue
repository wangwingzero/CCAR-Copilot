<script setup lang="ts">
import { computed } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import SettingItem from '@/components/settings/controls/SettingItem.vue'
import ToggleSwitch from '@/components/settings/controls/ToggleSwitch.vue'

const settingsStore = useSettingsStore()

const notificationConfig = computed(() => settingsStore.notification)

function handleStartupChange(value: boolean): void {
  settingsStore.updateNotification({ startup: value })
}

function handleSoftwareUpdateChange(value: boolean): void {
  settingsStore.updateNotification({ softwareUpdate: value })
}
</script>

<template>
  <div class="notification-section">
    <SettingsGroup :title="$t('settings.notification.title')">
      <!-- Startup Notification -->
      <SettingItem
        :label="$t('settings.notification.startup')"
        :help-text="$t('settings.notification.startupHelp')"
      >
        <ToggleSwitch
          :model-value="notificationConfig.startup"
          :aria-label="$t('settings.notification.startup')"
          @update:model-value="handleStartupChange"
        />
      </SettingItem>

      <!-- Software Update Notification -->
      <SettingItem
        :label="$t('settings.notification.softwareUpdate')"
        :help-text="$t('settings.notification.softwareUpdateHelp')"
      >
        <ToggleSwitch
          :model-value="notificationConfig.softwareUpdate"
          :aria-label="$t('settings.notification.softwareUpdate')"
          @update:model-value="handleSoftwareUpdateChange"
        />
      </SettingItem>
    </SettingsGroup>
  </div>
</template>

<style scoped>
.notification-section {
  /* Section container - inherits dark theme from parent */
}
</style>
