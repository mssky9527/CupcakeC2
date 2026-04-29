<template>
  <el-container class="layout-container">
    <!-- Sidebar Navigation -->
    <el-aside width="260px" class="sidebar">
      <!-- Logo / Branding -->
      <div class="logo-area">
        <div class="logo-orb">
          <el-icon><Lightning /></el-icon>
        </div>
        <div class="logo-info">
          <span class="logo-text">CUPCAKE <span class="v2-tag">V2</span></span>
          <span class="logo-sub">高级 C2 指挥平台</span>
        </div>
      </div>
      
      <!-- Navigation Menu -->
      <el-menu
        :default-active="activeMenu"
        class="sidebar-menu"
        router
      >
        <el-menu-item index="/dashboard">
          <el-icon><Odometer /></el-icon>
          <span>仪表盘</span>
        </el-menu-item>
        <el-menu-item index="/clients">
          <el-icon><Monitor /></el-icon>
          <span>终端管理</span>
        </el-menu-item>
        <el-menu-item index="/listeners">
          <el-icon><Headset /></el-icon>
          <span>监听链路</span>
        </el-menu-item>
        <el-menu-item index="/tunnels">
          <el-icon><Share /></el-icon>
          <span>隧道路由</span>
        </el-menu-item>
        <el-menu-item index="/generator">
          <el-icon><Lightning /></el-icon>
          <span>载荷投放</span>
        </el-menu-item>
        <el-menu-item index="/domain">
          <el-icon><Connection /></el-icon>
          <span>插件生态</span>
        </el-menu-item>
        <el-menu-item index="/settings">
          <el-icon><Setting /></el-icon>
          <span>核心配置</span>
        </el-menu-item>
      </el-menu>

      <!-- Sidebar Footer -->
      <div class="sidebar-footer">
        <div class="user-card-mini">
           <el-avatar :size="32" class="mini-avatar">{{ userInitial }}</el-avatar>
           <div class="mini-info">
             <div class="mini-name">{{ username }}</div>
             <div class="mini-role">系统管理员</div>
           </div>
           <el-button link class="logout-mini" @click="handleLogout">
              <el-icon><SwitchButton /></el-icon>
           </el-button>
        </div>
        <div class="build-info">V3.0.5 生产环境 | TIAMO</div>
      </div>
    </el-aside>

    <!-- Main Content Container -->
    <el-container class="main-container">
      <!-- Top Header Bar -->
      <el-header class="header">
        <div class="header-left">
           <div class="current-path">
              <span class="path-parent">CUPCAKE</span>
              <span class="path-sep">/</span>
              <span class="path-child">{{ $route.meta.title || '仪表盘' }}</span>
           </div>
        </div>
        <div class="header-right">
          <div class="status-chip">
             <span class="status-dot"></span>
             服务正常
          </div>
          <el-dropdown trigger="click" @command="handleCommand">
            <div class="profile-trigger">
              <el-icon><ArrowDown /></el-icon>
            </div>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="password">
                  <el-icon><Lock /></el-icon>安全中心
                </el-dropdown-item>
                <el-dropdown-item command="logout" divided>
                  <el-icon><SwitchButton /></el-icon>退出系统
                </el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </div>
      </el-header>

      <!-- Main Content Area -->
      <el-main class="main-content">
        <router-view v-slot="{ Component }">
          <transition name="page" mode="out-in">
            <keep-alive>
              <component :is="Component" />
            </keep-alive>
          </transition>
        </router-view>
      </el-main>
    </el-container>

    <!-- Change Password Dialog -->
    <el-dialog v-model="pwdDialog.visible" title="安全设置 - 身份验证" width="420px" class="premium-dialog" append-to-body>
      <el-form :model="pwdDialog.form" label-position="top">
        <el-form-item label="原始密码">
          <el-input v-model="pwdDialog.form.oldPassword" type="password" show-password />
        </el-form-item>
        <el-form-item label="设定新密码">
          <el-input v-model="pwdDialog.form.newPassword" type="password" show-password />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="pwdDialog.visible = false">取消</el-button>
        <el-button type="primary" class="purple-btn" @click="submitChangePassword" :loading="pwdDialog.loading">同步变更</el-button>
      </template>
    </el-dialog>
  </el-container>
</template>

<script setup>
import { ref, computed, reactive, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { 
  Odometer, Monitor, Headset, Share, Lightning, 
  Connection, Setting, ArrowDown,
  Lock, SwitchButton
} from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import api from '../api/index'

const route = useRoute()
const router = useRouter()

const userData = JSON.parse(localStorage.getItem('cupcake_user') || '{}')
const username = ref(userData.username || 'Admin')
const userInitial = computed(() => username.value.charAt(0).toUpperCase())

const activeMenu = computed(() => {
  const path = route.path
  if (path.startsWith('/client/')) return '/clients'
  return path
})

const pwdDialog = reactive({
  visible: false,
  loading: false,
  form: { oldPassword: '', newPassword: '', confirmPassword: '' }
})

const handleCommand = (command) => {
  if (command === 'password') {
    pwdDialog.form = { oldPassword: '', newPassword: '', confirmPassword: '' }
    pwdDialog.visible = true
  } else if (command === 'logout') {
    handleLogout()
  }
}

const submitChangePassword = async () => {
  if (!pwdDialog.form.oldPassword || !pwdDialog.form.newPassword) return ElMessage.warning('请填写完整信息')
  pwdDialog.loading = true
  try {
    const userId = userData.id
    await api.put(`/api/settings/users/${userId}`, { password: pwdDialog.form.newPassword })
    ElMessage.success('密码同步成功')
    pwdDialog.visible = false
  } catch (e) { ElMessage.error('变更失败') }
  finally { pwdDialog.loading = false }
}

const handleLogout = () => {
  ElMessageBox.confirm('确定结束任务并退出系统吗？', '安全提示', {
    type: 'warning', confirmButtonText: '确定', cancelButtonText: '取消'
  }).then(() => {
    localStorage.removeItem('cupcake_token')
    localStorage.removeItem('cupcake_user')
    router.push('/login')
  }).catch(() => {})
}
</script>

<style scoped>
.layout-container {
  height: 100vh;
  width: 100vw;
  background-color: #f8fafc;
  font-family: 'Inter', sans-serif;
}

/* Sidebar Styling */
.sidebar {
  background: #ffffff;
  border-right: 1px solid rgba(124, 58, 237, 0.08);
  display: flex;
  flex-direction: column;
  z-index: 100;
  box-shadow: 4px 0 20px rgba(0, 0, 0, 0.01);
}

.logo-area {
  height: 90px;
  display: flex;
  align-items: center;
  padding: 0 24px;
  gap: 14px;
}

.logo-orb {
  width: 44px; height: 44px;
  background: linear-gradient(135deg, #7c3aed, #a855f7);
  border-radius: 14px;
  display: flex; align-items: center; justify-content: center;
  color: white; font-size: 20px;
  box-shadow: 0 8px 20px rgba(124, 58, 237, 0.3);
}

.logo-info { display: flex; flex-direction: column; }
.logo-text { font-size: 18px; font-weight: 900; color: #1e1b4b; letter-spacing: -0.5px; }
.v2-tag { color: #7c3aed; font-size: 14px; }
.logo-sub { font-size: 10px; font-weight: 800; color: #94a3b8; letter-spacing: 1px; }

.sidebar-menu {
  flex: 1;
  border-right: none;
  padding: 10px 0;
}

:deep(.el-menu-item) {
  height: 50px;
  line-height: 50px;
  margin: 4px 16px;
  border-radius: 12px;
  font-size: 13px;
  font-weight: 700;
  color: #64748b;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}

:deep(.el-menu-item:hover) {
  background: rgba(124, 58, 237, 0.04) !important;
  color: #7c3aed !important;
}

:deep(.el-menu-item.is-active) {
  background: rgba(124, 58, 237, 0.08) !important;
  color: #7c3aed !important;
  box-shadow: 0 4px 12px rgba(124, 58, 237, 0.05);
}

:deep(.el-menu-item .el-icon) {
  font-size: 18px;
  margin-right: 12px;
}

/* Sidebar Footer */
.sidebar-footer {
  padding: 20px;
  border-top: 1px solid rgba(124, 58, 237, 0.05);
}

.user-card-mini {
  display: flex;
  align-items: center;
  padding: 12px;
  background: #f8fafc;
  border-radius: 16px;
  gap: 12px;
  margin-bottom: 20px;
}

.mini-avatar { background: #7c3aed; color: white; font-weight: 800; }
.mini-info { flex: 1; }
.mini-name { font-size: 13px; font-weight: 800; color: #1e1b4b; }
.mini-role { font-size: 11px; color: #94a3b8; font-weight: 600; }
.logout-mini { color: #94a3b8; font-size: 18px; }
.logout-mini:hover { color: #f43f5e; }

.build-info { font-size: 9px; font-weight: 800; color: #cbd5e1; text-align: center; letter-spacing: 0.5px; }

/* Header */
.header {
  height: 70px;
  background: transparent;
  display: flex; align-items: center; justify-content: space-between;
  padding: 0 32px;
}

.current-path { display: flex; align-items: center; gap: 8px; font-weight: 800; }
.path-parent { color: #94a3b8; font-size: 13px; }
.path-sep { color: #cbd5e1; }
.path-child { color: #1e1b4b; font-size: 15px; }

.header-right { display: flex; align-items: center; gap: 20px; }
.status-chip {
  display: flex; align-items: center; gap: 8px;
  background: white; padding: 8px 16px; border-radius: 12px;
  font-size: 11px; font-weight: 900; color: #10b981;
  box-shadow: 0 4px 12px rgba(16, 185, 129, 0.05);
}
.status-dot { width: 6px; height: 6px; background: #10b981; border-radius: 50%; animation: pulse 2s infinite; }

.profile-trigger {
  width: 32px; height: 32px;
  background: white; border-radius: 10px;
  display: flex; align-items: center; justify-content: center;
  cursor: pointer; color: #94a3b8;
  box-shadow: 0 4px 10px rgba(0, 0, 0, 0.02);
  transition: all 0.2s;
}
.profile-trigger:hover { color: #7c3aed; transform: translateY(-1px); }

@keyframes pulse { 0% { opacity: 1; scale: 1; } 50% { opacity: 0.6; scale: 1.2; } 100% { opacity: 1; scale: 1; } }

/* Main Content */
.main-content {
  padding: 0 32px 32px 32px;
  overflow-y: auto;
}

/* Page Transitions */
.page-enter-active, .page-leave-active { transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1); }
.page-enter-from { opacity: 0; transform: translateY(10px); }
.page-leave-to { opacity: 0; transform: translateY(-10px); }

/* Dialog Styles */
.premium-dialog :deep(.el-dialog) { border-radius: 24px; padding: 10px; }
.purple-btn { background: #7c3aed !important; border: none !important; font-weight: 800; height: 40px; border-radius: 10px; }
</style>
