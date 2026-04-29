<template>
  <div class="split-view-wrapper">
    <!-- Left Section: Brand & Welcome -->
    <div class="brand-side">
      <div class="brand-content">
        <h2 class="welcome-title animate__animated animate__fadeInLeft">欢迎回来!</h2>
        <p class="welcome-desc animate__animated animate__fadeInLeft animate__delay-1s">
          指挥矩阵已就绪，请输入您的操作员凭据以同步全局状态。
        </p>
        
        <!-- Tech City Graphic -->
        <div class="city-landscape">
          <div class="building b-1"></div>
          <div class="building b-2"></div>
          <div class="building b-3"></div>
          <div class="building b-4"></div>
          <div class="building b-5"></div>
          
          <div class="floating-icons">
            <el-icon class="f-icon i-1"><Monitor /></el-icon>
            <el-icon class="f-icon i-2"><Connection /></el-icon>
            <el-icon class="f-icon i-3"><Cpu /></el-icon>
            <el-icon class="f-icon i-4"><Share /></el-icon>
          </div>
        </div>
      </div>
      <div class="brand-logo-fixed">
        <span class="logo-text">🧁 CUPCAKE</span>
      </div>
    </div>

    <!-- Right Section: Login Form -->
    <div class="form-side">
      <div class="login-form-container animate__animated animate__fadeInRight">
        <h1 class="form-title">登录系统</h1>
        
        <el-form :model="form" class="auth-form" @keyup.enter="handleLogin">
          <div class="line-input-group">
            <label>操作员账号</label>
            <el-input 
              v-model="form.username" 
              placeholder="Username / Email" 
              variant="unstyled"
            />
          </div>

          <div class="line-input-group">
            <label>安全密匙</label>
            <el-input 
              v-model="form.password" 
              type="password" 
              placeholder="Password" 
              show-password
              variant="unstyled"
            />
          </div>

          <div class="form-utils">
            <el-checkbox v-model="form.agreed" size="small">
              我已阅读并同意 <span class="agreed-link" @click.stop="showDisclaimer = true">免责声明</span>
            </el-checkbox>
            <span class="forget-pass">遇到问题?</span>
          </div>

          <el-button 
            type="primary" 
            class="signin-button" 
            :loading="loading" 
            @click="handleLogin"
          >
            <span v-if="!loading">连接中枢</span>
            <span v-else>协议同步中...</span>
          </el-button>
        </el-form>

        <div class="legal-links">
          <span @click="showDisclaimer = true" class="link-item">免责声明</span>
          <span class="dot">•</span>
          <span>使用条款</span>
          <span class="dot">•</span>
          <span>隐私策略</span>
        </div>

        <!-- Disclaimer Dialog -->
        <el-dialog
          v-model="showDisclaimer"
          title="法律免责声明"
          width="500px"
          center
          append-to-body
          class="disclaimer-dialog"
        >
          <div class="disclaimer-content">
            <p><strong>重要提示：</strong></p>
            <p>1. 本系统（Cupcake C2）仅供网络安全从业人员在通过合法授权的渗透测试、内部审计或教育研究中使用。</p>
            <p>2. 严禁利用本系统进行任何未经授权的攻击行为、非法入侵或对他人计算机系统造成破坏。任何违反所在国家/地区法律的行为所产生的后果，均由使用者本人承担。</p>
            <p>3. 开发者不承担因滥用、不当操作或通过本系统进行的非法活动所导致的任何直接或间接法律责任、经济损失及名誉损失。</p>
            <p>4. 登录并使用本系统，即代表您已阅读并同意上述条款，并承诺在法律合规的前提下进行相关操作。</p>
          </div>
          <template #footer>
            <el-button type="primary" @click="showDisclaimer = false">我已了解并同意</el-button>
          </template>
        </el-dialog>
      </div>
    </div>
  </div>
</template>

<script setup>
import { reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Monitor, Connection, Cpu, Share } from '@element-plus/icons-vue'
import api from '../api/index'
import { ElMessage } from 'element-plus'

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
    ElMessage.warning('请输入身份凭据')
    return
  }

  if (!form.agreed) {
    ElMessage.warning('请阅读并勾选免责声明')
    return
  }
  
  loading.value = true
  try {
    const res = await api.post('/api/auth/login', form)
    localStorage.setItem('cupcake_token', res.data.token)
    localStorage.setItem('cupcake_user', JSON.stringify(res.data.user))
    
    ElMessage.success('身份验证成功')
    router.push('/dashboard')
  } catch (e) {
    ElMessage.error(e.response?.data?.error || '凭证错误：拒绝访问')
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.split-view-wrapper {
  height: 100vh;
  width: 100vw;
  display: flex;
  background: #ffffff;
  overflow: hidden;
  font-family: 'Inter', 'PingFang SC', sans-serif;
}

/* Left Section: Purple Brand Side */
.brand-side {
  flex: 0 0 45%;
  background: linear-gradient(135deg, #7c3aed 0%, #6d28d9 100%);
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 60px;
  color: #ffffff;
}

.brand-content {
  max-width: 400px;
  z-index: 10;
  margin-top: -100px;
}

.welcome-title {
  font-size: 48px;
  font-weight: 800;
  margin-bottom: 20px;
  letter-spacing: -1px;
}

.welcome-desc {
  font-size: 16px;
  line-height: 1.6;
  opacity: 0.85;
  font-weight: 400;
}

.brand-logo-fixed {
  position: absolute;
  top: 40px;
  left: 40px;
  font-weight: 900;
  font-size: 20px;
  letter-spacing: 2px;
}

/* Technological Cityscape */
.city-landscape {
  position: absolute;
  bottom: 0;
  left: 0;
  width: 100%;
  height: 300px;
  pointer-events: none;
}

.building {
  position: absolute;
  bottom: 0;
  background: rgba(255, 255, 255, 0.15);
  border-radius: 4px 4px 0 0;
}

.b-1 { width: 60px; height: 180px; left: 10%; }
.b-2 { width: 80px; height: 240px; left: 25%; background: rgba(255, 255, 255, 0.1); }
.b-3 { width: 100px; height: 160px; left: 45%; }
.b-4 { width: 70px; height: 210px; left: 65%; background: rgba(255, 255, 255, 0.12); }
.b-5 { width: 90px; height: 140px; left: 82%; }

.floating-icons {
  position: absolute;
  width: 100%;
  height: 100%;
}

.f-icon {
  position: absolute;
  color: rgba(255, 255, 255, 0.4);
  font-size: 24px;
  animation: floatIcon 6s ease-in-out infinite alternate;
}

.i-1 { top: 40px; left: 15%; }
.i-2 { top: 120px; left: 40%; animation-delay: 1s; }
.i-3 { top: 60px; left: 70%; animation-delay: 2s; }
.i-4 { top: 10px; left: 85%; animation-delay: 0.5s; }

@keyframes floatIcon {
  from { transform: translateY(0) scale(1); opacity: 0.3; }
  to { transform: translateY(-20px) scale(1.1); opacity: 0.6; }
}

/* Right Section: White Form Side */
.form-side {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 60px;
}

.login-form-container {
  width: 100%;
  max-width: 380px;
}

.form-title {
  font-size: 32px;
  font-weight: 700;
  color: #1e293b;
  margin-bottom: 50px;
}

/* Line Pattern Inputs */
.line-input-group {
  margin-bottom: 30px;
}

.line-input-group label {
  display: block;
  font-size: 11px;
  font-weight: 700;
  color: #94a3b8;
  margin-bottom: 5px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

:deep(.el-input__wrapper) {
  padding: 0 !important;
  background-color: transparent !important;
  box-shadow: none !important;
  border-bottom: 1.5px solid #e2e8f0 !important;
  border-radius: 0 !important;
  transition: all 0.3s;
}

:deep(.el-input__wrapper.is-focus) {
  border-bottom-color: #7c3aed !important;
}

:deep(.el-input__inner) {
  height: 40px;
  font-size: 15px;
  color: #1e293b !important;
  padding: 0 !important;
}

.form-utils {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: -10px;
  margin-bottom: 40px;
}

.forget-pass {
  font-size: 11px;
  color: #7c3aed;
  font-weight: 600;
  cursor: pointer;
}

/* Button Stylings */
.signin-button {
  width: 100%;
  height: 48px;
  background-color: #7c3aed !important;
  border: none !important;
  border-radius: 24px !important;
  font-weight: 700;
  letter-spacing: 1px;
  box-shadow: 0 4px 14px rgba(124, 58, 237, 0.35);
  transition: all 0.3s;
}

.signin-button:hover {
  transform: translateY(-2px);
  box-shadow: 0 6px 20px rgba(124, 58, 237, 0.45);
  background-color: #6d28d9 !important;
}

/* Footer Links */
.legal-links {
  margin-top: 80px;
  text-align: center;
  font-size: 11px;
  font-weight: 600;
  color: #94a3b8;
  display: flex;
  justify-content: center;
  gap: 15px;
}

.dot { opacity: 0.3; }

.link-item {
  cursor: pointer;
  transition: color 0.3s;
}

.link-item:hover {
  color: #7c3aed;
}

.agreed-link {
  color: #7c3aed;
  font-weight: 600;
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.disclaimer-content {
  line-height: 1.8;
  color: #475569;
  font-size: 14px;
}

.disclaimer-content p {
  margin-bottom: 12px;
}

:deep(.disclaimer-dialog) {
  border-radius: 12px;
}

:deep(.disclaimer-dialog .el-dialog__header) {
  padding-bottom: 0;
}

:deep(.disclaimer-dialog .el-dialog__title) {
  font-weight: 700;
  color: #1e293b;
}

/* Responsive adjustments */
@media (max-width: 900px) {
  .brand-side { display: none; }
  .form-side { background: #f8fafc; }
}
</style>



