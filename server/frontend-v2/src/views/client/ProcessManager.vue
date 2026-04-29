<template>
  <div class="process-explorer" @click="closeContextMenu">
    <!-- Sophisticated Toolbar -->
    <div class="proc-toolbar">
      <div class="left-tools">
        <el-input 
          v-model="searchQuery" 
          placeholder="搜索 PID / 进程名称 / 用户..." 
          clearable
          class="proc-search"
        >
          <template #prefix>
            <el-icon><Search /></el-icon>
          </template>
        </el-input>
        
        <div class="noise-filter">
          <span class="filter-label">隐藏系统进程:</span>
          <el-switch v-model="hideSystem" active-color="#13ce66" />
        </div>
      </div>

      <div class="right-tools">
        <el-button-group>
          <el-button :icon="Refresh" @click="fetchProcesses" :loading="loading">刷新列表</el-button>
        </el-button-group>
      </div>
    </div>

    <!-- Process Table -->
    <div class="explorer-body" @contextmenu.prevent="onGlobalContextMenu">
      <el-table
        :data="displayProcesses"
        style="width: 100%"
        height="100%"
        v-loading="loading"
        class="professional-proc-table"
        :row-class-name="getRowClass"
        @row-contextmenu="onRowContextMenu"
        :default-sort="{ prop: 'pid', order: 'ascending' }"
        size="small"
        stripe
      >
        <el-table-column prop="pid" label="PID" width="90" sortable fixed />
        
        <el-table-column prop="name" label="映像名称" min-width="250" sortable>
          <template #default="scope">
            <div class="name-cell">
              <img :src="getProcessIcon(scope.row.name)" class="proc-mini-icon" />
              <span class="p-name">{{ scope.row.name }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column prop="ppid" label="父进程 ID (PPID)" width="130" sortable />

        <el-table-column label="分类" width="130" align="center">
          <template #default="scope">
            <el-tag 
              v-if="scope.row.category === 'security'" 
              type="danger" 
              effect="dark" 
              class="type-tag"
            >
              SECURITY
            </el-tag>
            <el-tag 
              v-else-if="scope.row.category === 'system'" 
              type="info" 
              effect="plain" 
              class="type-tag"
            >
              SYSTEM
            </el-tag>
            <el-tag 
              v-else 
              type="success" 
              effect="light" 
              class="type-tag"
            >
              USER
            </el-tag>
          </template>
        </el-table-column>

        <el-table-column prop="user" label="所属用户" width="180" sortable show-overflow-tooltip>
          <template #default="scope">
            <span :class="{ 'system-user': isSystemUser(scope.row.user) }">
              {{ scope.row.user || 'Unknown' }}
            </span>
          </template>
        </el-table-column>

        <el-table-column prop="arch" label="架构" width="90" align="center">
          <template #default="scope">
            <el-tag size="small" effect="plain">{{ scope.row.arch || 'x64' }}</el-tag>
          </template>
        </el-table-column>

        <el-table-column label="操作" width="100" align="center" fixed="right">
          <template #default="scope">
            <el-popconfirm 
              :title="`确定强制结束进程 ${scope.row.name} (PID: ${scope.row.pid}) 吗?`"
              @confirm="handleKill(scope.row)"
              confirm-button-text="强制结束"
              confirm-button-type="danger"
              cancel-button-text="取消"
            >
              <template #reference>
                <el-button type="danger" link size="small" class="kill-link">结束</el-button>
              </template>
            </el-popconfirm>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- Right-Click Context Menu -->
    <div 
      v-show="contextMenu.visible" 
      class="custom-context-menu" 
      :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
    >
      <div class="menu-header" v-if="contextMenu.targetRow">
        {{ contextMenu.targetRow.name }} ({{ contextMenu.targetRow.pid }})
      </div>
      <div class="menu-item" @click="fetchProcesses">
        <el-icon><Refresh /></el-icon> 刷新列表
      </div>
      <el-divider style="margin: 4px 0" />
      <div 
        v-if="contextMenu.targetRow" 
        class="menu-item danger" 
        @click="handleKill(contextMenu.targetRow)"
      >
        <el-icon><CircleClose /></el-icon> 强制结束任务
      </div>
      <div 
        v-if="contextMenu.targetRow" 
        class="menu-item" 
        @click="copyInfo(contextMenu.targetRow)"
      >
        <el-icon><CopyDocument /></el-icon> 复制详细信息
      </div>
    </div>

    <div class="status-bar">
      <span>总进程数: {{ rawProcesses.length }}</span>
      <span v-if="searchQuery"> | 筛选后: {{ displayProcesses.length }}</span>
    </div>
  </div>
</template>

<script setup>
import { ref, reactive, computed, onMounted, onUnmounted, onActivated } from 'vue'
import { Search, Refresh, CircleClose, CopyDocument } from '@element-plus/icons-vue'
import { listProcesses, killProcess } from '@/api/process'
import { ElMessage, ElMessageBox } from 'element-plus'

const props = defineProps({
  clientId: { type: String, required: true }
})

const loading = ref(false)
const searchQuery = ref('')
const hideSystem = ref(false)
const rawProcesses = ref([])

// Fingerprinting Configuration
const securityKeywords = [
  '360tray', '360sd', 'ZhuDongFangYu', 'HipsDaemon', 'usysdiag', 
  'MsMpEng', 'NisSrv', 'kav', 'avp', 'mcshield', 
  'Sentinel', 'Cb', 'Falcon', 'Sysmon', 'defender', 'huorong'
]

const systemKeywords = [
  'smss', 'csrss', 'wininit', 'winlogon', 'services', 'lsass', 
  'svchost', 'spoolsv', 'explorer', 'System Idle Process', 'System',
  'Registry', 'Memory Compression', 'Interrupts'
]

const analyzeProcess = (name) => {
  if (!name) return 'user'
  const lowerName = name.toLowerCase()
  if (securityKeywords.some(key => lowerName.includes(key.toLowerCase()))) return 'security'
  if (systemKeywords.some(key => lowerName.includes(key.toLowerCase()))) return 'system'
  return 'user'
}

const isSystemUser = (user) => {
  if (!user) return false
  const u = user.toUpperCase()
  return u === 'SYSTEM' || u === 'SERVICES' || u.includes('NT AUTHORITY')
}

// Data Handling
const fetchProcesses = async () => {
  if (!props.clientId) return
  loading.value = true
  try {
    const res = await listProcesses(props.clientId)
    if (res.data && res.data.data) {
      rawProcesses.value = res.data.data
      ElMessage.success({ message: `获取成功: ${res.data.data.length} 个进程`, duration: 1000 })
    }
  } catch (e) {
    ElMessage.error('无法获取进程列表: ' + (e.response?.data?.error || e.message))
  } finally {
    loading.value = false
  }
}

const displayProcesses = computed(() => {
  let list = rawProcesses.value.map(p => ({
    ...p,
    category: analyzeProcess(p.name)
  }))
  if (hideSystem.value) list = list.filter(p => p.category !== 'system')
  if (searchQuery.value) {
    const q = searchQuery.value.toLowerCase()
    list = list.filter(p => 
      p.name.toLowerCase().includes(q) || 
      p.pid.toString().includes(q) ||
      (p.user && p.user.toLowerCase().includes(q))
    )
  }
  return list
})

const handleKill = async (row) => {
  try {
    await killProcess({ uuid: props.clientId, pid: row.pid })
    ElMessage.success(`已发送结束指令: ${row.name}`)
    // Refresh after a short delay to allow process to terminate
    setTimeout(fetchProcesses, 1500)
  } catch (error) {
    ElMessage.error('操作失败: ' + (error.response?.data?.error || '未知错误'))
  }
}

const getRowClass = ({ row }) => {
  if (row.category === 'security') return 'security-row-highlight'
  if (row.category === 'system') return 'system-row-dim'
  return ''
}

const getProcessIcon = (name) => {
  const cat = analyzeProcess(name)
  if (cat === 'security') return 'https://img.icons8.com/color/48/000000/shield.png'
  if (cat === 'system') return 'https://img.icons8.com/color/48/000000/windows-shortcut-default.png'
  return 'https://img.icons8.com/color/48/000000/application-window.png'
}

const copyInfo = (row) => {
  const info = `Name: ${row.name}\nPID: ${row.pid}\nPPID: ${row.ppid}\nUser: ${row.user}\nArch: ${row.arch}`
  navigator.clipboard.writeText(info)
  ElMessage.success('已复制详细信息')
}

// Context Menu
const contextMenu = reactive({ visible: false, x: 0, y: 0, targetRow: null })
const onRowContextMenu = (row, column, event) => {
  contextMenu.visible = true
  contextMenu.x = event.clientX
  contextMenu.y = event.clientY
  contextMenu.targetRow = row
}
const onGlobalContextMenu = (event) => {
  if (!contextMenu.visible) {
    contextMenu.visible = true; contextMenu.x = event.clientX; contextMenu.y = event.clientY
  }
}
const closeContextMenu = () => { contextMenu.visible = false }

onActivated(() => {
  if (rawProcesses.value.length === 0) {
    fetchProcesses()
  }
})

onMounted(() => {
  fetchProcesses()
})
</script>

<style scoped>
.process-explorer { height: 100%; display: flex; flex-direction: column; background: #ffffff; }
.proc-toolbar { padding: 10px 15px; background: #f8f9fa; border-bottom: 1px solid #ebeef5; display: flex; justify-content: space-between; align-items: center; }
.left-tools { display: flex; align-items: center; gap: 20px; }
.proc-search { width: 300px; }
.noise-filter { display: flex; align-items: center; gap: 10px; }
.filter-label { font-size: 13px; color: #606266; font-weight: 500; }
.explorer-body { flex: 1; overflow: hidden; }
.name-cell { display: flex; align-items: center; gap: 12px; }
.proc-mini-icon { width: 20px; height: 20px; }
.p-name { font-family: 'Segoe UI', sans-serif; font-weight: 600; color: #2c3e50; }
.system-user { color: #909399; font-style: italic; font-size: 12px; }
.type-tag { font-family: 'Inter', sans-serif; font-size: 10px; font-weight: 800; border-radius: 4px; }
:deep(.security-row-highlight) { background-color: #fff1f0 !important; }
:deep(.security-row-highlight:hover > td) { background-color: #ffccc7 !important; }
:deep(.security-row-highlight) .p-name { color: #cf1322 !important; }
:deep(.system-row-dim) { opacity: 0.85; }
:deep(.system-row-dim) td { color: #909399; }
.custom-context-menu { position: fixed; z-index: 3000; background: white; border: 1px solid #e4e7ed; box-shadow: 0 4px 12px rgba(0,0,0,0.1); padding: 4px 0; border-radius: 8px; min-width: 200px; }
.menu-header { padding: 8px 16px; font-size: 11px; color: #909399; border-bottom: 1px solid #f2f6fc; margin-bottom: 4px; font-weight: bold; }
.menu-item { padding: 10px 16px; font-size: 13px; color: #606266; cursor: pointer; display: flex; align-items: center; gap: 10px; }
.menu-item:hover { background: #f5f7fa; color: #409eff; }
.menu-item.danger:hover { background: #fff1f0; color: #f56c6c; }

.status-bar {
  padding: 5px 15px;
  font-size: 12px;
  color: #909399;
  border-top: 1px solid #ebeef5;
  background: #fafafa;
}
</style>
