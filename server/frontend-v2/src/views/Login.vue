<template>
  <div class="auth-shell">
    <section class="auth-hero">
      <div class="auth-hero__copy">
        <span class="auth-kicker">Cupcake Console</span>
        <h1>统一界面，统一布局，统一操作节奏。</h1>
        <p>
          The frontend now starts from a cleaner control identity instead of scattered gradients and
          one-off card styles. 登录 to continue into the unified workspace.
        </p>
      </div>

      <div class="auth-signal-grid">
        <div class="signal-card surface-card">
          <span class="signal-card__label">活跃通道</span>
          <strong>24</strong>
          <span class="signal-card__hint">监控下的传输边界</span>
        </div>
        <div class="signal-card surface-card">
          <span class="signal-card__label">布局状态</span>
          <strong>ͳһ</strong>
          <span class="signal-card__hint">跨视图共享 Shell</span>
        </div>
        <div class="signal-line"></div>
      </div>
    </section>

    <section class="auth-panel surface-card">
      <div class="auth-panel__head">
        <span class="auth-panel__eyebrow">登录</span>
        <h2>操作员访问</h2>
        <p>使用您的帐户凭据进入控制工作区。</p>
      </div>

      <el-form :model="form" class="auth-form" @keyup.enter="handleLogin">
        <el-form-item label="用户名">
          <el-input v-model="form.username" placeholder="操作员帐户" />
        </el-form-item>

        <el-form-item label="密码">
          <el-input v-model="form.password" type="password" placeholder="密码" show-password />
        </el-form-item>

        <div class="auth-form__meta">
          <el-checkbox v-model="form.agreed">
            我了解法律和操作限制。
          </el-checkbox>
          <button type="button" class="inline-link" @click="showDisclaimer = true">阅读通知</button>
        </div>

        <el-button type="primary" class="auth-submit" :loading="loading" @click="handleLogin">
          {{ loading ? '登录中...' : '进入工作区' }}
        </el-button>
      </el-form>

      <div class="auth-footer">
        <span>操作员界面刷新已应用于主 Shell。</span>
      </div>

      <el-dialog
        v-model="showDisclaimer"
        title="操作员通知"
        width="520px"
        append-to-body
        class="premium-dialog"
      >
        <div class="notice-copy">
          <p>此界面仅应在您获得明确授权的环境中使用。</p>
          <p>继续操作意味着您接受对合法使用、访问边界和操作审计性的责任。</p>
        </div>
        <template #footer>
          <el-button type="primary" @click="showDisclaimer = false">关闭</el-button>
        </template>
      </el-dialog>
    </section>
  </div>
</template>

<script setup>
import { reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
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
  grid-template-columns: minmax(0, 1.1fr) minmax(360px, 440px);
  gap: 28px;
  padding: 28px;
}

.auth-hero,
.auth-panel {
  min-height: calc(100vh - 56px);
}

.auth-hero {
  position: relative;
  overflow: hidden;
  padding: 42px;
  border-radius: 28px;
  background:
    linear-gradient(180deg, #ffffff 0%, #fafafa 100%);
  color: #111111;
  border: 1px solid #ebebeb;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
}

.auth-hero::after {
  content: "";
  position: absolute;
  inset: auto -12% -20% auto;
  width: 420px;
  height: 420px;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(17, 17, 17, 0.05), transparent 68%);
}

.auth-hero__copy {
  position: relative;
  z-index: 1;
  max-width: 560px;
}

.auth-kicker {
  display: inline-block;
  margin-bottom: 18px;
  font-size: 11px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  color: #767676;
}

.auth-hero h1 {
  margin: 0 0 16px;
  font-size: clamp(46px, 7vw, 74px);
  line-height: 0.94;
  letter-spacing: -0.06em;
}

.auth-hero p {
  margin: 0;
  max-width: 480px;
  line-height: 1.8;
  color: #4d4d4d;
}

.auth-signal-grid {
  position: relative;
  z-index: 1;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 220px));
  gap: 18px;
  align-content: end;
}

.signal-card {
  padding: 22px;
  background: #ffffff;
  border-color: #ebebeb;
  color: #111111;
}

.signal-card__label,
.signal-card__hint {
  font-size: 12px;
  color: #767676;
}

.signal-card strong {
  display: block;
  margin: 12px 0 8px;
  font-size: 30px;
  letter-spacing: -0.05em;
}

.signal-line {
  grid-column: 1 / -1;
  height: 110px;
  border-radius: 999px;
  background:
    linear-gradient(90deg, rgba(17, 17, 17, 0.02), transparent 45%),
    linear-gradient(180deg, rgba(17, 17, 17, 0.2), rgba(17, 17, 17, 0.02));
  mask: linear-gradient(90deg, transparent 0, #000 12%, #000 88%, transparent 100%);
}

.auth-panel {
  display: flex;
  flex-direction: column;
  justify-content: center;
  padding: 36px;
}

.auth-panel__head {
  margin-bottom: 28px;
}

.auth-panel__eyebrow {
  display: inline-block;
  margin-bottom: 10px;
  font-size: 11px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--accent-strong);
  font-weight: 700;
}

.auth-panel__head h2 {
  margin: 0 0 8px;
  font-size: 34px;
  letter-spacing: -0.05em;
}

.auth-panel__head p,
.auth-footer {
  margin: 0;
  color: var(--text-body);
  line-height: 1.7;
}

.auth-form {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.auth-form__meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  margin: 4px 0 12px;
}

.inline-link {
  border: 0;
  background: none;
  padding: 0;
  color: #111111;
  font: inherit;
  font-weight: 700;
  cursor: pointer;
}

.auth-submit {
  width: 100%;
  height: 48px;
}

.auth-footer {
  margin-top: 20px;
  padding-top: 18px;
  border-top: 1px solid var(--line-soft);
  font-size: 13px;
}

.notice-copy {
  display: flex;
  flex-direction: column;
  gap: 12px;
  line-height: 1.7;
  color: var(--text-body);
}

@media (max-width: 1100px) {
  .auth-shell {
    grid-template-columns: 1fr;
    padding: 18px;
  }

  .auth-hero,
  .auth-panel {
    min-height: auto;
  }

  .auth-hero {
    padding: 28px;
  }

  .auth-signal-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 640px) {
  .auth-shell {
    padding: 12px;
  }

  .auth-panel,
  .auth-hero {
    padding: 24px;
    border-radius: 24px;
  }

  .auth-form__meta {
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
