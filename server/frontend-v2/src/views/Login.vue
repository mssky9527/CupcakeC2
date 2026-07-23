<template>
  <div class="auth-page-wrapper">
    <!-- Ambient glowing backgrounds -->
    <div class="auth-bg-glows">
      <div class="glow-1"></div>
      <div class="glow-2"></div>
      <div class="glow-grid"></div>
    </div>
    
    <div class="auth-container">
      <!-- Left Hero Section -->
      <section class="auth-hero surface-card">
        <div class="auth-hero__top">
          <div class="auth-brand">
            <span class="brand-logo-icon">🧁</span>
            <span class="auth-kicker">CUPCAKE CONSOLE • SYSTEM ACCESS</span>
          </div>
          <h1>统一界面，统一布局，操作行云流水。</h1>
          <p>
            基于现代化控制舱设计理念，打造清爽纯粹的管理身份标识。摒弃繁杂的渐变堆叠，给您专注而高效的操作体验。
          </p>
        </div>

        <!-- Center Visual Graphic Illustration -->
        <div class="hero-graphic">
          <div class="graphic-card">
            <div class="graphic-card__bar">
              <div class="graphic-card__dots">
                <span class="dot dot-red"></span>
                <span class="dot dot-yellow"></span>
                <span class="dot dot-green"></span>
              </div>
              <span class="graphic-title">CONTROL PLANE DECK // LIVE MONITOR</span>
            </div>
            <div class="graphic-card__body">
              <div class="graphic-status-row">
                <div class="graphic-chip active">
                  <span class="pulse-dot"></span> 节点监听 24/24
                </div>
                <div class="graphic-chip">
                  延迟 12ms
                </div>
                <div class="graphic-chip">
                  负载 8.4%
                </div>
              </div>

              <!-- Animated Chart Lines SVG -->
              <div class="graphic-chart-box">
                <svg class="graphic-wave" viewBox="0 0 400 90" fill="none" xmlns="http://www.w3.org/2000/svg">
                  <defs>
                    <linearGradient id="chartGrad" x1="0%" y1="0%" x2="0%" y2="100%">
                      <stop offset="0%" stop-color="rgba(17,17,17,0.08)" />
                      <stop offset="100%" stop-color="rgba(17,17,17,0.00)" />
                    </linearGradient>
                  </defs>
                  <path d="M0 65 Q 60 25, 120 50 T 240 30 T 340 70 T 400 20 V 90 H 0 Z" fill="url(#chartGrad)" />
                  <path d="M0 65 Q 60 25, 120 50 T 240 30 T 340 70 T 400 20" stroke="#111111" stroke-width="2" fill="none" stroke-dasharray="0" />
                  <circle cx="120" cy="50" r="4" fill="#111111" />
                  <circle cx="240" cy="30" r="4" fill="#111111" />
                  <circle cx="340" cy="70" r="4" fill="#111111" />
                </svg>
              </div>

              <div class="graphic-metrics">
                <div class="metric-item">
                  <span class="metric-val">99.98%</span>
                  <span class="metric-lbl">服务在线率</span>
                </div>
                <div class="metric-divider"></div>
                <div class="metric-item">
                  <span class="metric-val">0 异常</span>
                  <span class="metric-lbl">安全通道评估</span>
                </div>
                <div class="metric-divider"></div>
                <div class="metric-item">
                  <span class="metric-val">TLS 1.3</span>
                  <span class="metric-lbl">端到端加密</span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Bottom Signal Cards -->
        <div class="auth-signal-grid">
          <div class="signal-card">
            <div class="signal-header">
              <el-icon class="icon-pulse"><Connection /></el-icon>
              <span class="signal-card__label">活跃通道</span>
            </div>
            <strong class="signal-card__num">24</strong>
            <span class="signal-card__hint">当前监控传输边界</span>
          </div>
          <div class="signal-card">
            <div class="signal-header">
              <el-icon class="icon-spin"><Setting /></el-icon>
              <span class="signal-card__label">布局状态</span>
            </div>
            <strong class="signal-card__num status-unified">统一</strong>
            <span class="signal-card__hint">跨视图共享 Shell</span>
          </div>
        </div>
      </section>

      <!-- Right Login Form Panel -->
      <section class="auth-panel surface-card">
        <div class="auth-panel__head">
          <span class="auth-panel__eyebrow">LOGIN PORTAL</span>
          <h2>操作员访问</h2>
          <p>请提供受信任凭据以认证并进入工作区</p>
        </div>

        <el-form :model="form" label-position="top" class="auth-form" @keyup.enter="handleLogin">
          <el-form-item label="用户名">
            <el-input v-model="form.username" placeholder="请输入操作员帐户" class="premium-input">
              <template #prefix>
                <el-icon class="input-icon"><User /></el-icon>
              </template>
            </el-input>
          </el-form-item>

          <el-form-item label="密码">
            <el-input v-model="form.password" type="password" placeholder="请输入访问密码" show-password class="premium-input">
              <template #prefix>
                <el-icon class="input-icon"><Lock /></el-icon>
              </template>
            </el-input>
          </el-form-item>

          <div class="auth-form__meta">
            <el-checkbox v-model="form.agreed" class="premium-checkbox" @change="handleAgreedChange">
              我了解法律及操作审计边界
            </el-checkbox>
          </div>

          <el-button type="primary" class="auth-submit premium-btn" :loading="loading" @click="handleLogin">
            {{ loading ? '验证中...' : '认证并接入' }}
          </el-button>
        </el-form>

        <div class="auth-footer">
          <div class="auth-footer__status">
            <span class="status-dot"></span>
            <span>当前节点已接入 Cupcake 主网关，通信已加密。</span>
          </div>
        </div>

        <el-dialog
          v-model="showDisclaimer"
          title="安全与审计通知"
          width="520px"
          append-to-body
          align-center
          class="premium-dialog"
          @close="handleCloseDisclaimer"
        >
          <div class="notice-copy">
            <p class="notice-highlight">⚠️ 授权声明</p>
            <p>此管理终端仅供获得书面授权的安全测试及系统合规审计项目使用。</p>
            <p>您在此控制台的所有会话操作（包括传输、命令执行与日志查阅）均会被加密留存并记录于操作日志中。继续进行登录即代表您已知晓并接受相关的审计责任及约束。</p>
          </div>
          <template #footer>
            <el-button type="primary" @click="handleAgreeDisclaimer">我已了解并同意</el-button>
          </template>
        </el-dialog>
      </section>
    </div>
  </div>
</template>

<script setup>
import { reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { User, Lock, Connection, Setting } from '@element-plus/icons-vue'
import api from '../api/index'

const router = useRouter()
const loading = ref(false)
const showDisclaimer = ref(false)
const confirmedAgreed = ref(false)

const form = reactive({
  username: '',
  password: '',
  agreed: false
})

const handleAgreedChange = (val) => {
  if (val) {
    confirmedAgreed.value = false
    showDisclaimer.value = true
  } else {
    confirmedAgreed.value = false
  }
}

const handleAgreeDisclaimer = () => {
  confirmedAgreed.value = true
  form.agreed = true
  showDisclaimer.value = false
}

const handleCloseDisclaimer = () => {
  if (!confirmedAgreed.value) {
    form.agreed = false
  }
}

const handleLogin = async () => {
  if (!form.username || !form.password) {
    ElMessage.warning('请输入用户名和密码。')
    return
  }

  if (!form.agreed) {
    ElMessage.warning('请确认操作员通知。')
    return
  }

  loading.value = true
  try {
    const res = await api.post('/api/auth/login', form)
    localStorage.setItem('cupcake_token', res.data.token)
    localStorage.setItem('cupcake_user', JSON.stringify(res.data.user))
    ElMessage.success('认证成功。')
    router.push('/dashboard')
  } catch (e) {
    ElMessage.error(e.response?.data?.error || '认证失败。')
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.auth-page-wrapper {
  min-height: 100vh;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 32px 24px;
  position: relative;
  background-color: #f7f7f8;
  box-sizing: border-box;
  overflow-x: hidden;
}

/* Background glows and subtle grid */
.auth-bg-glows {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 0;
  overflow: hidden;
}

.glow-1 {
  position: absolute;
  top: -15%;
  left: -10%;
  width: 55%;
  height: 55%;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(17, 17, 17, 0.04) 0%, transparent 70%);
  filter: blur(50px);
}

.glow-2 {
  position: absolute;
  bottom: -15%;
  right: -10%;
  width: 60%;
  height: 60%;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(17, 17, 17, 0.05) 0%, transparent 70%);
  filter: blur(60px);
}

.glow-grid {
  position: absolute;
  inset: 0;
  background-image: radial-gradient(rgba(17, 17, 17, 0.08) 1px, transparent 1px);
  background-size: 24px 24px;
  opacity: 0.4;
}

.auth-container {
  position: relative;
  z-index: 1;
  width: 100%;
  max-width: 1280px;
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(380px, 440px);
  gap: 32px;
  align-items: stretch;
}

.auth-hero,
.auth-panel {
  border-radius: var(--radius-lg, 24px);
  background: rgba(255, 255, 255, 0.95);
  border: 1px solid rgba(17, 17, 17, 0.08);
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.03);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
}

/* Left Hero Section */
.auth-hero {
  padding: 44px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
}

.auth-brand {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 20px;
}

.brand-logo-icon {
  font-size: 22px;
}

.auth-kicker {
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  color: #767676;
}

.auth-hero h1 {
  margin: 0 0 16px;
  font-size: clamp(28px, 3.2vw, 42px);
  line-height: 1.15;
  letter-spacing: -0.04em;
  font-weight: 800;
  color: #111111;
}

.auth-hero p {
  margin: 0;
  max-width: 560px;
  line-height: 1.7;
  color: #555555;
  font-size: 14px;
}

/* Middle Graphic Card */
.hero-graphic {
  margin: 28px 0;
}

.graphic-card {
  background: #fafafa;
  border: 1px solid rgba(17, 17, 17, 0.07);
  border-radius: 16px;
  overflow: hidden;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.02);
}

.graphic-card__bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 16px;
  background: #ffffff;
  border-bottom: 1px solid rgba(17, 17, 17, 0.06);
}

.graphic-card__dots {
  display: flex;
  gap: 6px;
}

.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: #d4d4d4;
}

.dot-red { background: #ff5f56; }
.dot-yellow { background: #ffbd2e; }
.dot-green { background: #27c93f; }

.graphic-title {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.12em;
  color: #888888;
}

.graphic-card__body {
  padding: 16px;
}

.graphic-status-row {
  display: flex;
  gap: 8px;
  margin-bottom: 14px;
  flex-wrap: wrap;
}

.graphic-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: 999px;
  background: #ffffff;
  border: 1px solid rgba(17, 17, 17, 0.08);
  font-size: 12px;
  font-weight: 600;
  color: #444444;
}

.graphic-chip.active {
  color: #111111;
}

.pulse-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #111111;
  animation: pulse 1.8s infinite;
}

.graphic-chart-box {
  width: 100%;
  height: 90px;
  margin-bottom: 14px;
}

.graphic-wave {
  width: 100%;
  height: 100%;
}

.graphic-metrics {
  display: flex;
  align-items: center;
  justify-content: space-around;
  padding-top: 12px;
  border-top: 1px solid rgba(17, 17, 17, 0.05);
}

.metric-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.metric-val {
  font-size: 14px;
  font-weight: 800;
  color: #111111;
}

.metric-lbl {
  font-size: 11px;
  color: #888888;
}

.metric-divider {
  width: 1px;
  height: 24px;
  background: rgba(17, 17, 17, 0.08);
}

/* Bottom Signal Grid */
.auth-signal-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}

.signal-card {
  padding: 18px 20px;
  background: #ffffff;
  border: 1px solid rgba(17, 17, 17, 0.06);
  border-radius: 16px;
  transition: transform 0.25 ease, box-shadow 0.25s ease;
}

.signal-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.03);
}

.signal-header {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #767676;
  margin-bottom: 8px;
}

.signal-header .el-icon {
  font-size: 16px;
  color: #111111;
}

.icon-pulse {
  animation: pulse 2s infinite;
}

.icon-spin {
  animation: spin 8s linear infinite;
}

@keyframes pulse {
  0% { transform: scale(1); opacity: 1; }
  50% { transform: scale(1.18); opacity: 0.7; }
  100% { transform: scale(1); opacity: 1; }
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.signal-card__label {
  font-size: 12px;
  font-weight: 700;
}

.signal-card__num {
  display: block;
  font-size: 28px;
  font-weight: 800;
  letter-spacing: -0.03em;
  color: #111111;
  line-height: 1.2;
  margin-bottom: 4px;
}

.signal-card__hint {
  font-size: 11px;
  color: #888888;
}

/* Right Panel Section */
.auth-panel {
  padding: 44px;
  display: flex;
  flex-direction: column;
  justify-content: center;
}

.auth-panel__head {
  margin-bottom: 28px;
}

.auth-panel__eyebrow {
  display: inline-block;
  margin-bottom: 8px;
  font-size: 11px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  color: #767676;
  font-weight: 800;
}

.auth-panel__head h2 {
  margin: 0 0 8px;
  font-size: 28px;
  font-weight: 800;
  letter-spacing: -0.03em;
  color: #111111;
}

.auth-panel__head p {
  margin: 0;
  color: #666666;
  line-height: 1.6;
  font-size: 14px;
}

/* Form Styles */
.auth-form {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

:deep(.el-form-item__label) {
  font-size: 13px;
  font-weight: 700;
  color: #333333;
  margin-bottom: 6px !important;
}

.premium-input :deep(.el-input__wrapper) {
  padding: 8px 14px;
  height: 44px;
  border-radius: 12px;
  background-color: #fafafa;
  border: 1px solid rgba(17, 17, 17, 0.08);
  box-shadow: none !important;
  transition: all 0.25s ease;
}

.premium-input :deep(.el-input__wrapper.is-focus),
.premium-input :deep(.el-input__wrapper:hover) {
  border-color: #111111;
  background-color: #ffffff;
}

.premium-input :deep(.el-input__inner) {
  font-family: inherit;
  font-size: 14px;
}

.input-icon {
  font-size: 16px;
  color: #888888;
  margin-right: 4px;
}

.auth-form__meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  margin: 6px 0 20px;
}

.premium-checkbox :deep(.el-checkbox__label) {
  font-size: 13px;
  color: #555555;
  font-weight: 500;
}

.premium-checkbox :deep(.el-checkbox__input.is-checked .el-checkbox__inner) {
  background-color: #111111;
  border-color: #111111;
}

.inline-link {
  border: 0;
  background: none;
  padding: 0;
  color: #555555;
  font: inherit;
  font-weight: 700;
  font-size: 13px;
  cursor: pointer;
  text-decoration: underline;
  transition: color 0.2s ease;
  white-space: nowrap;
}

.inline-link:hover {
  color: #111111;
}

.premium-btn {
  height: 48px;
  width: 100%;
  border-radius: 12px !important;
  background-color: #111111 !important;
  border-color: #111111 !important;
  font-size: 15px;
  font-weight: 700;
  transition: all 0.2s ease;
}

.premium-btn:hover {
  background-color: #2c2c2c !important;
  border-color: #2c2c2c !important;
  transform: translateY(-1px);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.12);
}

.auth-footer {
  margin-top: 32px;
  padding-top: 20px;
  border-top: 1px solid rgba(17, 17, 17, 0.06);
}

.auth-footer__status {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: #888888;
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #27c93f;
  flex-shrink: 0;
}

.notice-copy {
  display: flex;
  flex-direction: column;
  gap: 14px;
  line-height: 1.7;
  color: #444444;
}

.notice-highlight {
  font-weight: 700;
  color: #111111;
  font-size: 15px;
  margin-bottom: 4px;
}

:deep(.premium-dialog) {
  border-radius: 20px;
  overflow: hidden;
}

:deep(.premium-dialog .el-dialog__header) {
  margin-right: 0;
  padding: 24px 24px 12px;
  border-bottom: 1px solid rgba(17, 17, 17, 0.05);
}

:deep(.premium-dialog .el-dialog__title) {
  font-weight: 800;
}

:deep(.premium-dialog .el-dialog__body) {
  padding: 24px;
}

:deep(.premium-dialog .el-dialog__footer) {
  padding: 12px 24px 24px;
  border-top: 1px solid rgba(17, 17, 17, 0.05);
}

/* Responsive adjustments */
@media (max-width: 1024px) {
  .auth-container {
    grid-template-columns: 1fr;
    max-width: 520px;
    gap: 24px;
  }

  .auth-hero,
  .auth-panel {
    padding: 32px;
  }

  .hero-graphic {
    margin: 20px 0;
  }
}

@media (max-width: 560px) {
  .auth-page-wrapper {
    padding: 16px;
  }

  .auth-hero,
  .auth-panel {
    padding: 24px;
    border-radius: 18px;
  }

  .auth-signal-grid {
    grid-template-columns: 1fr;
  }

  .auth-form__meta {
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
  }
}
</style>
