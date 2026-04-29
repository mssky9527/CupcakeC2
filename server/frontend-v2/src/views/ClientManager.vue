<template>
  <div class="client-manager-container" @click="closeMenu">
    <!-- Header Area -->
    <div class="page-header glass-panel mb-24">
      <div class="header-content">
        <div class="title-section">
          <h2 class="main-title">受控端 <span class="purple-text">资产管理</span></h2>
          <p class="sub-title">实时监控与指挥中心</p>
        </div>
        <div class="action-section">
          <el-button class="premium-btn connect-btn" type="success" :icon="Plus" @click="openConnectDialog">
            正向连接受控端
          </el-button>
          <el-button class="premium-btn refresh-btn" :loading="loading" plain @click="fetchClients">
            <el-icon><Refresh /></el-icon>
          </el-button>
        </div>
      </div>
    </div>

    <!-- Stats Row (Bento style) -->
    <div class="stats-row mb-24">
      <div class="stat-module glass-panel">
        <div class="stat-icon-box blue">
          <el-icon><Monitor /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">注册总数</div>
          <div class="stat-value">{{ clients.length }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box green">
          <el-icon><CircleCheck /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">当前在线</div>
          <div class="stat-value">{{ onlineCount }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box purple">
          <el-icon><Promotion /></el-icon>
        </div>
        <div class="stat-info">
          <div class="stat-label">内存驻留</div>
          <div class="stat-value">{{ memoryCount }}</div>
        </div>
      </div>
    </div>

    <!-- Table Section -->
    <div class="table-module glass-panel">
      <el-table 
        :data="clients" 
        class="premium-table"
        v-loading="loading"
        @row-contextmenu="openContextMenu"
      >
        <el-table-column width="60" align="center">
          <template #default="scope">
            <div class="os-icon" :class="getOsClass(scope.row.os)">
              <el-icon v-if="scope.row.os?.toLowerCase().includes('win')"><Platform /></el-icon>
              <el-icon v-else><ChromeFilled /></el-icon>
            </div>
          </template>
        </el-table-column>

        <el-table-column prop="hostname" label="终端标识" min-width="180">
          <template #default="scope">
            <div class="hostname-cell">
              <span class="hostname-text">{{ scope.row.hostname }}</span>
              <span class="uuid-label">{{ scope.row.uuid.substring(0, 8) }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column prop="ip" label="资产 IP" width="150">
          <template #default="scope">
            <span class="mono-text">{{ scope.row.ip }}</span>
          </template>
        </el-table-column>

        <el-table-column prop="os" label="操作系统" width="140">
          <template #default="scope">
            <el-tag :type="getOsTag(scope.row.os)" class="premium-tag" effect="plain" round>
              {{ scope.row.os }}
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column prop="username" label="当前用户" min-width="140" show-overflow-tooltip>
          <template #default="scope">
             <div class="user-cell">
                <el-icon class="user-icon"><User /></el-icon>
                <span>{{ scope.row.username }}</span>
             </div>
          </template>
        </el-table-column>

        <el-table-column prop="last_seen" label="最后心跳" width="170" sortable>
          <template #default="scope">
            <span class="time-text">{{ formatTime(scope.row.last_seen) }}</span>
          </template>
        </el-table-column>

        <el-table-column label="通讯状态" fixed="right" width="120" align="center">
          <template #default="scope">
            <div class="status-indicator" :class="scope.row.status">
              <span class="dot"></span>
              {{ scope.row.status === 'online' ? '在线' : '离线' }}
            </div>
          </template>
        </el-table-column>

        <el-table-column label="管理" fixed="right" width="80" align="center">
          <template #default="scope">
            <el-button 
              type="primary" 
              link 
              class="manage-btn"
              @click="handleManageRow(scope.row)"
            >
              <el-icon><ArrowRight /></el-icon>
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- Dialogs remain similar but style them via global theme + scoped CSS -->
    <!-- [Dialogs omitted for brevity in thought but included in final write] -->
    
    <!-- Migration Dialog -->
    <el-dialog title="进程迁移 (Process Migration)" v-model="migrateDialogVisible" width="450px" class="premium-dialog" center>
      <div class="dialog-inner">
        <el-alert title="高风险操作" type="warning" :closable="false" show-icon description="我们将 Agent 注入到其他进程内存中。成功后磁盘文件将自动销毁。" />
        <el-form label-position="top" class="mt-20">
          <el-form-item label="目标进程映像名">
            <el-input v-model="migrateProcess" placeholder="例如: explorer.exe" prefix-icon="Promotion" />
          </el-form-item>
        </el-form>
      </div>
      <template #footer>
        <el-button @click="migrateDialogVisible = false">取消</el-button>
        <el-button type="primary" native-type="submit" class="purple-btn" @click="handleMigrate" :loading="migrating">立即注入</el-button>
      </template>
    </el-dialog>

    <!-- Connect Dialog -->
    <el-dialog title="正向 TCP 资产接入" v-model="connectDialogVisible" width="480px" class="premium-dialog" center>
      <el-form label-position="top">
        <el-form-item label="目标受控端公网/内网地址" required>
          <el-input v-model="connectForm.target_addr" placeholder="10.0.0.5:4444" prefix-icon="MapLocation" />
        </el-form-item>
        <el-form-item label="关联通讯监听器" required>
          <el-select v-model="connectForm.listener_id" placeholder="选择通信模板" style="width: 100%">
            <el-option 
              v-for="l in listeners.filter(i => i.protocol === '正向TCP' || i.protocol === 'Bind-TCP')" 
              :key="l.id" 
              :label="`${l.protocol} | Port: ${l.port}`"
              :value="l.id"
            />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="connectDialogVisible = false">关闭</el-button>
        <el-button type="primary" class="purple-btn" :loading="connecting" @click="handleConnect">发起握手</el-button>
      </template>
    </el-dialog>

    <!-- Updated Context Menu -->
    <div v-if="contextMenu.visible" :style="contextMenuStyle" class="premium-context-menu">
      <div class="menu-header">操作菜单 ({{ contextMenu.row?.hostname }})</div>
      <div class="menu-item" :class="{ disabled: contextMenu.row?.status !== 'online' }" @click="handleManageByContext()">
        <el-icon><Monitor /></el-icon> 进入交互终端
      </div>
      <div class="menu-item" :class="{ disabled: contextMenu.row?.status !== 'online' }" @click="openMigrateDialog()">
        <el-icon><Promotion /></el-icon> 内存迁移注入
      </div>
      <div class="menu-item" :class="{ disabled: contextMenu.row?.status !== 'online' }" @click="openTunnelDialog()">
        <el-icon><Connection /></el-icon> 建立数据隧道
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item delete" @click="handleDeleteByContext">
        <el-icon><Delete /></el-icon> 移除主机记录
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, reactive, computed, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox, ElLoading } from 'element-plus'
import { 
  Refresh, Monitor, Delete, ArrowRight, Connection, 
  Promotion, Plus, CircleCheck, User, Platform, ChromeFilled,
  MapLocation
} from '@element-plus/icons-vue'
import api, { deleteClient } from '../api/index'

const router = useRouter()
const clients = ref([])
const loading = ref(false)
let timer = null

const onlineCount = computed(() => clients.value.filter(c => c.status === 'online').length)
const memoryCount = computed(() => clients.value.filter(c => c.is_memory).length || 0)

// Context Menu
const contextMenu = reactive({ visible: false, x: 0, y: 0, row: null })
const contextMenuStyle = computed(() => ({ top: `${contextMenu.y}px`, left: `${contextMenu.x}px` }))
const openContextMenu = (row, column, event) => {
  event.preventDefault()
  contextMenu.x = event.clientX
  contextMenu.y = event.clientY
  contextMenu.row = row
  contextMenu.visible = true
}
const closeMenu = () => { contextMenu.visible = false }

// Dialog States
const migrateDialogVisible = ref(false)
const migrating = ref(false)
const migrateProcess = ref('explorer.exe')

const connectDialogVisible = ref(false)
const connecting = ref(false)
const listeners = ref([])
const connectForm = reactive({ target_addr: '', listener_id: '' })

const fetchClients = async () => {
  loading.value = true
  try {
    const res = await api.get('/api/clients')
    clients.value = res.data || []
  } catch (e) {
    ElMessage.error('获取列表失败')
  } finally {
    loading.value = false
  }
}

const handleManageRow = (row) => {
  router.push({ name: 'ClientDetail', params: { id: row.uuid } })
}

const getOsClass = (os) => {
  if (os?.toLowerCase().includes('win')) return 'os-win'
  if (os?.toLowerCase().includes('lin')) return 'os-lin'
  return 'os-other'
}

const getOsTag = (os) => {
  if (os?.toLowerCase().includes('win')) return 'primary'
  if (os?.toLowerCase().includes('lin')) return 'warning'
  return 'info'
}

const formatTime = (iso) => {
  if (!iso || iso.startsWith('0001')) return '---'
  return new Date(iso).toLocaleString('zh-CN', { hour12: false })
}

const openConnectDialog = async () => {
  connectDialogVisible.value = true
  const res = await api.get('/api/listeners')
  listeners.value = res.data || []
  if (listeners.value.length > 0) connectForm.listener_id = listeners.value[0].id
}

const handleConnect = async () => {
  connecting.value = true
  try {
    await api.post('/api/agents/connect', connectForm)
    ElMessage.success('连接指令已下发')
    connectDialogVisible.value = false
    setTimeout(fetchClients, 2000)
  } catch (e) { ElMessage.error('连接失败') }
  finally { connecting.value = false }
}

const handleMigrate = async () => {
  migrating.value = true
  try {
    await api.post('/api/clients/migrate', {
      uuid: contextMenu.row?.uuid,
      target_process: migrateProcess.value
    }) 
    ElMessage.success('指令已传达')
    migrateDialogVisible.value = false
  } catch (e) { ElMessage.error('操作核心拒绝') }
  finally { migrating.value = false }
}

const handleManageByContext = () => {
  handleManageRow(contextMenu.row)
  closeMenu()
}

const openMigrateDialog = () => {
  migrateDialogVisible.value = true
  migrateProcess.value = contextMenu.row?.os?.toLowerCase().includes('linux') ? '[kworker/u2:1]' : 'explorer.exe'
  closeMenu()
}

const handleDeleteByContext = () => {
  ElMessageBox.confirm(`确定删除 ${contextMenu.row?.hostname}?`, '确认', { type: 'warning' })
    .then(async () => {
      await deleteClient(contextMenu.row.uuid)
      ElMessage.success('已移除')
      fetchClients()
    })
  closeMenu()
}

onMounted(() => {
  fetchClients()
  timer = setInterval(fetchClients, 8000)
})
onUnmounted(() => clearInterval(timer))

</script>

<style scoped>
.client-manager-container {
  padding: 0;
  animation: slideUp 0.6s ease-out;
}

@keyframes slideUp {
  from { opacity: 0; transform: translateY(20px); }
  to { opacity: 1; transform: translateY(0); }
}

.mb-24 { margin-bottom: 24px; }

/* Premium Panes */
.glass-panel {
  background: rgba(255, 255, 255, 0.75);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: 1px solid rgba(124, 58, 237, 0.08);
  border-radius: 24px;
  box-shadow: 0 10px 30px rgba(124, 58, 237, 0.05);
}

/* Page Header */
.page-header { padding: 24px 32px; }
.header-content { display: flex; justify-content: space-between; align-items: center; }
.main-title { font-size: 26px; font-weight: 900; color: #1e1b4b; margin: 0; letter-spacing: -0.5px; }
.purple-text { color: #7c3aed; }
.sub-title { font-size: 13px; color: #64748b; margin-top: 4px; font-weight: 600; }

.premium-btn {
  border-radius: 12px;
  font-weight: 700;
  height: 42px;
  transition: all 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275);
}
.connect-btn { background: #7c3aed !important; border: none !important; box-shadow: 0 4px 15px rgba(124, 58, 237, 0.3); }
.connect-btn:hover { transform: scale(1.05); background: #6d28d9 !important; }

/* Stats Row */
.stats-row { display: grid; grid-template-columns: repeat(3, 1fr); gap: 20px; }
.stat-module { padding: 20px; display: flex; align-items: center; gap: 16px; }
.stat-icon-box {
  width: 50px; height: 50px;
  border-radius: 16px;
  display: flex; align-items: center; justify-content: center;
  font-size: 24px;
}
.stat-icon-box.purple { background: rgba(124, 58, 237, 0.1); color: #7c3aed; }
.stat-icon-box.green { background: rgba(16, 185, 129, 0.1); color: #10b981; }
.stat-icon-box.blue { background: rgba(14, 165, 233, 0.1); color: #0ea5e9; }

.stat-label { font-size: 11px; font-weight: 800; color: #94a3b8; text-transform: uppercase; letter-spacing: 0.5px; }
.stat-value { font-family: 'JetBrains Mono'; font-size: 28px; font-weight: 800; color: #1e1b4b; }

/* Table Styling */
.table-module { padding: 12px; overflow: hidden; }
.premium-table { background: transparent !important; }

:deep(.el-table__row) { transition: all 0.2s; cursor: pointer; }
:deep(.el-table__row:hover) { background: rgba(124, 58, 237, 0.02) !important; transform: scale(1.002); }

.os-icon {
  width: 32px; height: 32px;
  border-radius: 8px;
  display: flex; align-items: center; justify-content: center;
  font-size: 18px;
}
.os-win { background: #eff6ff; color: #3b82f6; }
.os-lin { background: #fffbeb; color: #d97706; }
.os-other { background: #f1f5f9; color: #64748b; }

.hostname-cell { display: flex; flex-direction: column; }
.hostname-text { font-weight: 800; color: #1e1b4b; font-size: 14px; }
.uuid-label { font-size: 10px; color: #94a3b8; font-family: 'JetBrains Mono'; }

.mono-text { font-family: 'JetBrains Mono'; font-weight: 700; color: #475569; font-size: 13px; }
.time-text { font-size: 12px; color: #64748b; font-weight: 500; }

.status-indicator {
  display: inline-flex; align-items: center; gap: 8px;
  padding: 4px 12px; border-radius: 20px;
  font-size: 11px; font-weight: 800;
}
.status-indicator.online { background: rgba(16, 185, 129, 0.1); color: #059669; }
.status-indicator.offline { background: rgba(241, 245, 249, 1); color: #94a3b8; }
.status-indicator.online .dot { width: 6px; height: 6px; background: #10b981; border-radius: 50%; box-shadow: 0 0 8px #10b981; }

.user-cell { display: flex; align-items: center; gap: 6px; font-size: 13px; color: #475569; font-weight: 600; }
.user-icon { color: #94a3b8; font-size: 14px; }

.manage-btn { font-size: 18px; color: #cbd5e1 !important; transition: all 0.2s; }
.manage-btn:hover { color: #7c3aed !important; transform: translateX(3px); }

/* Context Menu */
.premium-context-menu {
  position: fixed; background: rgba(255, 255, 255, 0.95);
  backdrop-filter: blur(10px); border: 1px solid rgba(124, 58, 237, 0.1);
  border-radius: 16px; box-shadow: 0 15px 35px rgba(124, 58, 237, 0.15);
  z-index: 3000; padding: 8px; min-width: 180px;
}
.menu-header { padding: 8px 12px; font-size: 10px; font-weight: 800; color: #94a3b8; text-transform: uppercase; border-bottom: 1px solid rgba(124, 58, 237, 0.05); margin-bottom: 4px; }
.menu-item {
  padding: 10px 14px; cursor: pointer; display: flex; align-items: center; gap: 10px;
  font-size: 13px; font-weight: 700; color: #475569; border-radius: 10px; transition: all 0.2s;
}
.menu-item:hover { background: rgba(124, 58, 237, 0.05); color: #7c3aed; }
.menu-item.delete:hover { background: #fff1f2; color: #f43f5e; }
.menu-divider { height: 1px; background: rgba(124, 58, 237, 0.05); margin: 6px 4px; }

/* Dialogs */
.purple-btn { background: #7c3aed !important; border: none !important; font-weight: 800; }
:deep(.el-dialog) { border-radius: 28px !important; overflow: hidden; backdrop-filter: blur(20px); }
:deep(.el-dialog__header) { padding: 24px !important; border-bottom: 1px solid rgba(124, 58, 237, 0.05) !important; }
:deep(.el-dialog__title) { font-weight: 900 !important; color: #1e1b4b !important; }
</style>
