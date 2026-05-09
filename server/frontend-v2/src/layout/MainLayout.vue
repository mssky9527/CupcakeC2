<template>
  <div class="control-shell" :class="{ 'control-shell--mobile-nav': isMobileNavOpen, 'control-shell--collapsed': isSidebarCollapsed }">
    <button v-if="isCompact" class="mobile-nav-toggle" type="button" @click="isMobileNavOpen = !isMobileNavOpen">
      <el-icon><Operation /></el-icon>
    </button>

    <aside class="control-sidebar" :class="{ collapsed: isSidebarCollapsed }">
      <div class="sidebar-brand">
        <div class="brand-mark" @click="toggleSidebar" style="cursor:pointer" title="收起/展开">
          <el-icon><Grid /></el-icon>
        </div>
        <div class="brand-copy" v-show="!isSidebarCollapsed">
          <span class="brand-kicker">Cupcake Console</span>
          <strong class="brand-title">Unified Control</strong>
          <span class="brand-subtitle">Operations workspace</span>
        </div>
      </div>

      <div class="sidebar-section">
        <div class="sidebar-label" v-show="!isSidebarCollapsed">导航</div>
        <el-menu :default-active="activeMenu" class="sidebar-menu" router>
          <el-menu-item v-for="item in menuItems" :key="item.path" :index="item.path" @click="isMobileNavOpen = false">
            <el-icon><component :is="item.icon" /></el-icon>
            <template #title><span>{{ item.label }}</span></template>
          </el-menu-item>
        </el-menu>
      </div>

      <div class="sidebar-foot" v-show="!isSidebarCollapsed">
        <div class="user-panel">
          <el-avatar :size="42" class="user-avatar">{{ userInitial }}</el-avatar>
          <div class="user-meta">
            <strong>{{ username }}</strong>
            <span>已验证的操作员</span>
          </div>
        </div>

        <div class="foot-actions">
          <button type="button" class="foot-link" @click="openPasswordDialog">
            <el-icon><Lock /></el-icon>
            <span>安全</span>
          </button>
          <button type="button" class="foot-link foot-link--danger" @click="handleLogout">
            <el-icon><SwitchButton /></el-icon>
            <span>注销</span>
          </button>
        </div>
      </div>

      <!-- Collapsed footer: just avatar and logout icon -->
      <div class="sidebar-foot-collapsed" v-show="isSidebarCollapsed">
        <el-avatar :size="36" class="user-avatar">{{ userInitial }}</el-avatar>
        <button type="button" class="foot-link-icon" @click="handleLogout" title="注销">
          <el-icon><SwitchButton /></el-icon>
        </button>
      </div>
    </aside>

    <div v-if="isCompact && isMobileNavOpen" class="mobile-overlay" @click="isMobileNavOpen = false"></div>

    <main class="control-main">
      <header class="control-header">
        <div class="header-copy">
          <h1 class="header-title">{{ currentTitle }}</h1>
          <p class="header-description">{{ currentDescription }}</p>
        </div>

        <div class="header-meta">
          <div class="header-chip">
            <span class="chip-dot"></span>
            <span>控制平面在线</span>
          </div>
          <div class="header-chip">
            <el-icon><Clock /></el-icon>
            <span>{{ currentDate }}</span>
          </div>
          <div class="header-clock">{{ currentTime }}</div>
        </div>
      </header>

      <section class="control-content">
        <router-view v-slot="{ Component }">
          <transition name="layout-fade" mode="out-in">
            <keep-alive>
              <component :is="Component" />
            </keep-alive>
          </transition>
        </router-view>
      </section>
    </main>

    <el-dialog v-model="pwdDialog.visible" title="修改密码" width="420px" class="premium-dialog" append-to-body>
      <el-form :model="pwdDialog.form" label-position="top">
        <el-form-item label="当前密码">
          <el-input v-model="pwdDialog.form.oldPassword" type="password" show-password />
        </el-form-item>
        <el-form-item label="新密码">
          <el-input v-model="pwdDialog.form.newPassword" type="password" show-password />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="pwdDialog.visible = false">取消</el-button>
        <el-button type="primary" @click="submitChangePassword" :loading="pwdDialog.loading">保存</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  Clock,
  Connection,
  Grid,
  Headset,
  Lightning,
  Lock,
  Monitor,
  Odometer,
  Operation,
  Setting,
  Share,
  SwitchButton
} from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import api from '../api/index'

const route = useRoute()
const router = useRouter()

const menuItems = [
  { path: '/dashboard', label: '仪表盘', icon: Odometer },
  { path: '/clients', label: '受控端', icon: Monitor },
  { path: '/listeners', label: '监听器', icon: Headset },
  { path: '/tunnels', label: '隧道', icon: Share },
  { path: '/generator', label: '生成器', icon: Lightning },
  { path: '/domain', label: '插件', icon: Connection },
  { path: '/settings', label: '设置', icon: Setting }
]

const titleDescriptions = {
  Dashboard: '',
  '仪表盘': '',
  Clients: '',
  '受控端': '',
  Listeners: '',
  '监听器': '',
  Tunnels: '',
  '隧道': '',
  Generator: '',
  '生成器': '',
  Plugins: '',
  '插件': '',
  Settings: '',
  '设置': '',
  'Client Detail': '',
  Terminal: '',
  Files: '',
  Processes: '',
  'Client Tunnels': '',
  'Client Plugins': ''
}

const userData = JSON.parse(localStorage.getItem('cupcake_user') || '{}')
const username = ref(userData.username || 'Operator')
const userInitial = computed(() => username.value.charAt(0).toUpperCase())
const activeMenu = computed(() => route.path.startsWith('/client/') ? '/clients' : route.path)

const currentTitle = computed(() => route.meta.title || '仪表盘')
const currentDescription = computed(() => titleDescriptions[currentTitle.value] || '具有单一布局和共享视觉系统的统一操作员工作流。')

const currentTime = ref('')
const currentDate = ref('')
const viewportWidth = ref(typeof window === 'undefined' ? 1440 : window.innerWidth)
const isMobileNavOpen = ref(false)
const isCompact = computed(() => viewportWidth.value < 1080)
const isSidebarCollapsed = ref(false)

const toggleSidebar = () => {
  isSidebarCollapsed.value = !isSidebarCollapsed.value
}

const pwdDialog = reactive({
  visible: false,
  loading: false,
  form: { oldPassword: '', newPassword: '' }
})

let clockTimer = null

const syncClock = () => {
  const now = new Date()
  currentTime.value = now.toLocaleTimeString('en-GB', { hour12: false })
  currentDate.value = now.toLocaleDateString('en-GB', {
    weekday: 'short',
    year: 'numeric',
    month: 'short',
    day: 'numeric'
  })
}

const handleResize = () => {
  viewportWidth.value = window.innerWidth
  if (!isCompact.value) {
    isMobileNavOpen.value = false
  }
}

const openPasswordDialog = () => {
  pwdDialog.form.oldPassword = ''
  pwdDialog.form.newPassword = ''
  pwdDialog.visible = true
}

const submitChangePassword = async () => {
  if (!pwdDialog.form.oldPassword || !pwdDialog.form.newPassword) {
    ElMessage.warning('请填写所有密码字段。')
    return
  }

  pwdDialog.loading = true
  try {
    const userId = userData.id
    await api.put(`/api/settings/users/${userId}`, { password: pwdDialog.form.newPassword })
    ElMessage.success('密码已更新。')
    pwdDialog.visible = false
  } catch {
    ElMessage.error('密码更新失败。')
  } finally {
    pwdDialog.loading = false
  }
}

const handleLogout = () => {
  ElMessageBox.confirm('结束当前会话并返回登录页面？', '确认注销', {
    type: 'warning',
    confirmButtonText: '注销',
    cancelButtonText: '取消'
  }).then(() => {
    localStorage.removeItem('cupcake_token')
    localStorage.removeItem('cupcake_user')
    router.push('/login')
  }).catch(() => {})
}

onMounted(() => {
  syncClock()
  clockTimer = window.setInterval(syncClock, 1000)
  window.addEventListener('resize', handleResize)
})

onBeforeUnmount(() => {
  if (clockTimer) {
    window.clearInterval(clockTimer)
  }
  window.removeEventListener('resize', handleResize)
})
</script>

<style scoped>
.control-shell {
  display: grid;
  grid-template-columns: 296px minmax(0, 1fr);
  height: 100vh;
  min-height: 100vh;
  overflow: hidden;
  position: relative;
  transition: grid-template-columns 0.25s ease;
}

.control-shell--collapsed {
  grid-template-columns: 72px minmax(0, 1fr);
}

.control-sidebar {
  position: relative;
  z-index: 4;
  display: flex;
  flex-direction: column;
  gap: 24px;
  padding: 28px 22px 22px;
  color: var(--text-strong);
  background: var(--bg-sidebar);
  border-right: 1px solid var(--line-soft);
  transition: padding 0.25s ease, width 0.25s ease;
  overflow: hidden;
}

.control-sidebar.collapsed {
  padding: 28px 0 22px;
  width: 72px;
  align-items: center;
}

.control-sidebar.collapsed .sidebar-brand {
  justify-content: center;
  padding: 0;
}

.control-sidebar.collapsed .sidebar-section {
  width: 100%;
  padding: 0;
}

.control-sidebar.collapsed .sidebar-menu {
  width: 100% !important;
}

.control-sidebar.collapsed :deep(.el-menu) {
  width: 100% !important;
  border-right: none !important;
  background: transparent !important;
}

.control-sidebar.collapsed :deep(.el-menu-item) {
  height: 44px !important;
  line-height: 44px !important;
  padding: 0 !important;
  padding-left: 0 !important;
  margin: 4px 0 !important;
  display: flex !important;
  justify-content: center !important;
  align-items: center !important;
}

.control-sidebar.collapsed :deep(.el-menu-item .el-icon) {
  margin: 0 !important;
  font-size: 20px;
}

.control-sidebar.collapsed :deep(.el-menu-item span),
.control-sidebar.collapsed :deep(.el-menu-item .el-menu-tooltip__trigger) {
  display: none !important;
}

.sidebar-brand {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 8px 6px;
}

.brand-mark {
  width: 52px;
  height: 52px;
  display: grid;
  place-items: center;
  border-radius: 18px;
  color: #111111;
  background: #f2f2f2;
  box-shadow: none;
}

.brand-copy {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.brand-kicker,
.sidebar-label {
  font-size: 11px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.brand-title {
  font-size: 18px;
  letter-spacing: -0.03em;
}

.brand-subtitle {
  font-size: 13px;
  color: var(--text-muted);
}

.sidebar-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.sidebar-menu :deep(.el-menu-item) {
  height: 48px;
  margin: 4px 0;
  border-radius: 16px;
  color: var(--text-body) !important;
  font-weight: 700;
}

.sidebar-menu :deep(.el-menu-item:hover) {
  background: var(--bg-sidebar-soft) !important;
  color: var(--text-strong) !important;
}

.sidebar-menu :deep(.el-menu-item.is-active) {
  background: #f4f4f4 !important;
  color: var(--text-strong) !important;
  box-shadow: inset 0 0 0 1px #dddddd;
}

.sidebar-menu :deep(.el-menu-item .el-icon) {
  margin-right: 10px;
  font-size: 18px;
}

.sidebar-foot {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.user-panel {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px;
  border-radius: 20px;
  background: #fafafa;
  border: 1px solid #ececec;
}

.user-avatar {
  background: #111111;
  color: #fff;
  font-weight: 700;
}

.user-meta {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.user-meta strong {
  font-size: 14px;
}

.user-meta span {
  font-size: 12px;
  color: var(--text-muted);
}

.foot-actions {
  display: grid;
  gap: 10px;
}

.foot-link {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 12px 14px;
  border: 0;
  border-radius: 16px;
  color: var(--text-body);
  background: #fafafa;
  cursor: pointer;
  transition: background 0.16s ease, transform 0.16s ease;
}

.foot-link:hover {
  background: #f2f2f2;
  transform: translateY(-1px);
}

.foot-link--danger:hover {
  color: #111111;
  background: #f2f2f2;
}

.sidebar-foot-collapsed {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.foot-link-icon {
  width: 36px;
  height: 36px;
  border: 0;
  border-radius: 10px;
  background: #fafafa;
  color: var(--text-body);
  cursor: pointer;
  display: grid;
  place-items: center;
  transition: background 0.15s;
}

.foot-link-icon:hover {
  background: #f0f0f0;
}

/* Collapsed: el-menu tooltip hide */
:deep(.el-menu--collapse .el-sub-menu__icon-arrow) {
  display: none;
}

.control-main {
  min-width: 0;
  min-height: 0;
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: 22px;
}

.control-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 20px;
  padding: 10px 8px 24px;
}

.header-copy {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.header-kicker {
  font-size: 11px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--accent-strong);
  font-weight: 700;
}

.header-title {
  margin: 0;
  font-size: 24px;
  line-height: 1.2;
  letter-spacing: -0.02em;
}

.header-description {
  max-width: 760px;
  margin: 0;
  color: var(--text-body);
  line-height: 1.6;
  font-size: 13px;
}

.header-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.header-chip {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-height: 40px;
  padding: 0 14px;
  border-radius: 999px;
  background: #ffffff;
  border: 1px solid var(--line-soft);
  color: var(--text-body);
  font-size: 12px;
  font-weight: 700;
}

.chip-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: #111111;
  box-shadow: none;
}

.header-clock {
  display: inline-flex;
  align-items: center;
  min-height: 40px;
  padding: 0 16px;
  border-radius: 999px;
  background: #f5f5f5;
  color: var(--text-strong);
  font-weight: 800;
  letter-spacing: 0.08em;
}

.control-content {
  min-height: 0;
  flex: 1;
  overflow-x: hidden;
  overflow-y: auto;
  padding-bottom: 24px;
}

.layout-fade-enter-active,
.layout-fade-leave-active {
  transition: opacity 0.12s ease;
}

.layout-fade-enter-from,
.layout-fade-leave-to {
  opacity: 0;
}

.mobile-nav-toggle {
  position: fixed;
  top: 20px;
  left: 20px;
  z-index: 6;
  width: 44px;
  height: 44px;
  border: 0;
  border-radius: 14px;
  background: #111111;
  color: #fff;
  cursor: pointer;
  box-shadow: var(--shadow-panel);
}

.mobile-overlay {
  position: fixed;
  inset: 0;
  z-index: 3;
  background: rgba(17, 17, 17, 0.14);
  backdrop-filter: blur(4px);
}

@media (max-width: 1079px) {
  .control-shell {
    grid-template-columns: 1fr;
  }

  .control-sidebar {
    position: fixed;
    inset: 0 auto 0 0;
    width: min(320px, 84vw);
    transform: translateX(-105%);
    transition: transform 0.2s ease;
  }

  .control-shell--mobile-nav .control-sidebar {
    transform: translateX(0);
  }

  .control-main {
    padding-top: 84px;
  }

  .control-header {
    flex-direction: column;
  }

  .header-meta {
    justify-content: flex-start;
  }
}

@media (max-width: 720px) {
  .control-main {
    padding-left: 14px;
    padding-right: 14px;
  }

  .header-title {
    font-size: 34px;
  }
}
</style>
