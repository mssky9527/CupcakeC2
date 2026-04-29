<template>
  <div class="terminal-tabs-container">
    <div class="tabs-header">
      <el-tabs
        v-model="activeTabName"
        type="border-card"
        closable
        @tab-remove="handleTabRemove"
        class="terminal-tabs"
      >
        <el-tab-pane
          v-for="tab in tabs"
          :key="tab.name"
          :label="tab.title"
          :name="tab.name"
        >
          <template #label>
            <span class="tab-label">
              <el-icon><Monitor /></el-icon>
              {{ tab.title }}
            </span>
          </template>
        </el-tab-pane>
      </el-tabs>
      <el-button
        type="primary"
        :icon="Plus"
        circle
        size="small"
        @click="addNewTab"
        class="add-tab-btn"
        title="新建终端"
      />
    </div>

    <!-- Terminal Content Area -->
    <div class="terminal-content">
      <div v-if="tabs.length === 0" style="padding: 40px; text-align: center; color: #666;">
        正在初始化终端...
      </div>
      
      <div
        v-for="tab in tabs"
        :key="tab.name"
        v-show="activeTabName === tab.name"
        class="terminal-instance"
      >
        <div class="terminal-box">
          <!-- CobaltStrike / VShell Style HUD Status Bar -->
          <div class="terminal-hud">
            <div class="hud-left">
              <span class="status-dot online"></span>
              <span class="hud-text primary">{{ clientInfo?.hostname }}</span>
              <span class="hud-divider">/</span>
              <span class="hud-text secondary">{{ clientInfo?.ip }}</span>
              <el-tag size="small" type="success" class="hud-tag" effect="dark" round>PTY ACTIVE</el-tag>
            </div>
            <div class="hud-right">
              <span class="hud-label">USER:</span>
              <span class="hud-value">{{ clientInfo?.username || 'N/A' }}</span>
              <span class="hud-divider">|</span>
              <span class="hud-label">OS:</span>
              <span class="hud-value">{{ clientInfo?.os || 'Windows' }}</span>
            </div>
          </div>

          <!-- WebTerminal acts as the Output Display -->
          <div class="terminal-display-wrapper">
             <!-- PTY is only enabled for "Live Shell" tabs if we choose to add a button for it.
                  For now, let's keep the default shell as non-PTY (Legacy) unless explicitly requested.
                  OR, upgraded to PTY if available. 
                  Let's assume Tab 1 is always Legacy (Command/Response), 
                  and user can click "New PTY Tab" to open a real shell. -->
             <WebTerminal 
               :ref="el => setTerminalRef(tab.name, el)"
               :socket="socket"
               :client-id="clientId"
               :allow-p-t-y="tab.isPTY" 
             />
          </div>
          
          <!-- Input Area (Only needed for Legacy Tabs. PTY has direct xterm input) -->
          <el-input
            v-if="!tab.isPTY"
            v-model="tab.input"
            placeholder="输入 Shell 命令并回车..."
            @keyup.enter="sendCommand(tab)"
            :disabled="tab.submitting"
            class="terminal-input-bar"
          >
            <template #prefix>
              <el-icon><Right /></el-icon>
            </template>
          </el-input>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, reactive, onMounted, defineProps, defineExpose } from 'vue'
import { Monitor, Plus, Right } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import api from '../api/index'
import WebTerminal from './WebTerminal.vue'

const props = defineProps({
  clientId: {
    type: String,
    required: true
  },
  clientInfo: {
    type: Object,
    default: null
  },
  socket: {
    type: Object,
    default: null
  }
})

// Tab management
const tabs = ref([])
const activeTabName = ref('')
let tabCounter = 0

// Terminal refs
const terminalRefs = reactive({})

const setTerminalRef = (name, el) => {
  if (el) {
    terminalRefs[name] = el
  }
}

// Global Message Handler (Called by ClientDetail)
const handleSocketMessage = (event) => {
    // Broadcast to all active terminals for now
    // Since we don't have session routing in the WebTerminal logic yet.
    Object.values(terminalRefs).forEach(termComp => {
        if (termComp && termComp.handleSocketMessage) {
            termComp.handleSocketMessage(event)
        }
    })
}

// Create initial tab
const createTab = (isPTY = false) => {
  tabCounter++
  const sessionId = `session-${Date.now()}-${tabCounter}`
  return {
    name: sessionId,
    title: isPTY ? `Interactive PTY ${tabCounter}` : `Shell ${tabCounter}`,
    sessionId: sessionId,
    isPTY: isPTY,
    input: '',
    submitting: false
  }
}

const addNewTab = () => {
    // Default to Legacy for now, or add a Dropdown to choose. 
    // Let's make "New Tab" button default to PTY if user holds Shift? 
    // Or just alternating? 
    // Let's default to PTY for better UX if backend supports it.
    const newTab = createTab(true) // Default to PTY
    tabs.value.push(newTab)
    activeTabName.value = newTab.name
}

const handleTabRemove = (targetName) => {
  if (tabs.value.length === 1) {
    ElMessage.warning('至少保留一个终端')
    return
  }
  
  const index = tabs.value.findIndex(tab => tab.name === targetName)
  if (index !== -1) {
    tabs.value.splice(index, 1)
    delete terminalRefs[targetName]
    if (activeTabName.value === targetName) {
      activeTabName.value = tabs.value[Math.max(0, index - 1)].name
    }
  }
}

const sendCommand = async (tab) => {
  if (!tab.input.trim()) return
  
  const cmd = tab.input
  tab.input = ''
  tab.submitting = true
  
  // Local echo is confusing if using xterm and we don't control the cursor perfectly.
  // But let's assume valid output comes from server.
  // Maybe valid shell output includes the command echoing? 
  // If not, we might want to manually write it:
  // if (terminalRefs[tab.name]) terminalRefs[tab.name].term.writeln(`> ${cmd}`) 
  // (We can't access term directly easily unless we expose it or use handleSocketMessage to fake it)

  try {
    await api.post('/api/cmd', {
      uuid: props.clientId,
      cmd: cmd,
      session_id: tab.sessionId 
    })
  } catch (e) {
    ElMessage.error('命令下发失败')
  } finally {
    tab.submitting = false
  }
}

onMounted(() => {
  // First tab defaults to PTY interactive shell
  const ptyTab = createTab(true)
  ptyTab.title = "Interactive Shell"
  tabs.value.push(ptyTab)
  activeTabName.value = ptyTab.name
})

// Expose for parent
defineExpose({ handleSocketMessage })
</script>

<style scoped>
@import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@500;800&display=swap');

.terminal-tabs-container {
  height: 100%;
  display: flex;
  flex-direction: column;
  background-color: #ffffff;
  border-radius: 16px;
  box-shadow: 0 4px 20px rgba(124, 58, 237, 0.05);
  border: 1px solid rgba(124, 58, 237, 0.08);
}

/* Tabs Header */
.tabs-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  background-color: #ffffff;
  border-bottom: 1px solid rgba(124, 58, 237, 0.08);
  border-radius: 16px 16px 0 0;
}

.terminal-tabs {
  flex: 1;
}

:deep(.el-tabs__header) {
  margin: 0;
  border: none !important;
  background: transparent !important;
}

:deep(.el-tabs--border-card) {
  background: transparent !important;
  border: none !important;
  box-shadow: none !important;
}

:deep(.el-tabs--border-card .el-tabs__item) {
  border: none !important;
  background-color: transparent !important;
  color: #64748b !important;
  font-weight: 800;
  font-size: 13px;
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  border-radius: 8px;
  margin-right: 4px;
}

:deep(.el-tabs--border-card .el-tabs__item:hover) {
  color: #7c3aed !important;
  background: rgba(124, 58, 237, 0.04) !important;
}

:deep(.el-tabs--border-card .el-tabs__item.is-active) {
  background-color: #ffffff !important;
  color: #7c3aed !important;
  border: 1px solid rgba(124, 58, 237, 0.2) !important;
  box-shadow: 0 4px 12px rgba(124, 58, 237, 0.1);
}

:deep(.el-tabs__content) {
  display: none;
}

.tab-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-family: 'Inter', sans-serif;
}

.add-tab-btn {
  background-color: rgba(124, 58, 237, 0.04) !important;
  border: 1px solid rgba(124, 58, 237, 0.1) !important;
  color: #7c3aed !important;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}
.add-tab-btn:hover {
  background-color: #7c3aed !important;
  color: #ffffff !important;
  border: none !important;
}

/* HUD System Details Panel */
.terminal-hud {
  display: flex;
  justify-content: space-between;
  align-items: center;
  background-color: #ffffff;
  padding: 10px 20px;
  border-bottom: 1px solid rgba(124, 58, 237, 0.08);
}

.hud-left, .hud-right { display: flex; align-items: center; gap: 12px; }

.status-dot { width: 8px; height: 8px; border-radius: 50%; }
.status-dot.online { background: #10b981; box-shadow: 0 0 10px rgba(16, 185, 129, 0.5); }

.hud-text { font-family: 'JetBrains Mono', monospace; font-size: 13px; font-weight: 800; }
.hud-text.primary { color: #1e1b4b; }
.hud-text.secondary { color: #7c3aed; }

.hud-divider { color: rgba(124, 58, 237, 0.2); font-size: 14px; }
.hud-tag { font-size: 9px; font-weight: 900; background: rgba(16, 185, 129, 0.1) !important; color: #10b981 !important; border: 1px solid rgba(16, 185, 129, 0.2) !important; }

.hud-label { color: #94a3b8; font-size: 11px; font-weight: 800; }
.hud-value { color: #1e1b4b; font-family: 'JetBrains Mono', monospace; font-size: 12px; font-weight: 700; }

/* Output Container */
.terminal-content {
  flex: 1;
  overflow: hidden;
  position: relative;
  background-color: #000000;
  border-radius: 0 0 16px 16px;
}

.terminal-instance {
  position: absolute;
  top: 0; left: 0; right: 0; bottom: 0;
}

.terminal-box {
  height: 100%;
  background-color: #000000;
  display: flex;
  flex-direction: column;
}

.terminal-display-wrapper {
  flex: 1;
  overflow: hidden;
  position: relative;
}

:deep(.terminal-wrapper) {
    height: 100%;
    padding: 12px !important;
}

.terminal-input-bar {
  --el-input-bg-color: #1e1b4b !important;
  --el-input-text-color: #ffffff;
  --el-input-border-color: rgba(124, 58, 237, 0.2) !important;
  border-radius: 0 0 16px 16px !important;
}
</style>


