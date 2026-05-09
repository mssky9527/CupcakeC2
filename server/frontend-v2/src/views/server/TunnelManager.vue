<template>
  <div class="tunnel-manager-container">
    <div class="page-header glass-panel mb-24">
      <div class="header-content">
        <div class="title-section">
          <h2 class="main-title">全局 <span class="purple-text">隧道路由</span></h2>
          <p class="sub-title">内网穿透数据转发管理 (Network Routing Control Center)</p>
        </div>
        <el-button class="premium-btn refresh-btn" :loading="loading" @click="fetchData">
          <el-icon><Refresh /></el-icon> 刷新状态
        </el-button>
      </div>
    </div>

    <div class="stats-row mb-24">
      <div class="stat-module glass-panel">
        <div class="stat-icon-box blue"><el-icon><Connection /></el-icon></div>
        <div class="stat-info">
          <div class="stat-label">活跃隧道</div>
          <div class="stat-value">{{ tunnels.filter(t => t.status === 'running').length }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box purple"><el-icon><Share /></el-icon></div>
        <div class="stat-info">
          <div class="stat-label">注册端口数</div>
          <div class="stat-value">{{ tunnels.length }}</div>
        </div>
      </div>
      <div class="stat-module glass-panel">
        <div class="stat-icon-box green"><el-icon><User /></el-icon></div>
        <div class="stat-info">
          <div class="stat-label">转发后端</div>
          <div class="stat-value">{{ Array.from(new Set(tunnels.map(t => t.agent_id))).length }}</div>
        </div>
      </div>
    </div>

    <div class="table-module glass-panel">
      <el-table :data="tunnels" v-loading="loading" class="premium-table">
        <el-table-column label="本端监听 (Local Listener)" width="200">
          <template #default="scope">
            <div class="local-addr">
              <span class="addr-prefix">0.0.0.0:</span>
              <span class="addr-port">{{ scope.row.port }}</span>
              <el-tag size="small" class="protocol-tag">{{ scope.row.type?.toUpperCase?.() || 'SOCKS5' }}</el-tag>
            </div>
          </template>
        </el-table-column>

        <el-table-column label="隧道出口 (Agent Asset)" min-width="280">
          <template #default="scope">
            <div v-if="scope.row.agent_ip" class="agent-trace">
              <router-link :to="'/client/' + scope.row.agent_id" class="asset-link">
                <el-icon><Monitor /></el-icon>
                <span class="asset-ip">{{ scope.row.agent_ip }}</span>
                <span class="asset-sep">/</span>
                <span class="asset-name">{{ scope.row.agent_name }}</span>
              </router-link>
              <div class="asset-uuid">UUID: {{ scope.row.agent_id?.substring(0, 16) }}...</div>
            </div>
            <el-tag v-else type="info" class="premium-tag" effect="plain" round>资源暂不可用</el-tag>
          </template>
        </el-table-column>

        <el-table-column label="身份验证" width="140">
          <template #default="scope">
            <div class="auth-status" :class="{ enabled: scope.row.username }">
              <el-icon><Lock /></el-icon>
              {{ scope.row.username ? '已启用认证' : '匿名访问' }}
            </div>
          </template>
        </el-table-column>

        <el-table-column label="路由状态" width="120" align="center">
          <template #default="scope">
            <div class="status-indicator" :class="scope.row.status">
              <span class="dot"></span>
              {{ scope.row.status === 'running' ? '已激活' : '已断开' }}
            </div>
          </template>
        </el-table-column>

        <el-table-column label="管理" width="100" align="center" fixed="right">
          <template #default="scope">
            <el-dropdown trigger="click" @command="handleCommand($event, scope.row)">
              <div class="manage-trigger"><el-icon><Setting /></el-icon></div>
              <template #dropdown>
                <el-dropdown-menu class="premium-dropdown">
                  <el-dropdown-item command="start" v-if="scope.row.status !== 'running'" icon="VideoPlay" class="item-green">远程激活</el-dropdown-item>
                  <el-dropdown-item command="stop" v-if="scope.row.status === 'running'" icon="VideoPause" class="item-orange">强制熔断</el-dropdown-item>
                  <el-dropdown-item command="edit" icon="Edit" divided>编辑路由</el-dropdown-item>
                  <el-dropdown-item command="delete" icon="Delete" class="item-red">永久移除</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-dialog
      :title="isEdit ? '修正隧道路由' : '建立新转发链路'"
      v-model="editDialogVisible"
      width="480px"
      class="premium-dialog"
      center
    >
      <div class="dialog-inner">
        <el-form label-position="top">
          <el-form-item label="核心监听端口 (Local Management Port)">
            <el-input-number v-model="editForm.port" :min="1" style="width: 100%" controls-position="right" />
          </el-form-item>
          <el-form-item label="隧道载体协议">
            <el-radio-group v-model="editForm.type" class="platform-tabs">
              <el-radio-button label="socks5">SOCKS5 (PROXY)</el-radio-button>
              <el-radio-button label="http">HTTP (PROXY)</el-radio-button>
            </el-radio-group>
          </el-form-item>

          <div class="auth-section-box glass-panel">
            <div class="section-title-line">
              <el-switch v-model="editForm.enableAuth" />
              <span class="label">ACL 访问控制验证</span>
            </div>

            <transition name="fade">
              <div v-if="editForm.enableAuth" class="auth-fields mt-15">
                <el-row :gutter="15">
                  <el-col :span="12">
                    <el-form-item label="用户名">
                      <el-input v-model="editForm.username" :prefix-icon="User" />
                    </el-form-item>
                  </el-col>
                  <el-col :span="12">
                    <el-form-item label="密码">
                      <el-input v-model="editForm.password" type="password" show-password :prefix-icon="Key" />
                    </el-form-item>
                  </el-col>
                </el-row>
              </div>
            </transition>
          </div>
        </el-form>
      </div>
      <template #footer>
        <div class="dialog-footer">
          <el-button @click="editDialogVisible = false" class="plain-btn">取消</el-button>
          <el-button type="primary" class="purple-btn" :loading="submitting" @click="submitEdit">保存并同步核心</el-button>
        </div>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { ref, reactive, onMounted } from 'vue'
import { getActiveTunnels, stopTunnel, startTunnel, deleteTunnel } from '@/api/socks'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Refresh, Connection, Share, Monitor, User, Lock, Setting,
  Key
} from '@element-plus/icons-vue'

const tunnels = ref([])
const loading = ref(false)
const editDialogVisible = ref(false)
const submitting = ref(false)
const isEdit = ref(false)
const currentAgentId = ref('')
const oldPort = ref('')

const editForm = reactive({
  port: 1080,
  type: 'socks5',
  enableAuth: false,
  username: '',
  password: ''
})

const fetchData = async () => {
  loading.value = true
  try {
    const res = await getActiveTunnels()
    tunnels.value = res.data?.tunnels || []
  } catch (e) {
    ElMessage.error('无法同步数据')
  } finally {
    loading.value = false
  }
}

const handleCommand = (command, row) => {
  if (command === 'start') handleRestart(row)
  else if (command === 'stop') handleStop(row.port)
  else if (command === 'delete') handleDelete(row.port)
  else if (command === 'edit') handleEdit(row)
}

const handleRestart = async (row) => {
  try {
    await startTunnel({
      uuid: row.agent_id,
      port: String(row.port),
      type: row.type,
      username: row.username || '',
      password: row.password || ''
    })
    ElMessage.success('隧道已重启')
    fetchData()
  } catch (e) {
    ElMessage.error('激活请求被拒绝')
  }
}

const handleStop = async (port) => {
  await stopTunnel({ port })
  ElMessage.warning('链路已手动熔断')
  fetchData()
}

const handleDelete = (port) => {
  ElMessageBox.confirm('彻底移除该路由配置吗？', '注销确认', { type: 'error' }).then(async () => {
    await deleteTunnel({ port })
    ElMessage.success('已移除')
    fetchData()
  })
}

const handleEdit = (row) => {
  isEdit.value = true
  currentAgentId.value = row.agent_id
  oldPort.value = String(row.port)
  editForm.port = parseInt(row.port)
  editForm.type = row.type || 'socks5'
  editForm.username = row.username || ''
  editForm.password = row.password || ''
  editForm.enableAuth = !!editForm.username
  editDialogVisible.value = true
}

const submitEdit = async () => {
  submitting.value = true
  try {
    await startTunnel({
      uuid: currentAgentId.value,
      port: String(editForm.port),
      type: editForm.type,
      username: editForm.enableAuth ? editForm.username : '',
      password: editForm.enableAuth ? editForm.password : ''
    })
    if (String(editForm.port) !== oldPort.value) await deleteTunnel({ port: oldPort.value })
    ElMessage.success('同步成功')
    editDialogVisible.value = false
    fetchData()
  } catch (e) {
    ElMessage.error('变更同步失败')
  } finally {
    submitting.value = false
  }
}

onMounted(fetchData)
</script>

<style scoped>
.tunnel-manager-container { padding: 0; }
.mb-24 { margin-bottom: 24px; }
.mt-15 { margin-top: 15px; }
.glass-panel {
  background: rgba(255, 255, 255, 0.75);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  border: 1px solid var(--accent-soft);
  border-radius: 24px;
  box-shadow: 0 10px 30px var(--line-soft);
}
.purple-text { color: var(--accent); }
.premium-btn { background: var(--accent) !important; border: none !important; color: white !important; font-weight: 800; border-radius: 12px; height: 38px; padding: 0 16px; transition: all 0.2s; }
.premium-btn:hover { transform: translateY(-1px); box-shadow: 0 4px 15px var(--accent-focus); }
.page-header { padding: 24px 28px; }
.header-content { display: flex; align-items: center; justify-content: space-between; }
.main-title { margin: 0; color: var(--text-strong); font-size: 26px; font-weight: 900; }
.sub-title { margin: 6px 0 0; color: var(--text-muted); font-size: 13px; font-weight: 600; }
.stats-row { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
.stat-module { min-height: 84px; padding: 16px 20px; display: flex; align-items: center; gap: 14px; }
.stat-icon-box { width: 42px; height: 42px; border-radius: 14px; display: flex; align-items: center; justify-content: center; font-size: 18px; }
.stat-icon-box.purple { background: var(--accent-soft); color: var(--accent); }
.stat-icon-box.green { background: rgba(16, 185, 129, 0.1); color: #10b981; }
.stat-icon-box.blue { background: rgba(14, 165, 233, 0.1); color: #0ea5e9; }
.stat-label { font-size: 12px; font-weight: 700; color: var(--text-muted); line-height: 1.25; }
.stat-value { font-family: 'JetBrains Mono'; font-size: 26px; font-weight: 800; color: var(--text-strong); line-height: 1; margin-top: 6px; }
.table-module { padding: 12px; }
.premium-table { background: transparent !important; }
.local-addr { display: flex; align-items: center; gap: 8px; font-family: 'JetBrains Mono'; }
.addr-prefix { color: var(--text-muted); font-weight: 600; }
.addr-port { color: var(--text-strong); font-weight: 800; font-size: 15px; }
.protocol-tag { background: var(--accent) !important; border: none !important; color: white !important; font-weight: 900; font-size: 10px; border-radius: 6px; }
.asset-link { display: flex; align-items: center; gap: 8px; color: var(--text-strong); text-decoration: none; font-weight: 800; }
.asset-uuid { margin-top: 4px; color: var(--text-muted); font-size: 11px; font-family: 'JetBrains Mono'; }
.auth-status { display: flex; align-items: center; gap: 6px; color: var(--text-muted); font-weight: 700; }
.auth-status.enabled { color: var(--text-strong); }
.status-indicator { display: inline-flex; align-items: center; gap: 8px; color: var(--text-muted); font-weight: 800; }
.status-indicator.running { color: #059669; }
.dot { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
.manage-trigger { cursor: pointer; color: var(--text-strong); font-size: 18px; }
.dialog-inner { padding: 8px; }
.auth-section-box { padding: 16px; border-radius: 12px; }
.section-title-line { display: flex; align-items: center; gap: 10px; font-weight: 700; color: var(--text-strong); }
</style>
