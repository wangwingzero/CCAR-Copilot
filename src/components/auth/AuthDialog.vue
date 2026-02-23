<template>
  <div class="auth-dialog" :class="{ 'is-loading': isLoading }">
    <!-- 对话框头部 -->
    <div class="dialog-header">
      <span class="dialog-title">{{ isLoginMode ? '登录' : '注册' }}</span>
      <button class="close-btn" @click="handleClose" :disabled="isLoading">
        ✕
      </button>
    </div>

    <!-- 表单内容 -->
    <form class="form-content" @submit.prevent="handleSubmit">
      <!-- 错误提示 -->
      <div v-if="error" class="error-message">
        <span class="error-icon">⚠️</span>
        <span class="error-text">{{ error }}</span>
      </div>

      <!-- 邮箱输入 -->
      <div class="form-group">
        <label class="form-label">邮箱</label>
        <input
          ref="emailInput"
          v-model="formData.email"
          type="email"
          class="form-input"
          :class="{ 'is-invalid': fieldErrors.email }"
          placeholder="请输入邮箱"
          :disabled="isLoading"
          autocomplete="email"
          @blur="validateEmail"
        />
        <span v-if="fieldErrors.email" class="field-error">{{ fieldErrors.email }}</span>
      </div>

      <!-- 密码输入 -->
      <div class="form-group">
        <label class="form-label">密码</label>
        <div class="password-input-wrapper">
          <input
            v-model="formData.password"
            :type="showPassword ? 'text' : 'password'"
            class="form-input"
            :class="{ 'is-invalid': fieldErrors.password }"
            placeholder="请输入密码"
            :disabled="isLoading"
            :autocomplete="isLoginMode ? 'current-password' : 'new-password'"
            @blur="validatePassword"
          />
          <button
            type="button"
            class="toggle-password-btn"
            @click="showPassword = !showPassword"
            :disabled="isLoading"
          >
            {{ showPassword ? '👁️' : '👁️‍🗨️' }}
          </button>
        </div>
        <span v-if="fieldErrors.password" class="field-error">{{ fieldErrors.password }}</span>
      </div>

      <!-- 确认密码（仅注册模式） -->
      <div v-if="!isLoginMode" class="form-group">
        <label class="form-label">确认密码</label>
        <input
          v-model="formData.confirmPassword"
          :type="showPassword ? 'text' : 'password'"
          class="form-input"
          :class="{ 'is-invalid': fieldErrors.confirmPassword }"
          placeholder="请再次输入密码"
          :disabled="isLoading"
          autocomplete="new-password"
          @blur="validateConfirmPassword"
        />
        <span v-if="fieldErrors.confirmPassword" class="field-error">{{ fieldErrors.confirmPassword }}</span>
      </div>

      <!-- 昵称（仅注册模式，可选） -->
      <div v-if="!isLoginMode" class="form-group">
        <label class="form-label">昵称 <span class="optional-tag">（可选）</span></label>
        <input
          v-model="formData.nickname"
          type="text"
          class="form-input"
          placeholder="给自己起个名字吧"
          :disabled="isLoading"
          autocomplete="nickname"
        />
      </div>

      <!-- 操作按钮 -->
      <div class="form-actions">
        <button
          type="submit"
          class="submit-btn"
          :class="{ 'is-loading': isLoading }"
          :disabled="!canSubmit"
        >
          <span v-if="isLoading" class="loading-spinner-small" />
          <span class="btn-text">{{ submitButtonText }}</span>
        </button>
      </div>

      <!-- 忘记密码（仅登录模式） -->
      <div v-if="isLoginMode" class="forgot-password">
        <button type="button" class="link-btn" @click="handleForgotPassword" :disabled="isLoading">
          忘记密码？
        </button>
      </div>
    </form>

    <!-- 切换登录/注册 -->
    <div class="switch-mode">
      <span class="switch-text">{{ isLoginMode ? '没有账号？' : '已有账号？' }}</span>
      <button type="button" class="link-btn" @click="toggleMode" :disabled="isLoading">
        {{ isLoginMode ? '立即注册' : '立即登录' }}
      </button>
    </div>

    <!-- 成功提示 -->
    <Transition name="toast">
      <div v-if="showSuccess" class="success-toast">
        {{ successMessage }}
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 登录/注册对话框组件
 *
 * 功能：
 * - 用户登录
 * - 用户注册
 * - 表单验证
 * - 密码重置
 */

import { ref, reactive, computed, watch, onMounted, nextTick } from 'vue'
import { useAuthStore } from '@/stores/auth'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 初始模式：login 或 register */
  initialMode?: 'login' | 'register'
  /** 是否显示 */
  visible?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  initialMode: 'login',
  visible: true,
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'success', user: { id: string; email: string }): void
  (e: 'switch-mode', mode: 'login' | 'register'): void
}>()

// ============================================
// Store
// ============================================

const authStore = useAuthStore()

// ============================================
// State
// ============================================

/** 是否是登录模式 */
const isLoginMode = ref(props.initialMode === 'login')

/** 表单数据 */
const formData = reactive({
  email: '',
  password: '',
  confirmPassword: '',
  nickname: '',
})

/** 字段错误 */
const fieldErrors = reactive({
  email: '',
  password: '',
  confirmPassword: '',
})

/** 是否显示密码 */
const showPassword = ref(false)

/** 是否正在加载 */
const isLoading = computed(() => authStore.isLoading)

/** 错误信息 */
const error = computed(() => authStore.error)

/** 是否显示成功提示 */
const showSuccess = ref(false)

/** 成功消息 */
const successMessage = ref('')

/** 邮箱输入框引用 */
const emailInput = ref<HTMLInputElement | null>(null)

// ============================================
// Computed
// ============================================

/** 提交按钮文字 */
const submitButtonText = computed(() => {
  if (isLoading.value) {
    return isLoginMode.value ? '登录中...' : '注册中...'
  }
  return isLoginMode.value ? '登录' : '注册'
})

/** 是否可以提交 */
const canSubmit = computed(() => {
  if (isLoading.value) return false
  if (!formData.email || !formData.password) return false
  if (!isLoginMode.value && !formData.confirmPassword) return false
  if (fieldErrors.email || fieldErrors.password || fieldErrors.confirmPassword) return false
  return true
})

// ============================================
// Validation
// ============================================

/** 验证邮箱 */
function validateEmail(): boolean {
  const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  if (!formData.email) {
    fieldErrors.email = '请输入邮箱'
    return false
  }
  if (!emailRegex.test(formData.email)) {
    fieldErrors.email = '邮箱格式不正确'
    return false
  }
  fieldErrors.email = ''
  return true
}

/** 验证密码 */
function validatePassword(): boolean {
  if (!formData.password) {
    fieldErrors.password = '请输入密码'
    return false
  }
  if (formData.password.length < 6) {
    fieldErrors.password = '密码至少需要 6 位'
    return false
  }
  fieldErrors.password = ''
  return true
}

/** 验证确认密码 */
function validateConfirmPassword(): boolean {
  if (!isLoginMode.value) {
    if (!formData.confirmPassword) {
      fieldErrors.confirmPassword = '请确认密码'
      return false
    }
    if (formData.password !== formData.confirmPassword) {
      fieldErrors.confirmPassword = '两次密码不一致'
      return false
    }
  }
  fieldErrors.confirmPassword = ''
  return true
}

/** 验证所有字段 */
function validateAll(): boolean {
  const emailValid = validateEmail()
  const passwordValid = validatePassword()
  const confirmValid = isLoginMode.value || validateConfirmPassword()
  return emailValid && passwordValid && confirmValid
}

// ============================================
// Methods
// ============================================

/**
 * 提交表单
 */
async function handleSubmit(): Promise<void> {
  if (!validateAll()) return

  authStore.clearError()

  let success: boolean

  if (isLoginMode.value) {
    success = await authStore.login(formData.email, formData.password)
  } else {
    success = await authStore.signUp(
      formData.email,
      formData.password,
      formData.nickname || undefined
    )
  }

  if (success) {
    successMessage.value = isLoginMode.value ? '登录成功' : '注册成功'
    showSuccess.value = true

    // 清空密码字段
    formData.password = ''
    formData.confirmPassword = ''

    setTimeout(() => {
      showSuccess.value = false
      if (authStore.user) {
        emit('success', { id: authStore.user.id, email: authStore.user.email })
      }
      handleClose()
    }, 1500)
  }
}

/**
 * 切换登录/注册模式
 */
function toggleMode(): void {
  isLoginMode.value = !isLoginMode.value
  authStore.clearError()

  // 清空表单错误
  fieldErrors.email = ''
  fieldErrors.password = ''
  fieldErrors.confirmPassword = ''

  emit('switch-mode', isLoginMode.value ? 'login' : 'register')

  // 聚焦邮箱输入框
  nextTick(() => {
    emailInput.value?.focus()
  })
}

/**
 * 忘记密码
 */
async function handleForgotPassword(): Promise<void> {
  if (!formData.email) {
    fieldErrors.email = '请先输入邮箱'
    emailInput.value?.focus()
    return
  }

  if (!validateEmail()) return

  const success = await authStore.resetPassword(formData.email)

  if (success) {
    successMessage.value = '重置密码邮件已发送，请查收'
    showSuccess.value = true
    setTimeout(() => {
      showSuccess.value = false
    }, 3000)
  }
}

/**
 * 关闭对话框
 */
function handleClose(): void {
  authStore.clearError()
  emit('close')
}

// ============================================
// Watchers
// ============================================

// 监听可见性变化
watch(() => props.visible, (newVal) => {
  if (newVal) {
    nextTick(() => {
      emailInput.value?.focus()
    })
  }
})

// ============================================
// Lifecycle
// ============================================

onMounted(() => {
  if (props.visible) {
    emailInput.value?.focus()
  }
})
</script>

<style scoped>
.auth-dialog {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 340px;
  max-width: 400px;
  position: relative;
}

/* 对话框头部 */
.dialog-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.dialog-title {
  color: #fff;
  font-size: 18px;
  font-weight: 600;
}

.close-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 14px;
  cursor: pointer;
  transition: all 0.15s;
}

.close-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.close-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 表单内容 */
.form-content {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-label {
  color: rgba(255, 255, 255, 0.8);
  font-size: 13px;
  font-weight: 500;
}

.optional-tag {
  color: rgba(255, 255, 255, 0.4);
  font-size: 12px;
  font-weight: normal;
}

.form-input {
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 6px;
  color: #fff;
  font-size: 14px;
  outline: none;
  transition: border-color 0.15s, background 0.15s;
}

.form-input::placeholder {
  color: rgba(255, 255, 255, 0.3);
}

.form-input:hover:not(:disabled) {
  border-color: rgba(255, 255, 255, 0.25);
}

.form-input:focus {
  border-color: rgba(66, 133, 244, 0.6);
  background: rgba(0, 0, 0, 0.4);
}

.form-input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.form-input.is-invalid {
  border-color: rgba(244, 67, 54, 0.6);
}

/* 密码输入框包装器 */
.password-input-wrapper {
  position: relative;
}

.password-input-wrapper .form-input {
  padding-right: 40px;
}

.toggle-password-btn {
  position: absolute;
  right: 8px;
  top: 50%;
  transform: translateY(-50%);
  background: transparent;
  border: none;
  padding: 4px;
  cursor: pointer;
  font-size: 16px;
  opacity: 0.6;
  transition: opacity 0.15s;
}

.toggle-password-btn:hover:not(:disabled) {
  opacity: 1;
}

.toggle-password-btn:disabled {
  cursor: not-allowed;
}

/* 字段错误 */
.field-error {
  color: #ff6b6b;
  font-size: 12px;
}

/* 错误提示 */
.error-message {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 6px;
}

.error-icon {
  font-size: 14px;
  flex-shrink: 0;
}

.error-text {
  color: #ff6b6b;
  font-size: 13px;
  line-height: 1.4;
}

/* 操作按钮 */
.form-actions {
  margin-top: 8px;
}

.submit-btn {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 16px;
  background: linear-gradient(135deg, #4285f4 0%, #3367d6 100%);
  border: none;
  border-radius: 6px;
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
}

.submit-btn:hover:not(:disabled) {
  background: linear-gradient(135deg, #5a9bff 0%, #4285f4 100%);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(66, 133, 244, 0.3);
}

.submit-btn:active:not(:disabled) {
  transform: translateY(0);
}

.submit-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
  transform: none;
  box-shadow: none;
}

.submit-btn.is-loading {
  background: rgba(66, 133, 244, 0.6);
}

/* 忘记密码 */
.forgot-password {
  text-align: center;
}

.link-btn {
  background: transparent;
  border: none;
  color: rgba(66, 133, 244, 0.9);
  font-size: 13px;
  cursor: pointer;
  transition: color 0.15s;
}

.link-btn:hover:not(:disabled) {
  color: #4285f4;
  text-decoration: underline;
}

.link-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 切换模式 */
.switch-mode {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 4px;
  padding-top: 12px;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}

.switch-text {
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
}

/* 加载动画 */
.loading-spinner-small {
  width: 16px;
  height: 16px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: #fff;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 成功提示 */
.success-toast {
  position: absolute;
  bottom: 20px;
  left: 50%;
  transform: translateX(-50%);
  padding: 10px 20px;
  background: rgba(76, 175, 80, 0.95);
  border-radius: 6px;
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  z-index: 100;
}

/* Toast 动画 */
.toast-enter-active,
.toast-leave-active {
  transition: all 0.2s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(10px);
}
</style>
