<template>
  <div class="auth-shell">
    <!-- Background elements for visual premium feel -->
    <div class="auth-bg-glows">
      <div class="glow-1"></div>
      <div class="glow-2"></div>
    </div>
    
    <section class="auth-hero">
      <div class="auth-hero__copy">
        <span class="auth-kicker">Cupcake Console</span>
        <h1>统一界面，统一布局，操作行云流水。</h1>
        <p>
          基于现代化控制舱设计理念，打造清爽纯粹的管理身份标识。摒弃繁杂的渐变堆叠，给您专注而高效的操作体验。
        </p>
      </div>

      <div class="auth-signal-grid">
        <div class="signal-card surface-card">
          <div class="signal-header">
            <el-icon class="icon-pulse"><Connection /></el-icon>
            <span class="signal-card__label">活跃通道</span>
          </div>
          <strong>24</strong>
          <span class="signal-card__hint">当前监控传输边界</span>
        </div>
        <div class="signal-card surface-card">
          <div class="signal-header">
            <el-icon class="icon-spin"><Setting /></el-icon>
            <span class="signal-card__label">布局状态</span>
          </div>
          <strong class="status-unified">统一</strong>
          <span class="signal-card__hint">跨视图共享 Shell</span>
        </div>
        <div class="signal-line"></div>
      </div>
    </section>

    <section class="auth-panel surface-card">
      <div class="auth-panel__head">
        <span class="auth-panel__eyebrow">LOGIN</span>
        <h2>操作员访问</h2>
        <p>请提供受信任凭据以认证并进入工作区</p>
      </div>

      <el-form :model="form" class="auth-form" @keyup.enter="handleLogin">
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
          <el-checkbox v-model="form.agreed" class="premium-checkbox">
            我了解法律及操作审计边界
          </el-checkbox>
          <button type="button" class="inline-link" @click="showDisclaimer = true">阅读通知</button>
        </div>

        <el-button type="primary" class="auth-submit premium-btn" :loading="loading" @click="handleLogin">
          {{ loading ? '验证中...' : '认证并接入' }}
        </el-button>
      </el-form>

      <div class="auth-footer">
        <span>当前节点已接入 Cupcake 主网关，通信已加密。</span>
      </div>

      <el-dialog
        v-model="showDisclaimer"
        title="安全与审计通知"
        width="520px"
        append-to-body
        align-center
        class="premium-dialog"
      >
        <div class="notice-copy">
          <p class="notice-highlight">⚠️ 授权声明</p>
          <p>此管理终端仅供获得书面授权的安全测试及系统合规审计项目使用。</p>
          <p>您在此控制台的所有会话操作（包括传输、命令执行与日志查阅）均会被加密留存并记录于操作日志中。继续进行登录即代表您已知晓并接受相关的审计责任及约束。</p>
        </div>
        <template #footer>
          <el-button type="primary" @click="showDisclaimer = false">我已了解并同意</el-button>
        </template>
      </el-dialog>
    </section>
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

const form = reactive({
  username: '',
  password: '',
  agreed: false
})

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
.auth-shell {
  min-height: 100vh;
  display: grid;
  grid-template-columns: minmax(0, 1.2fr) minmax(380px, 460px);
  gap: 32px;
  padding: 32px;
  position: relative;
  background-color: #fafafa;
  overflow: hidden;
}

/* Premium glows in the background */
.auth-bg-glows {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
  z-index: 0;
}

.glow-1 {
  position: absolute;
  top: -10%;
  left: -10%;
  width: 50%;
  height: 50%;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(17, 17, 17, 0.03) 0%, transparent 70%);
  filter: blur(40px);
}

.glow-2 {
  position: absolute;
  bottom: -10%;
  right: -10%;
  width: 60%;
  height: 60%;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(17, 17, 17, 0.04) 0%, transparent 70%);
  filter: blur(60px);
}

.auth-hero,
.auth-panel {
  min-height: calc(100vh - 64px);
  z-index: 1;
}

.auth-hero {
  position: relative;
  overflow: hidden;
  padding: 48px;
  border-radius: var(--radius-lg);
  background: rgba(255, 255, 255, 0.8);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  color: #111111;
  border: 1px solid rgba(17, 17, 17, 0.05);
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  box-shadow: 0 10px 40px rgba(0, 0, 0, 0.02);
}

.auth-hero::after {
  content: "";
  position: absolute;
  inset: auto -10% -15% auto;
  width: 450px;
  height: 450px;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(17, 17, 17, 0.04) 0%, transparent 70%);
  pointer-events: none;
}

.auth-hero__copy {
  position: relative;
  z-index: 1;
  max-width: 600px;
}

.auth-kicker {
  display: inline-block;
  margin-bottom: 20px;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.25em;
  text-transform: uppercase;
  color: #767676;
}

.auth-hero h1 {
  margin: 0 0 20px;
  font-size: clamp(38px, 5.5vw, 64px);
  line-height: 1.05;
  letter-spacing: -0.04em;
  font-weight: 800;
}

.auth-hero p {
  margin: 0;
  max-width: 520px;
  line-height: 1.8;
  color: #555555;
  font-size: 15px;
}

.auth-signal-grid {
  position: relative;
  z-index: 1;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 20px;
}

.signal-card {
  padding: 24px;
  background: rgba(255, 255, 255, 0.9);
  border: 1px solid rgba(17, 17, 17, 0.06);
  border-radius: var(--radius-md);
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.01);
  transition: transform 0.3s ease, box-shadow 0.3s ease;
}

.signal-card:hover {
  transform: translateY(-4px);
  box-shadow: 0 12px 30px rgba(0, 0, 0, 0.03);
}

.signal-header {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #767676;
  margin-bottom: 12px;
}

.signal-header .el-icon {
  font-size: 16px;
  color: var(--accent);
}

.icon-pulse {
  animation: pulse 2s infinite;
}

.icon-spin {
  animation: spin 8s linear infinite;
}

@keyframes pulse {
  0% { transform: scale(1); opacity: 1; }
  50% { transform: scale(1.15); opacity: 0.7; }
  100% { transform: scale(1); opacity: 1; }
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.signal-card__label {
  font-size: 13px;
  font-weight: 600;
}

.signal-card__hint {
  font-size: 11px;
  color: #888888;
}

.signal-card strong {
  display: block;
  margin: 8px 0 6px;
  font-size: 36px;
  font-weight: 800;
  letter-spacing: -0.03em;
  color: #111111;
}

.status-unified {
  color: #111111;
}

.signal-line {
  grid-column: 1 / -1;
  height: 80px;
  border-radius: 999px;
  background: linear-gradient(90deg, rgba(17, 17, 17, 0.02) 0%, rgba(17, 17, 17, 0.08) 50%, rgba(17, 17, 17, 0.02) 100%);
  mask: linear-gradient(90deg, transparent 0, #000 15%, #000 85%, transparent 100%);
  -webkit-mask: linear-gradient(90deg, transparent 0, #000 15%, #000 85%, transparent 100%);
}

.auth-panel {
  display: flex;
  flex-direction: column;
  justify-content: center;
  padding: 48px;
  border-radius: var(--radius-lg);
  background: #ffffff;
  border: 1px solid rgba(17, 17, 17, 0.06);
  box-shadow: 0 15px 50px rgba(0, 0, 0, 0.04);
}

.auth-panel__head {
  margin-bottom: 32px;
}

.auth-panel__eyebrow {
  display: inline-block;
  margin-bottom: 12px;
  font-size: 11px;
  letter-spacing: 0.25em;
  text-transform: uppercase;
  color: #767676;
  font-weight: 800;
}

.auth-panel__head h2 {
  margin: 0 0 10px;
  font-size: 32px;
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

.auth-form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.premium-input :deep(.el-input__wrapper) {
  padding: 10px 14px;
  border-radius: var(--radius-sm);
  background-color: #fafafa;
  border: 1px solid rgba(17, 17, 17, 0.06);
  box-shadow: none !important;
  transition: all 0.3s ease;
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
  margin: 8px 0 16px;
}

.premium-checkbox :deep(.el-checkbox__label) {
  font-size: 13px;
  color: #555555;
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
}

.inline-link:hover {
  color: #111111;
}

.premium-btn {
  height: 48px;
  border-radius: var(--radius-sm);
  background-color: #111111 !important;
  border-color: #111111 !important;
  font-size: 15px;
  font-weight: 700;
  transition: all 0.2s ease;
}

.premium-btn:hover {
  background-color: #333333 !important;
  border-color: #333333 !important;
  transform: translateY(-1px);
}

.auth-footer {
  margin-top: 36px;
  padding-top: 24px;
  border-top: 1px solid rgba(17, 17, 17, 0.05);
  font-size: 12px;
  color: #999999;
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
  border-radius: var(--radius-md);
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

@media (max-width: 1100px) {
  .auth-shell {
    grid-template-columns: 1fr;
    padding: 20px;
    gap: 20px;
  }

  .auth-hero,
  .auth-panel {
    min-height: auto;
  }

  .auth-hero {
    padding: 32px;
  }
}

@media (max-width: 640px) {
  .auth-shell {
    padding: 12px;
    gap: 12px;
  }

  .auth-panel,
  .auth-hero {
    padding: 24px;
    border-radius: 20px;
  }

  .auth-form__meta {
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }
}
</style>
