<template>
  <div class="view-shell client-shell" @click="closeMenu">
    <section class="view-actions client-actions">
      <el-button @click="fetchClients">
        <el-icon><Refresh /></el-icon>
        刷新
      </el-button>
      <el-button type="primary" @click="openConnectDialog">
        <el-icon><Connection /></el-icon>
        正向 TCP 接入
      </el-button>
    </section>

    <section class="stat-grid">
      <article class="surface-card stat-card">
        <div class="stat-card__icon">
          <el-icon><Monitor /></el-icon>
        </div>
        <div>
          <span class="stat-card__label">注册主机</span>
          <div class="stat-card__value">{{ clients.length }}</div>
        </div>
      </article>

      <article class="surface-card stat-card">
        <div class="stat-card__icon">
          <el-icon><CircleCheck /></el-icon>
        </div>
        <div>
          <span class="stat-card__label">在线主机</span>
          <div class="stat-card__value">{{ onlineCount }}</div>
        </div>
      </article>

      <article class="surface-card stat-card">
        <div class="stat-card__icon">
          <el-icon><Promotion /></el-icon>
        </div>
        <div>
          <span class="stat-card__label">内存驻留</span>
          <div class="stat-card__value">{{ memoryCount }}</div>
        </div>
      </article>
    </section>

    <section class="surface-card table-shell client-table-card">
      <div class="panel-head" style="display:none">
        <div>
          <span class="panel-kicker">Inventory</span>
          <h3>主机清单</h3>
        </div>
        <div class="chip">右键查看更多操作</div>
      </div>

      <el-table
        :data="clients"
        class="premium-table"
        v-loading="loading"
        @row-contextmenu="openContextMenu"
      >
        <el-table-column width="64" align="center">
          <template #default="{ row }">
            <div class="os-icon" :class="getOsClass(row.os)">
              <el-icon v-if="row.os?.toLowerCase().includes('win')"><Platform /></el-icon>
              <el-icon v-else><ChromeFilled /></el-icon>
            </div>
          </template>
        </el-table-column>

        <el-table-column prop="hostname" label="主机名" min-width="190">
          <template #default="{ row }">
            <div class="hostname-cell">
              <span class="hostname-text">{{ row.hostname || 'Unknown Host' }}</span>
              <span class="uuid-label mono">{{ row.uuid?.substring(0, 8) || '--------' }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column prop="ip" label="IP 地址" width="150">
          <template #default="{ row }">
            <span class="mono">{{ row.ip || '--' }}</span>
          </template>
        </el-table-column>

        <el-table-column prop="os" label="系统" width="150">
          <template #default="{ row }">
            <el-tag :type="getOsTag(row.os)" effect="plain" round>
              {{ row.os || 'Unknown' }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column prop="username" label="当前用户" min-width="150" show-overflow-tooltip>
          <template #default="{ row }">
            <div class="user-cell">
              <el-icon><User /></el-icon>
              <span>{{ row.username || '--' }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column prop="last_seen" label="最后心跳" width="180" sortable>
          <template #default="{ row }">
            <span class="muted">{{ formatTime(row.last_seen) }}</span>
          </template>
        </el-table-column>

        <el-table-column label="状态" fixed="right" width="120" align="center">
          <template #default="{ row }">
            <div class="status-indicator" :class="getStatusClass(row.status)">
              <span class="dot"></span>
              {{ getStatusLabel(row.status) }}
            </div>
          </template>
        </el-table-column>

        <el-table-column label="进入" fixed="right" width="88" align="center">
          <template #default="{ row }">
            <el-button type="primary" link class="manage-btn" @click="handleManageRow(row)">
              <el-icon><ArrowRight /></el-icon>
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </section>

    <el-dialog
      title="正向 TCP 资产接入"
      v-model="connectDialogVisible"
      width="500px"
      class="premium-dialog"
      center
    >
      <el-form label-position="top">
        <el-form-item label="目标地址" required>
          <el-input v-model="connectForm.target_addr" placeholder="10.0.0.5:4444" :prefix-icon="MapLocation" />
        </el-form-item>

        <el-form-item label="关联监听器" required>
          <el-select v-model="connectForm.listener_id" placeholder="选择正向 TCP 监听器" style="width: 100%">
            <el-option
              v-for="listener in bindListeners"
              :key="listener.id"
              :label="`${listener.protocol} | Port ${listener.port}`"
              :value="listener.id"
            />
          </el-select>
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="connectDialogVisible = false">关闭</el-button>
        <el-button type="primary" :loading="connecting" @click="handleConnect">发送接入指令</el-button>
      </template>
    </el-dialog>

    <div v-if="contextMenu.visible" :style="contextMenuStyle" class="premium-context-menu">
      <div class="menu-header">{{ contextMenu.row?.hostname || 'Host' }}</div>

      <button
        type="button"
        class="menu-item"
        :class="{ disabled: !isAgentOnline(contextMenu.row?.status) }"
        @click="handleManageByContext"
      >
        <el-icon><Monitor /></el-icon>
        进入终端
      </button>

      <button
        type="button"
        class="menu-item"
        @click="openConnectFromContext"
      >
        <el-icon><Connection /></el-icon>
        正向 TCP 接入
      </button>

      <div class="menu-divider"></div>

      <button type="button" class="menu-item menu-item--danger" @click="handleDeleteByContext">
        <el-icon><Delete /></el-icon>
        删除记录
      </button>
    </div>
  </div>
</template>

<script setup>
import { computed, onMounted, onUnmounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  ArrowRight,
  ChromeFilled,
  CircleCheck,
  Connection,
  Delete,
  MapLocation,
  Monitor,
  Platform,
  Promotion,
  Refresh,
  User
} from '@element-plus/icons-vue'
import api, { deleteClient } from '../api/index'

const router = useRouter()
const clients = ref([])
const loading = ref(false)
let timer = null

const onlineCount = computed(() => clients.value.filter((client) => client.status === 'online' || client.status === 'memory_online').length)
const memoryCount = computed(() => clients.value.filter((client) => client.status === 'memory_online' || client.status === 'memory_offline').length)

const isAgentOnline = (status) => status === 'online' || status === 'memory_online'

const getStatusClass = (status) => {
  if (status === 'online') return 'online'
  if (status === 'memory_online') return 'memory-online'
  if (status === 'memory_offline') return 'memory-offline'
  return 'offline'
}

const getStatusLabel = (status) => {
  if (status === 'online') return '在线'
  if (status === 'memory_online') return '内存在线'
  if (status === 'memory_offline') return '内存离线'
  return '离线'
}

const contextMenu = reactive({ visible: false, x: 0, y: 0, row: null })
const contextMenuStyle = computed(() => ({ top: `${contextMenu.y}px`, left: `${contextMenu.x}px` }))

const connectDialogVisible = ref(false)
const connecting = ref(false)
const listeners = ref([])
const connectForm = reactive({ target_addr: '', listener_id: '' })

const bindListeners = computed(() =>
  listeners.value.filter((item) => item.protocol === '正向TCP' || item.protocol === 'Bind-TCP')
)

const fetchClients = async () => {
  loading.value = true
  try {
    const res = await api.get('/api/clients')
    clients.value = res.data || []
  } catch {
    ElMessage.error('获取主机列表失败')
  } finally {
    loading.value = false
  }
}

const handleManageRow = (row) => {
  router.push({ name: 'ClientDetail', params: { id: row.uuid } })
}

const getOsClass = (os) => {
  const value = String(os || '').toLowerCase()
  if (value.includes('win')) return 'os-icon--windows'
  if (value.includes('lin')) return 'os-icon--linux'
  return 'os-icon--other'
}

const getOsTag = (os) => {
  const value = String(os || '').toLowerCase()
  if (value.includes('win')) return 'primary'
  if (value.includes('lin')) return 'warning'
  return 'info'
}

const formatTime = (iso) => {
  if (!iso || iso.startsWith('0001')) return '--'
  return new Date(iso).toLocaleString('zh-CN', { hour12: false })
}

const fetchListeners = async () => {
  const res = await api.get('/api/listeners')
  listeners.value = res.data || []
}

const openConnectDialog = async () => {
  connectDialogVisible.value = true
  await fetchListeners()
  if (bindListeners.value.length > 0) {
    connectForm.listener_id = bindListeners.value[0].id
  }
}

const openConnectFromContext = async () => {
  closeMenu()
  await openConnectDialog()
}

const handleConnect = async () => {
  connecting.value = true
  try {
    await api.post('/api/agents/connect', connectForm)
    ElMessage.success('接入指令已发送')
    connectDialogVisible.value = false
    setTimeout(fetchClients, 2000)
  } catch {
    ElMessage.error('接入指令发送失败')
  } finally {
    connecting.value = false
  }
}

const openContextMenu = (row, column, event) => {
  event.preventDefault()
  contextMenu.x = event.clientX
  contextMenu.y = event.clientY
  contextMenu.row = row
  contextMenu.visible = true
}

const closeMenu = () => {
  contextMenu.visible = false
}

const handleManageByContext = () => {
  if (!isAgentOnline(contextMenu.row?.status)) return
  handleManageRow(contextMenu.row)
  closeMenu()
}

const handleDeleteByContext = () => {
  ElMessageBox.confirm(`确认删除主机 ${contextMenu.row?.hostname || ''} 的记录吗？`, '删除记录', { type: 'warning' })
    .then(async () => {
      await deleteClient(contextMenu.row.uuid)
      ElMessage.success('主机记录已删除')
      fetchClients()
    })
    .catch(() => {})
  closeMenu()
}

onMounted(() => {
  fetchClients()
  timer = setInterval(fetchClients, 8000)
})

onUnmounted(() => {
  clearInterval(timer)
})
</script>

<style scoped>
.client-shell {
  gap: 20px;
}

.client-actions {
  justify-content: flex-end;
}

.client-table-card {
  padding-top: 20px;
}

.os-icon {
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
  border-radius: 12px;
  background: var(--surface-subtle);
}

.os-icon--windows {
  background: #eff6ff;
  color: #2563eb;
}

.os-icon--linux {
  background: #fff7ed;
  color: #d97706;
}

.os-icon--other {
  color: var(--text-strong);
}

.hostname-cell {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.hostname-text {
  font-size: 14px;
  font-weight: 700;
  color: var(--text-strong);
}

.uuid-label {
  font-size: 11px;
  color: var(--text-muted);
}

.user-cell {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.status-indicator {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 4px 12px;
  border-radius: 999px;
  background: var(--surface-muted);
  color: var(--text-strong);
  font-size: 12px;
  font-weight: 700;
}

.status-indicator .dot {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: var(--text-strong);
}

.status-indicator.online {
  background: rgba(16, 185, 129, 0.12);
  color: #047857;
}

.status-indicator.online .dot {
  background: #10b981;
}

.status-indicator.memory-online {
  background: rgba(99, 102, 241, 0.12);
  color: #4338ca;
}

.status-indicator.memory-online .dot {
  background: #6366f1;
}

.status-indicator.memory-offline {
  background: rgba(99, 102, 241, 0.06);
  color: #6b7280;
}

.status-indicator.memory-offline .dot {
  background: #9ca3af;
}

.manage-btn {
  font-size: 18px;
}

.dialog-stack {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.premium-context-menu {
  position: fixed;
  min-width: 210px;
  padding: 8px;
  border-radius: 18px;
  border: 1px solid var(--line-soft);
  background: rgba(255, 255, 255, 0.96);
  box-shadow: var(--shadow-soft);
  z-index: 3000;
}

.menu-header {
  padding: 10px 12px;
  border-bottom: 1px solid var(--line-soft);
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.14em;
}

.menu-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 11px 12px;
  border: 0;
  border-radius: 12px;
  background: transparent;
  color: var(--text-body);
  text-align: left;
  font-size: 13px;
  font-weight: 700;
  cursor: pointer;
}

.menu-item:hover {
  background: var(--surface-muted);
}

.menu-item.disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.menu-item--danger {
  color: #b42318;
}

.menu-divider {
  height: 1px;
  margin: 6px 4px;
  background: var(--line-soft);
}

@media (max-width: 1100px) {
  .client-actions {
    justify-content: flex-start;
  }
}

@media (max-width: 820px) {
  .client-actions {
    flex-direction: column;
    align-items: stretch;
  }
}

</style>
